//! Folder scanning and Explorer-like natural sorting.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// File extensions muud-view can decode (lower case, without the dot).
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "apng", "gif", "webp", "bmp", "dib", "tif", "tiff", "tga",
    "ico", "pnm", "pbm", "pgm", "ppm", "pam", "qoi", "dds", "hdr", "exr", "svg", "psd",
];

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_ascii_lowercase();
            IMAGE_EXTENSIONS.contains(&e.as_str())
                || super::formats::RAW_EXTENSIONS.contains(&e.as_str())
        })
        .unwrap_or(false)
}

/// What to show after opening `target` (a file or a folder).
pub struct Listing {
    pub files: Vec<PathBuf>,
    pub index: usize,
}

/// Lists every image next to `target` (or inside it, if it is a folder),
/// sorted naturally, and returns the index of `target` in that list.
#[cfg(test)]
pub fn list_for(target: &Path) -> std::io::Result<Listing> {
    list_sorted(target, crate::core::settings::SortBy::Name, false)
}

/// Like [`list_for`], with a choice of order.
pub fn list_sorted(
    target: &Path,
    sort: crate::core::settings::SortBy,
    desc: bool,
) -> std::io::Result<Listing> {
    let (dir, selected) = if target.is_dir() {
        (target.to_path_buf(), None)
    } else {
        let dir = target
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        (dir, Some(target.to_path_buf()))
    };

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| is_image(p))
        .collect();

    // A file opened explicitly is always shown, even with an unusual extension.
    if let Some(sel) = &selected {
        if !files.iter().any(|f| same_file(f, sel)) {
            files.push(sel.clone());
        }
    }

    sort_files(&mut files, sort, desc);

    let index = selected
        .and_then(|sel| files.iter().position(|f| same_file(f, &sel)))
        .unwrap_or(0);
    Ok(Listing { files, index })
}

/// True if `dir` (a folder or an archive) holds at least one image.
fn has_images(dir: &Path) -> bool {
    if super::archive::is_archive(dir) {
        return super::archive::list(dir).is_ok_and(|l| !l.is_empty());
    }
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.filter_map(Result::ok)
            .any(|e| e.file_type().is_ok_and(|t| t.is_file()) && is_image(&e.path()))
    })
}

/// The next (`delta` = 1) or previous (-1) folder or archive next to
/// `container` that has images, in natural name order (`]` / `[`).
pub fn sibling(container: &Path, delta: i32) -> Option<PathBuf> {
    let parent = container.parent()?;
    let mut items: Vec<PathBuf> = std::fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() || (p.is_file() && super::archive::is_archive(p)))
        .collect();
    items.sort_by(|a, b| natural_cmp(&file_name(a), &file_name(b)));
    let at = items.iter().position(|p| p == container)?;
    let mut i = at as i64;
    loop {
        i += delta.signum() as i64;
        let candidate = items.get(usize::try_from(i).ok()?)?;
        if has_images(candidate) {
            return Some(candidate.clone());
        }
    }
}

pub fn sort_files(files: &mut [PathBuf], sort: crate::core::settings::SortBy, desc: bool) {
    sort_files_with(files, sort, desc, |p| {
        std::fs::metadata(p)
            .ok()
            .map(|m| (m.len(), m.modified().ok()))
    });
}

/// [`sort_files`] with size and time from `stamp` (e.g. the folder
/// watcher's listing) instead of asking the disk about each file.
pub fn sort_files_with(
    files: &mut [PathBuf],
    sort: crate::core::settings::SortBy,
    desc: bool,
    stamp: impl Fn(&Path) -> Option<super::watch::Stamp>,
) {
    use crate::core::settings::SortBy;
    match sort {
        SortBy::Name => files.sort_by(|a, b| natural_cmp(&file_name(a), &file_name(b))),
        SortBy::Modified => files.sort_by_cached_key(|p| stamp(p).and_then(|s| s.1)),
        SortBy::Size => files.sort_by_cached_key(|p| stamp(p).map_or(0, |s| s.0)),
    }
    if desc {
        files.reverse();
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.file_name(), b.file_name()) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(y) && a.parent() == b.parent(),
        _ => false,
    }
}

