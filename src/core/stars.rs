//! Star marks for quick culling: `S` toggles, `Shift+S` shows only
//! starred images. Kept in `stars.txt` in the data folder, one full path
//! per line, so they survive restarts and are easy to read or back up.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Stars {
    set: BTreeSet<PathBuf>,
    file: Option<PathBuf>,
}

impl Stars {
    /// Loads from `file` (missing file = no stars).
    pub fn load(file: PathBuf) -> Self {
        let set = std::fs::read_to_string(&file)
            .map(|t| {
                t.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        Self {
            set,
            file: Some(file),
        }
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.set.contains(path)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Stars or unstars `path`; returns whether it is starred now.
    pub fn toggle(&mut self, path: &Path) -> bool {
        let on = if self.set.remove(path) {
            false
        } else {
            self.set.insert(path.to_path_buf());
            true
        };
        self.save();
        on
    }

    /// Keeps the star when a file is renamed.
    pub fn rename(&mut self, old: &Path, new: &Path) {
        if self.set.remove(old) {
            self.set.insert(new.to_path_buf());
            self.save();
        }
    }

    /// The starred files among `files`, in the same order.
    pub fn filter(&self, files: &[PathBuf]) -> Vec<PathBuf> {
        files.iter().filter(|p| self.contains(p)).cloned().collect()
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        let text: String = self
            .set
            .iter()
            .map(|p| format!("{}\n", p.display()))
            .collect();
        if let Err(e) = std::fs::write(file, text) {
            super::log::error(&format!("save stars: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_filter_rename_and_persist() {
        let dir = std::env::temp_dir().join(format!(
            "muud-stars-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("stars.txt");
        let files: Vec<PathBuf> = ["a.jpg", "b.jpg", "c.jpg"].map(|n| dir.join(n)).to_vec();
        let mut s = Stars::load(file.clone());
        assert!(s.toggle(&files[2]));
        assert!(s.toggle(&files[0]));
        assert_eq!(s.filter(&files), vec![files[0].clone(), files[2].clone()]);
        assert!(!s.toggle(&files[0]));
        s.rename(&files[2], &dir.join("c2.jpg"));
        // Survives a restart.
        let s = Stars::load(file);
        assert_eq!(s.len(), 1);
        assert!(s.contains(&dir.join("c2.jpg")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
