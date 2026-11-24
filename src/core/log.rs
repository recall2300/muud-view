//! A small log file for "it doesn't open" reports:
//! `%LOCALAPPDATA%\muud-view\log.txt` (or the portable data folder),
//! rotated at 1 MB.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 1024 * 1024;

static FILE: OnceLock<Mutex<PathBuf>> = OnceLock::new();

/// Folder for settings and logs.
pub fn data_dir() -> PathBuf {
    if let Some(p) = crate::platform::integration::portable_data_dir() {
        return p;
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("muud-view")
}

pub fn init(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("log.txt");
    rotate(&path);
    let _ = FILE.set(Mutex::new(path));
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error(&format!("panic: {info}"));
        prev(info);
    }));
}

fn rotate(path: &Path) {
    if std::fs::metadata(path)
        .map(|m| m.len() > MAX_BYTES)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(path, path.with_file_name("log.old.txt"));
    }
}

fn write(level: &str, msg: &str) {
    let Some(lock) = FILE.get() else {
        return;
    };
    let Ok(path) = lock.lock() else {
        return;
    };
    let line = format!(
        "{} [{level}] {msg}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&*path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn error(msg: &str) {
    write("ERROR", msg);
}

pub fn path() -> Option<PathBuf> {
    FILE.get().and_then(|l| l.lock().ok().map(|p| p.clone()))
}

/// The last `n` lines of the log.
pub fn tail(n: usize) -> String {
    let Some(p) = path() else {
        return String::new();
    };
    let text = std::fs::read_to_string(p).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_moves_big_logs_aside() {
        let d = std::env::temp_dir().join(format!("muud-log-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("log.txt");
        std::fs::write(&p, vec![b'x'; (MAX_BYTES + 1) as usize]).unwrap();
        rotate(&p);
        assert!(!p.exists());
        assert!(d.join("log.old.txt").exists());
        std::fs::remove_dir_all(d).unwrap();
    }
}
