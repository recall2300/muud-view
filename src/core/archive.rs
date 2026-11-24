//! Viewing images inside zip / cbz archives without extracting them.
//!
//! An entry is addressed by a *virtual path*: the archive's path followed by
//! the entry's name, e.g. `D:\comics\vol1.cbz\ch1\001.jpg`. Everything that
//! reads image bytes goes through [`read`], which opens the archive when the
//! path points inside one. Korean archives made on Windows usually store
//! names in CP949 without the UTF-8 flag; those are decoded as CP949.

use std::io::Read;
use std::path::{Path, PathBuf};

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

/// The images in an archive as virtual paths, sorted naturally by their
/// full names (so `ch2/10.jpg` comes after `ch2/9.jpg`).
pub fn list(archive: &Path) -> std::io::Result<Vec<PathBuf>> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(std::io::Error::other)?;
    let mut names = Vec::new();
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index_raw(i) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let name = decode_name(entry.name_raw());
        if super::folder::is_image(Path::new(&name)) && !name.starts_with("__MACOSX/") {
            names.push(name);
        }
    }
    names.sort_by(|a, b| super::folder::natural_cmp(a, b));
    Ok(names
        .into_iter()
        .map(|n| {
            n.split('/')
                .fold(archive.to_path_buf(), |p, part| p.join(part))
        })
        .collect())
}

/// Bytes of a file, or of an archive entry for a virtual path.
pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
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
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(std::io::Error::other)?;
    for i in 0..zip.len() {
        let name = match zip.by_index_raw(i) {
            Ok(e) => decode_name(e.name_raw()),
            Err(_) => continue,
        };
        if name == entry {
            let mut f = zip.by_index(i).map_err(std::io::Error::other)?;
            let mut out = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut out)?;
            return Ok(out);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{entry} not in archive"),
    ))
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
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
