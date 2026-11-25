//! Viewing images inside zip / cbz archives without extracting them.
//!
//! An entry is addressed by a *virtual path*: the archive's path followed by
//! the entry's name, e.g. `D:\comics\vol1.cbz\ch1\001.jpg`. Everything that
//! reads image bytes goes through [`read`], which opens the archive when the
//! path points inside one. Korean archives made on Windows usually store
//! names in CP949 without the UTF-8 flag; those are decoded as CP949.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// Archive types opened like folders.
pub const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "cbz"];

pub fn is_archive(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ARCHIVE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Entry name from its raw bytes: UTF-8 if valid, otherwise CP949.
pub fn decode_name(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_owned(),
        Err(_) => encoding_rs::EUC_KR.decode(raw).0.into_owned(),
    }
}

/// Splits a virtual path into (archive file, entry name with `/`).
pub fn split(path: &Path) -> Option<(PathBuf, String)> {
    let mut archive = path.parent()?;
    loop {
        if is_archive(archive) && archive.is_file() {
            let rest = path.strip_prefix(archive).ok()?;
            let entry = rest
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            return Some((archive.to_path_buf(), entry));
        }
        archive = archive.parent()?;
    }
}

/// True if `path` is inside an archive (not a real file).
pub fn is_virtual(path: &Path) -> bool {
    !path.exists() && split(path).is_some()
}

/// An open archive with its entry names decoded once.
struct Open {
    stamp: (u64, Option<SystemTime>),
    zip: zip::ZipArchive<std::fs::File>,
    names: HashMap<String, usize>,
}

/// Recently used archives, kept open: reading an entry then costs one
/// check of the file's size and time instead of reopening it and parsing
/// its whole directory (several extra round trips on a NAS).
type Shared = Arc<Mutex<Open>>;

fn cache() -> &'static Mutex<Vec<(PathBuf, Shared)>> {
    static CACHE: OnceLock<Mutex<Vec<(PathBuf, Shared)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

const KEEP_OPEN: usize = 4;

/// Closes every cached archive, so Windows lets their folders be moved or
/// deleted (called when another folder or archive is opened).
pub fn close_all() {
    cache().lock().unwrap().clear();
}

fn open(archive: &Path) -> std::io::Result<Shared> {
    let meta = std::fs::metadata(archive)?;
    let stamp = (meta.len(), meta.modified().ok());
    let mut c = cache().lock().unwrap();
    if let Some(i) = c.iter().position(|(p, _)| p == archive) {
        let (p, entry) = c.remove(i);
        if entry.lock().unwrap().stamp == stamp {
            c.insert(0, (p, entry.clone()));
            return Ok(entry);
        }
        // Changed on disk: open it again below.
    }
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(std::io::Error::other)?;
    let mut names = HashMap::new();
    for i in 0..zip.len() {
        if let Ok(e) = zip.by_index_raw(i) {
            if !e.is_dir() {
                names.insert(decode_name(e.name_raw()), i);
            }
        }
    }
    let entry = Arc::new(Mutex::new(Open { stamp, zip, names }));
    c.insert(0, (archive.to_path_buf(), entry.clone()));
    c.truncate(KEEP_OPEN);
    Ok(entry)
}

/// The images in an archive as virtual paths, sorted naturally by their
/// full names (so `ch2/10.jpg` comes after `ch2/9.jpg`).
pub fn list(archive: &Path) -> std::io::Result<Vec<PathBuf>> {
    let open = open(archive)?;
    let mut names: Vec<String> = open
        .lock()
        .unwrap()
        .names
        .keys()
        .filter(|n| super::folder::is_image(Path::new(n)) && !n.starts_with("__MACOSX/"))
        .cloned()
        .collect();
    names.sort_by(|a, b| super::folder::natural_cmp(a, b));
    Ok(names
        .into_iter()
        .map(|n| {
            n.split('/')
                .fold(archive.to_path_buf(), |p, part| p.join(part))
        })
        .collect())
}

/// Bytes of a file, or of an archive entry for a virtual path. RAW files
/// are read only up to their preview image.
pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    if let Some(b) = super::formats::read_raw_preview(path) {
        return Ok(b);
    }
    match std::fs::read(path) {
        Ok(b) => Ok(b),
        Err(e) => {
            let Some((archive, entry)) = split(path) else {
                return Err(e);
            };
            read_entry(&archive, &entry)
        }
    }
}

fn read_entry(archive: &Path, entry: &str) -> std::io::Result<Vec<u8>> {
    let open = open(archive)?;
    let mut open = open.lock().unwrap();
    let Some(&i) = open.names.get(entry) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{entry} not in archive"),
        ));
    };
    let mut f = open.zip.by_index(i).map_err(std::io::Error::other)?;
    let mut out = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A zip with stored (uncompressed) entries and raw, unflagged names —
    /// what old Korean Windows tools write.
    pub fn raw_zip(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
        let crc = |data: &[u8]| {
            let mut c = !0u32;
            for &b in data {
                c ^= b as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        (c >> 1) ^ 0xEDB8_8320
                    } else {
                        c >> 1
                    };
                }
            }
            !c
        };
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            let (c, n, d) = (crc(data), name.len() as u16, data.len() as u32);
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&c.to_le_bytes());
            out.extend_from_slice(&d.to_le_bytes());
            out.extend_from_slice(&d.to_le_bytes());
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name);
            out.extend_from_slice(data);
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend_from_slice(&c.to_le_bytes());
            central.extend_from_slice(&d.to_le_bytes());
            central.extend_from_slice(&d.to_le_bytes());
            central.extend_from_slice(&n.to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name);
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    pub fn png(rgba: [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba(rgba));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    pub fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "muud-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_and_reads_entries_with_korean_names() {
        let dir = temp_dir("zip");
        // "만화" in CP949, then a UTF-8 name, a non-image and a mac junk file.
        let cp949: &[u8] = &[0xB8, 0xB8, 0xC8, 0xAD];
        let mut ko = b"ch1/".to_vec();
        ko.extend_from_slice(cp949);
        ko.extend_from_slice(b"10.png");
        let zip = raw_zip(&[
            (&ko, &png([1, 0, 0, 255])),
            ("ch1/만화9.png".as_bytes(), &png([2, 0, 0, 255])),
            (b"readme.txt", b"hi"),
            (b"__MACOSX/ch1/._x.png", b"junk"),
        ]);
        let archive = dir.join("vol1.cbz");
        std::fs::write(&archive, zip).unwrap();
        assert!(is_archive(&archive));
        let files = list(&archive).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&archive)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(names, ["ch1/만화9.png", "ch1/만화10.png"]);
        assert!(is_virtual(&files[1]));
        assert_eq!(
            split(&files[1]).unwrap(),
            (archive.clone(), "ch1/만화10.png".into())
        );
        let bytes = read(&files[1]).unwrap();
        let img = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(0, 0).0, [1, 0, 0, 255]);
        // Real files still read directly.
        assert_eq!(
            read(&archive).unwrap().len(),
            std::fs::metadata(&archive).unwrap().len() as usize
        );
        assert!(read(&archive.join("missing.png")).is_err());
        // Rewritten on disk: the cached directory is not used any more.
        let zip = raw_zip(&[(b"other.png", &png([9, 9, 9, 255]))]);
        std::fs::write(&archive, zip).unwrap();
        let files = list(&archive).unwrap();
        assert_eq!(files.len(), 1);
        let img = image::load_from_memory(&read(&files[0]).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(img.get_pixel(0, 0).0, [9, 9, 9, 255]);
        close_all();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