pub fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Compares two names the way Windows Explorer does: case-insensitively, with
/// runs of digits compared by numeric value (`img2` < `img10`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = take_digits(&mut ai);
                let nb = take_digits(&mut bi);
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let ord = ta
                    .len()
                    .cmp(&tb.len())
                    .then_with(|| ta.cmp(tb))
                    .then_with(|| nb.len().cmp(&na.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

fn take_digits(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(c) = it.peek().copied() {
        if c.is_ascii_digit() {
            s.push(c);
            it.next();
        } else {
            break;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| natural_cmp(a, b));
        v
    }

    #[test]
    fn next_and_previous_folder_skip_ones_without_images() {
        let dir = crate::core::archive::tests::temp_dir("sibling");
        for (name, img) in [("a", true), ("b", false), ("c", true)] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
            if img {
                std::fs::write(dir.join(name).join("1.png"), b"x").unwrap();
            }
        }
        let zip = crate::core::archive::tests::raw_zip(&[(b"1.png", b"x")]);
        std::fs::write(dir.join("d.cbz"), zip).unwrap();
        assert_eq!(sibling(&dir.join("a"), 1), Some(dir.join("c")));
        assert_eq!(sibling(&dir.join("c"), 1), Some(dir.join("d.cbz")));
        assert_eq!(sibling(&dir.join("d.cbz"), -1), Some(dir.join("c")));
        assert_eq!(sibling(&dir.join("a"), -1), None);
        assert_eq!(sibling(&dir.join("d.cbz"), 1), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn numbers_compare_by_value() {
        assert_eq!(
            sorted(vec!["img10.jpg", "img2.jpg", "img1.jpg"]),
            vec!["img1.jpg", "img2.jpg", "img10.jpg"]
        );
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(
            sorted(vec!["b.png", "A.png", "a2.png"]),
            vec!["A.png", "a2.png", "b.png"]
        );
    }

    #[test]
    fn leading_zeros_are_stable() {
        assert_eq!(
            sorted(vec!["01.jpg", "1.jpg", "001.jpg"]),
            vec!["001.jpg", "01.jpg", "1.jpg"]
        );
    }

    #[test]
    fn korean_names() {
        assert_eq!(
            sorted(vec!["사진 10.jpg", "사진 9.jpg", "가방.jpg"]),
            vec!["가방.jpg", "사진 9.jpg", "사진 10.jpg"]
        );
    }

    #[test]
    fn extension_filter() {
        assert!(is_image(Path::new("a/B.JPG")));
        assert!(is_image(Path::new("x.webp")));
        assert!(!is_image(Path::new("notes.txt")));
        assert!(!is_image(Path::new("noext")));
    }

    #[test]
    fn listing_finds_selected_file() {
        let dir = std::env::temp_dir().join(format!("muud-view-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["p10.png", "p2.png", "p1.png", "readme.txt"] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        let l = list_for(&dir.join("p2.png")).unwrap();
        let names: Vec<String> = l.files.iter().map(|p| file_name(p)).collect();
        assert_eq!(names, vec!["p1.png", "p2.png", "p10.png"]);
        assert_eq!(l.index, 1);
        let l = list_for(&dir).unwrap();
        assert_eq!(l.index, 0);
        std::fs::write(dir.join("p1.png"), vec![b'x'; 100]).unwrap();
        let l = list_sorted(&dir, crate::core::settings::SortBy::Size, true).unwrap();
        assert_eq!(file_name(&l.files[0]), "p1.png");
        let l = list_sorted(&dir, crate::core::settings::SortBy::Name, true).unwrap();
        assert_eq!(file_name(&l.files[0]), "p10.png");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
