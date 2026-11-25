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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use eframe::egui;

/// How often the folder is listed while the window is in use.
pub const INTERVAL: Duration = Duration::from_secs(2);
/// ...while another window is in front (a NAS folder is listed over the
/// network each time, so be gentle).
pub const BACKGROUND_INTERVAL: Duration = Duration::from_secs(15);

/// How the window is being used, which decides how often to look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// In front: every [`INTERVAL`].
    Focused,
    /// Behind other windows: every [`BACKGROUND_INTERVAL`].
    Background,
    /// Minimised: not at all (a scan runs as soon as it comes back).
    Hidden,
}

impl Activity {
    pub fn interval(self, focused: Duration) -> Option<Duration> {
        match self {
            Activity::Focused => Some(focused),
            Activity::Background => Some(BACKGROUND_INTERVAL.max(focused)),
            Activity::Hidden => None,
        }
    }
}

pub type Stamp = (u64, Option<SystemTime>);

/// Size and modification time of every image in a folder.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Snapshot {
    entries: HashMap<PathBuf, Stamp>,
}

impl Snapshot {
    /// Size and time of `path`, if it was in the listing.
    pub fn stamp(&self, path: &Path) -> Option<Stamp> {
        self.entries.get(path).copied()
    }

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
    /// The new listing, so the file list can be sorted by time or size
    /// without asking the disk (or the NAS) about every file again.
    pub listing: Arc<Snapshot>,
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
    let mut c = Change {
        listing: Arc::new(new.clone()),
        ..Change::default()
    };
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
    /// Current interval in ms (`u64::MAX` = paused).
    interval_ms: Arc<AtomicU64>,
    scan_now: Arc<AtomicBool>,
    focused_interval: Duration,
    activity: std::cell::Cell<Activity>,
}

impl Watcher {
    pub fn start(dir: PathBuf, ctx: egui::Context, interval: Duration) -> Self {
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let interval_ms = Arc::new(AtomicU64::new(interval.as_millis() as u64));
        let scan_now = Arc::new(AtomicBool::new(false));
        let (flag, every, now_flag) = (stop.clone(), interval_ms.clone(), scan_now.clone());
        let d = dir.clone();
        let _ = std::thread::Builder::new()
            .name("folder-watch".into())
            .spawn(move || {
                let mut last = Snapshot::scan(&d).unwrap_or_default();
                loop {
                    // Sleep in small steps so dropping the watcher, a new
                    // interval or "look now" take effect quickly.
                    let mut slept = 0u64;
                    loop {
                        if flag.load(Ordering::Relaxed) {
                            return;
                        }
                        if now_flag.swap(false, Ordering::Relaxed) {
                            break;
                        }
                        if slept >= every.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                        slept += 100;
                    }
                    // A folder that cannot be listed right now (network
                    // hiccup) is skipped, not treated as emptied.
                    let Ok(now) = Snapshot::scan(&d) else {
                        continue;
                    };
                    let now = Arc::new(now);
                    let mut change = diff(&last, &now);
                    last = (*now).clone();
                    if !change.is_empty() {
                        change.listing = now;
                        if tx.send(change).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                }
            });
        Self {
            dir,
            rx,
            stop,
            interval_ms,
            scan_now,
            focused_interval: interval,
            activity: std::cell::Cell::new(Activity::Focused),
        }
    }

    /// Looks often while the window is in use, rarely behind other windows
    /// and not at all while minimised; catches up at once when it returns.
    pub fn set_activity(&self, a: Activity) {
        let before = self.activity.replace(a);
        if before == a {
            return;
        }
        let ms = a
            .interval(self.focused_interval)
            .map_or(u64::MAX, |d| d.as_millis() as u64);
        self.interval_ms.store(ms, Ordering::Relaxed);
        if a == Activity::Focused {
            self.scan_now.store(true, Ordering::Relaxed);
        }
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
    fn activity_sets_how_often_to_look() {
        let two = Duration::from_secs(2);
        assert_eq!(Activity::Focused.interval(two), Some(two));
        assert_eq!(
            Activity::Background.interval(two),
            Some(BACKGROUND_INTERVAL)
        );
        assert_eq!(Activity::Hidden.interval(two), None);
        let old = Snapshot::from(&[("a.jpg", 10, 1)]);
        let new = Snapshot::from(&[("a.jpg", 10, 1), ("b.jpg", 20, 2)]);
        let c = diff(&old, &new);
        assert_eq!(c.listing.stamp(Path::new("b.jpg")).map(|s| s.0), Some(20));
    }

    #[test]
    fn hidden_window_stops_looking_until_it_returns() {
        let dir = std::env::temp_dir().join(format!(
            "muud-watch-idle-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let w = Watcher::start(
            dir.clone(),
            egui::Context::default(),
            Duration::from_millis(100),
        );
        w.set_activity(Activity::Hidden);
        std::thread::sleep(Duration::from_millis(250));
        std::fs::write(dir.join("new.png"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert!(w.poll().is_empty(), "no scans while hidden");
        w.set_activity(Activity::Focused);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut added = Vec::new();
        while added.is_empty() && std::time::Instant::now() < deadline {
            for c in w.poll() {
                added.extend(c.added);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(added, vec![dir.join("new.png")]);
        drop(w);
        std::fs::remove_dir_all(&dir).unwrap();
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
