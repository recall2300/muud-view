//! Photo folders (사진보관함): one key press copies or moves the current
//! image into one of nine target folders.

use std::io;
use std::path::{Path, PathBuf};

pub const SLOTS: usize = 9;

/// Where folder `slot` (1-based) lives: the configured path, or
/// `Pictures\muud-view\<slot>` by default.
pub fn resolve(configured: &[Option<PathBuf>], slot: u8) -> PathBuf {
    let i = slot.clamp(1, SLOTS as u8) as usize - 1;
    if let Some(Some(p)) = configured.get(i) {
        return p.clone();
    }
    default_root().join(slot.to_string())
}

pub fn default_root() -> PathBuf {
    crate::platform::pictures_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("muud-view")
}

/// A path in `dir` for `name` that does not exist yet:
/// `photo.jpg`, then `photo (2).jpg`, `photo (3).jpg`, …
pub fn unique_target(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|c| !c.exists())
        .expect("some free name")
}

fn file_name(src: &Path) -> io::Result<String> {
    src.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))
}

/// Copies `src` into `dir`, creating it if needed. Returns the new path.
pub fn copy_to(src: &Path, dir: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let dest = unique_target(dir, &file_name(src)?);
    std::fs::copy(src, &dest)?;
    Ok(dest)
}

/// Moves `src` into `dir`, creating it if needed. Works across drives by
/// copying and then deleting. Returns the new path.
pub fn move_to(src: &Path, dir: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let dest = unique_target(dir, &file_name(src)?);
    move_file(src, &dest)?;
    Ok(dest)
}

/// Renames, falling back to copy + delete when the target is on another drive.
pub fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "muud-pf-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn names_never_collide() {
        let d = temp_dir("names");
        assert_eq!(unique_target(&d, "a.jpg"), d.join("a.jpg"));
        std::fs::write(d.join("a.jpg"), b"1").unwrap();
        assert_eq!(unique_target(&d, "a.jpg"), d.join("a (2).jpg"));
        std::fs::write(d.join("a (2).jpg"), b"2").unwrap();
        assert_eq!(unique_target(&d, "a.jpg"), d.join("a (3).jpg"));
        std::fs::write(d.join("noext"), b"x").unwrap();
        assert_eq!(unique_target(&d, "noext"), d.join("noext (2)"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn copy_and_move() {
        let src_dir = temp_dir("src");
        let box_dir = temp_dir("box").join("1");
        let src = src_dir.join("사진.jpg");
        std::fs::write(&src, b"jpeg").unwrap();

        let c1 = copy_to(&src, &box_dir).unwrap();
        let c2 = copy_to(&src, &box_dir).unwrap();
        assert_eq!(c1, box_dir.join("사진.jpg"));
        assert_eq!(c2, box_dir.join("사진 (2).jpg"));
        assert!(src.exists());

        let m = move_to(&src, &box_dir).unwrap();
        assert_eq!(m, box_dir.join("사진 (3).jpg"));
        assert!(!src.exists());
        assert_eq!(std::fs::read(&m).unwrap(), b"jpeg");

        std::fs::remove_dir_all(src_dir).unwrap();
        std::fs::remove_dir_all(box_dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn resolves_defaults_and_overrides() {
        let mut cfg = vec![None; SLOTS];
        assert!(
            resolve(&cfg, 2).ends_with("muud-view/2") || resolve(&cfg, 2).ends_with("muud-view\\2")
        );
        cfg[0] = Some(PathBuf::from("/tmp/mine"));
        assert_eq!(resolve(&cfg, 1), PathBuf::from("/tmp/mine"));
        assert_eq!(resolve(&[], 3), default_root().join("3"));
    }
}
