//! Folder change detection: notices files added, removed, renamed or
//! rewritten in the open folder while you look at it.
//!
//! A background thread lists the folder every couple of seconds and compares
//! file names, sizes and modification times. Polling needs no extra
//! dependency, works the same on local disks and network shares (where
//! change notifications are unreliable), and is cheap: on Windows the
//! directory listing already carries size and time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use eframe::egui;

pub const INTERVAL: Duration = Duration::from_secs(2);

type Stamp = (u64, Option<SystemTime>);

/// Size and modification time of every image in a folder.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Snapshot {
    entries: HashMap<PathBuf, Stamp>,
}

impl Snapshot {
    pub fn scan(dir: &Path) -> std::io::Result<Self> {
        let mut entries = HashMap::new();
        for e in std::fs::read_dir(dir)?.filter_map(Result::ok) {
            let path = e.path();
            if !super::folder::is_image(&path) {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_file() {
                entries.insert(path, (meta.len(), meta.modified().ok()));
            }
        }
        Ok(Self { entries })
    }

    #[cfg(test)]
    fn from(items: &[(&str, u64, u64)]) -> Self {
        Self {
            entries: items
                .iter()
                .map(|(n, size, t)| {
                    (
                        PathBuf::from(n),
                        (
                            *size,
                            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(*t)),
                        ),
                    )
                })
                .collect(),
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct Change {
    pub added: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    /// (old, new): a file that disappeared while one with the same size and
    /// time appeared.
    pub renamed: Vec<(PathBuf, PathBuf)>,
    /// Same name, new contents.
    pub modified: Vec<PathBuf>,
}

impl Change {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.renamed.is_empty()
            && self.modified.is_empty()
    }
}

/// What changed between two listings.
pub fn diff(old: &Snapshot, new: &Snapshot) -> Change {
    let mut c = Change::default();
    let mut removed: Vec<(&PathBuf, &Stamp)> = Vec::new();
    for (p, stamp) in &old.entries {
        match new.entries.get(p) {
            None => removed.push((p, stamp)),
            Some(s) if s != stamp => c.modified.push(p.clone()),
            Some(_) => {}
        }
    }
    let mut added: Vec<(&PathBuf, &Stamp)> = new
        .entries
        .iter()
        .filter(|(p, _)| !old.entries.contains_key(*p))
        .collect();
    // A rename keeps size and time. Only pair up unambiguous matches.
    removed.retain(|(old_path, stamp)| {
        let same: Vec<usize> = (0..added.len()).filter(|&i| added[i].1 == *stamp).collect();
        let others = removed_count_with(&old.entries, new, stamp);
        if same.len() == 1 && others == 1 {
            let (new_path, _) = added.remove(same[0]);
            c.renamed.push(((*old_path).clone(), new_path.clone()));
            false
        } else {
            true
        }
    });
    c.removed = removed.into_iter().map(|(p, _)| p.clone()).collect();
    c.added = added.into_iter().map(|(p, _)| p.clone()).collect();
    c.removed.sort();
    c.added.sort();
    c.modified.sort();
    c.renamed.sort();
    c
}

/// How many vanished files share this stamp (more than one is ambiguous).
fn removed_count_with(old: &HashMap<PathBuf, Stamp>, new: &Snapshot, stamp: &Stamp) -> usize {
    old.iter()
        .filter(|(p, s)| *s == stamp && !new.entries.contains_key(*p))
        .count()
}

/// Watches one folder on a background thread until dropped.
pub struct Watcher {
    pub dir: PathBuf,
    rx: Receiver<Change>,
    stop: Arc<AtomicBool>,
}

impl Watcher {
    pub fn start(dir: PathBuf, ctx: egui::Context, interval: Duration) -> Self {
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let d = dir.clone();
        let _ = std::thread::Builder::new()
            .name("folder-watch".into())
            .spawn(move || {
                let mut last = Snapshot::scan(&d).unwrap_or_default();
                loop {
                    // Sleep in small steps so dropping the watcher is quick.
                    let mut slept = Duration::ZERO;
                    while slept < interval {
                        if flag.load(Ordering::Relaxed) {
                            return;
                        }
                        let step = Duration::from_millis(100).min(interval - slept);
                        std::thread::sleep(step);
                        slept += step;
                    }
                    // A folder that cannot be listed right now (network
                    // hiccup) is skipped, not treated as emptied.
                    let Ok(now) = Snapshot::scan(&d) else {
                        continue;
                    };
                    let change = diff(&last, &now);
                    last = now;
                    if !change.is_empty() {
                        if tx.send(change).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                }
            });
        Self { dir, rx, stop }
    }

    pub fn poll(&self) -> Vec<Change> {
        self.rx.try_iter().collect()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_adds_removes_renames_and_rewrites() {
        let old = Snapshot::from(&[("a.jpg", 10, 1), ("b.jpg", 20, 2), ("c.jpg", 30, 3)]);
        let new = Snapshot::from(&[
            ("a.jpg", 10, 1),
            ("b2.jpg", 20, 2), // renamed
            ("c.jpg", 31, 4),  // rewritten
            ("d.jpg", 40, 5),  // new
        ]);
        let c = diff(&old, &new);
        assert_eq!(c.renamed, vec![("b.jpg".into(), "b2.jpg".into())]);
        assert_eq!(c.modified, vec![PathBuf::from("c.jpg")]);
        assert_eq!(c.added, vec![PathBuf::from("d.jpg")]);
        assert!(c.removed.is_empty());
        assert!(diff(&new, &new).is_empty());
    }

    #[test]
    fn ambiguous_renames_are_add_and_remove() {
        // Two copies with the same size and time vanish, two appear: we
        // cannot tell which became which.
        let old = Snapshot::from(&[("a.jpg", 10, 1), ("b.jpg", 10, 1)]);
        let new = Snapshot::from(&[("x.jpg", 10, 1), ("y.jpg", 10, 1)]);
        let c = diff(&old, &new);
        assert!(c.renamed.is_empty());
        assert_eq!(c.removed.len(), 2);
        assert_eq!(c.added.len(), 2);
    }

    #[test]
    fn watcher_reports_a_new_file() {
        let dir = std::env::temp_dir().join(format!(
            "muud-watch-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.png"), b"x").unwrap();
        let w = Watcher::start(
            dir.clone(),
            egui::Context::default(),
            Duration::from_millis(50),
        );
        std::thread::sleep(Duration::from_millis(120));
        std::fs::write(dir.join("b.png"), b"y").unwrap();
        std::fs::write(dir.join("notes.txt"), b"ignored").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut added = Vec::new();
        while added.is_empty() && std::time::Instant::now() < deadline {
            for c in w.poll() {
                added.extend(c.added);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(added, vec![dir.join("b.png")]);
        drop(w);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
