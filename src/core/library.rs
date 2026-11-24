//! The open folder: file list, current position, decoded-image cache and
//! prefetching. No UI code here, so it is tested directly.
//!
//! Images are cached at screen size ([`Quality::Screen`]). Only the current
//! image is ever held at full resolution, and only once the view needs it
//! (zoomed in past the screen-size pixels).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::loader::{Decoded, Job, Loader, Quality};

/// How many files ahead (in the direction of travel) are read into memory.
pub const READ_AHEAD: usize = 10;

pub enum Slot {
    Ready(Arc<Decoded>),
    Failed(String),
}

impl Slot {
    fn bytes(&self) -> usize {
        match self {
            Slot::Ready(d) => d.bytes(),
            Slot::Failed(_) => 0,
        }
    }
}

/// What a folder change did to the current image.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Applied {
    Nothing,
    /// Still shown (maybe at a new position or under a new name).
    Kept,
    /// Rewritten by another program; decoded again.
    CurrentReloaded,
    /// Gone; the next image is shown.
    CurrentRemoved,
    /// The folder has no images left.
    Emptied,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Step {
    Moved,
    AtStart,
    AtEnd,
    Empty,
}

pub struct Library {
    pub files: Vec<PathBuf>,
    pub index: usize,
    direction: i32,
    cache: HashMap<PathBuf, Slot>,
    lru: VecDeque<PathBuf>,
    budget: usize,
    /// The current image at full resolution, once asked for.
    full: Option<(PathBuf, Arc<Decoded>)>,
    want_full: Option<PathBuf>,
    /// A navigation key is being held: decode only where we are now, so the
    /// images flown past are skipped instead of queued.
    rapid: bool,
    loader: Loader,
}

impl Library {
    pub fn new(loader: Loader, budget: usize) -> Self {
        Self {
            files: Vec::new(),
            index: 0,
            direction: 1,
            cache: HashMap::new(),
            lru: VecDeque::new(),
            budget,
            full: None,
            want_full: None,
            rapid: false,
            loader,
        }
    }

    pub fn loader(&self) -> &Loader {
        &self.loader
    }

    pub fn set_files(&mut self, files: Vec<PathBuf>, index: usize) {
        self.files = files;
        self.index = index.min(self.files.len().saturating_sub(1));
        self.direction = 1;
        self.drop_full_unless_current();
        self.request();
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.index = 0;
        self.full = None;
        self.want_full = None;
        self.loader.want(Vec::new());
        self.loader.read_ahead(Vec::new());
    }

    /// Forgets every decoded image and decodes again (after a setting that
    /// changes the pixels).
    pub fn reload(&mut self) {
        self.cache.clear();
        self.lru.clear();
        self.full = None;
        self.want_full = None;
        self.request();
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn current(&self) -> Option<&PathBuf> {
        self.files.get(self.index)
    }

    /// Screen-quality slot (or the failure) for `path`.
    pub fn get(&self, path: &Path) -> Option<&Slot> {
        self.cache.get(path)
    }

    /// The best decoded image for `path`: full resolution if loaded.
    pub fn best(&self, path: &Path) -> Option<&Arc<Decoded>> {
        if let Some((p, d)) = &self.full {
            if p == path {
                return Some(d);
            }
        }
        match self.cache.get(path) {
            Some(Slot::Ready(d)) => Some(d),
            _ => None,
        }
    }

    pub fn current_decoded(&self) -> Option<&Arc<Decoded>> {
        self.best(self.current()?)
    }

    pub fn is_rapid(&self) -> bool {
        self.rapid
    }

    /// Turns "key held down" mode on or off.
    pub fn set_rapid(&mut self, rapid: bool) {
        if self.rapid != rapid {
            self.rapid = rapid;
            self.request();
        }
    }

    /// Files worth having decoded, most important first: the current one,
    /// two ahead in the direction of travel, one behind, then one more ahead.
    /// While a key is held only the current one.
    pub fn wanted(&self) -> Vec<PathBuf> {
        if self.files.is_empty() {
            return Vec::new();
        }
        if self.rapid {
            return vec![self.files[self.index].clone()];
        }
        self.around(&[0, 1, 2, -1, 3])
    }

    /// Files whose bytes should be in memory: the current one, the next
    /// [`READ_AHEAD`] in the direction of travel and two behind.
    pub fn read_list(&self) -> Vec<PathBuf> {
        if self.files.is_empty() {
            return Vec::new();
        }
        if self.rapid {
            return vec![self.files[self.index].clone()];
        }
        let mut steps: Vec<i64> = (0..=READ_AHEAD as i64).collect();
        steps.extend([-1, -2]);
        self.around(&steps)
    }

    /// Files at the given offsets from the current one (positive = the
    /// direction of travel), skipping ones past either end.
    fn around(&self, steps: &[i64]) -> Vec<PathBuf> {
        let i = self.index as i64;
        let d = if self.direction < 0 { -1 } else { 1 };
        let mut out = Vec::new();
        for k in steps {
            let j = i + k * d;
            if j >= 0 && (j as usize) < self.files.len() {
                let p = &self.files[j as usize];
                if !out.contains(p) {
                    out.push(p.clone());
                }
            }
        }
        out
    }

    /// Decode jobs for whatever is wanted and not cached yet. A wanted full
    /// decode comes right after the current image.
    pub fn jobs(&self) -> Vec<Job> {
        let mut jobs: Vec<Job> = self
            .wanted()
            .into_iter()
            .filter(|p| !self.cache.contains_key(p))
            .map(Job::screen)
            .collect();
        if let Some(p) = &self.want_full {
            if self.full.as_ref().map(|(f, _)| f) != Some(p) {
                // Right after the current image's own screen decode, if that
                // is still pending; otherwise first.
                let at = usize::from(jobs.first().is_some_and(|j| &j.path == p));
                jobs.insert(at, Job::full(p.clone()));
            }
        }
        jobs
    }

    /// Asks the loader for whatever is wanted and not cached yet.
    pub fn request(&self) {
        self.loader.want(self.jobs());
        self.loader.read_ahead(self.read_list());
    }

    /// Asks for the current image at full resolution, if the cached one was
    /// shrunk for the screen. Returns true if a new decode was started.
    pub fn request_full(&mut self) -> bool {
        let Some(cur) = self.current().cloned() else {
            return false;
        };
        if self.want_full.as_ref() == Some(&cur) {
            return false;
        }
        let reduced = matches!(self.cache.get(&cur), Some(Slot::Ready(d)) if d.reduced);
        if !reduced {
            return false;
        }
        self.want_full = Some(cur);
        self.request();
        true
    }

    /// A full-resolution decode was asked for and has not arrived yet.
    pub fn full_pending(&self) -> bool {
        self.want_full.is_some() && self.full.as_ref().map(|(p, _)| p) != self.want_full.as_ref()
    }

    pub fn has_full(&self, path: &Path) -> bool {
        self.full.as_ref().is_some_and(|(p, _)| p == path)
    }

    fn drop_full_unless_current(&mut self) {
        let cur = self.current().cloned();
        if self.full.as_ref().map(|(p, _)| p) != cur.as_ref() {
            self.full = None;
        }
        if self.want_full != cur {
            self.want_full = None;
        }
    }

    /// Collects finished decodes. Returns true if anything arrived.
    pub fn poll(&mut self) -> bool {
        let results = self.loader.poll();
        let got = !results.is_empty();
        for done in results {
            match (done.quality, done.result) {
                (Quality::Screen, Ok(d)) => self.insert(done.path, Slot::Ready(d)),
                (Quality::Screen, Err(e)) => {
                    super::log::error(&format!("decode {}: {e}", done.path.display()));
                    self.insert(done.path, Slot::Failed(e));
                }
                (Quality::Full, Ok(d)) => {
                    if self.want_full.as_ref() == Some(&done.path) {
                        self.full = Some((done.path, d));
                    }
                }
                (Quality::Full, Err(e)) => {
                    // Keep showing the screen-size image.
                    super::log::error(&format!("full decode {}: {e}", done.path.display()));
                }
            }
        }
        if got {
            self.evict();
        }
        got
    }

    fn insert(&mut self, path: PathBuf, slot: Slot) {
        self.lru.retain(|p| p != &path);
        self.lru.push_back(path.clone());
        self.cache.insert(path, slot);
    }

    /// Drops the least recently decoded images that are not wanted until the
    /// cache fits its memory budget.
    fn evict(&mut self) {
        let wanted: HashSet<PathBuf> = self.wanted().into_iter().collect();
        let mut total: usize = self.cache.values().map(Slot::bytes).sum();
        let mut keep = VecDeque::new();
        while let Some(p) = self.lru.pop_front() {
            if total > self.budget && !wanted.contains(&p) {
                if let Some(s) = self.cache.remove(&p) {
                    total -= s.bytes();
                }
            } else {
                keep.push_back(p);
            }
        }
        self.lru = keep;
    }

    /// What to draw while the current image is still decoding: the current
    /// one if ready, otherwise the ready image closest to it among those
    /// between `shown` and the current one. Holding a key down therefore
    /// shows whatever finished along the way instead of freezing.
    pub fn nearest_ready(&self, shown: Option<&Path>) -> Option<PathBuf> {
        let cur = self.current()?;
        if self.best(cur).is_some() {
            return Some(cur.clone());
        }
        let from = self.files.iter().position(|p| Some(p.as_path()) == shown)?;
        let to = self.index;
        let between: Vec<usize> = if from < to {
            (from + 1..to).rev().collect()
        } else {
            (to + 1..from).collect()
        };
        between
            .into_iter()
            .map(|i| &self.files[i])
            .find(|p| matches!(self.cache.get(*p), Some(Slot::Ready(_))))
            .cloned()
    }

    /// Moves to `i` (clamped). Returns true if the position changed.
    pub fn go_to(&mut self, i: usize) -> bool {
        if self.files.is_empty() {
            return false;
        }
        let i = i.min(self.files.len() - 1);
        if i == self.index {
            return false;
        }
        self.direction = if i > self.index { 1 } else { -1 };
        self.index = i;
        self.drop_full_unless_current();
        self.request();
        self.evict();
        true
    }

    pub fn step(&mut self, delta: i64) -> Step {
        if self.files.is_empty() {
            return Step::Empty;
        }
        let last = self.files.len() as i64 - 1;
        let target = (self.index as i64 + delta).clamp(0, last);
        if self.go_to(target as usize) {
            Step::Moved
        } else if delta > 0 {
            Step::AtEnd
        } else {
            Step::AtStart
        }
    }

    /// Removes the current file from the list (after it was moved or deleted)
    /// and returns its path and former position.
    pub fn remove_current(&mut self) -> Option<(PathBuf, usize)> {
        let at = self.index;
        let path = self.files.get(at)?.clone();
        self.files.remove(at);
        self.cache.remove(&path);
        self.lru.retain(|p| p != &path);
        self.index = at.min(self.files.len().saturating_sub(1));
        self.drop_full_unless_current();
        self.request();
        Some((path, at))
    }

    /// Puts a file back at `at` and shows it (used by undo).
    pub fn restore(&mut self, path: PathBuf, at: usize) {
        let at = at.min(self.files.len());
        self.files.insert(at, path);
        self.index = at;
        self.drop_full_unless_current();
        self.request();
    }

    /// Updates a file's path after it was renamed, keeping its decoded image.
    pub fn replace_path(&mut self, old: &Path, new: PathBuf) {
        if let Some(i) = self.files.iter().position(|p| p == old) {
            self.files[i] = new.clone();
        }
        if let Some(slot) = self.cache.remove(old) {
            self.cache.insert(new.clone(), slot);
        }
        for p in self.lru.iter_mut() {
            if p == old {
                *p = new.clone();
            }
        }
        if let Some((p, _)) = &mut self.full {
            if p == old {
                *p = new.clone();
            }
        }
        if self.want_full.as_deref() == Some(old) {
            self.want_full = Some(new);
        }
    }

    /// Applies changes made to the folder by other programs. The current
    /// image stays current if it still exists (also when renamed); if it was
    /// removed, the image that took its place is shown. Changes the app made
    /// itself (already applied) are ignored. Returns what happened to the
    /// current image.
    pub fn apply_change(
        &mut self,
        change: &super::watch::Change,
        sort: super::settings::SortBy,
        desc: bool,
    ) -> Applied {
        let before = self.current().cloned();
        let mut changed = false;
        for (old, new) in &change.renamed {
            if self.files.contains(old) {
                self.replace_path(old, new.clone());
                changed = true;
            } else if !self.files.contains(new) {
                self.files.push(new.clone());
                changed = true;
            }
        }
        // Where the current image was, counted among files that remain, so
        // a removed current image is replaced by the next one.
        let removed: HashSet<&PathBuf> = change.removed.iter().collect();
        let kept_before = self.files[..self.index.min(self.files.len())]
            .iter()
            .filter(|p| !removed.contains(p))
            .count();
        let n = self.files.len();
        self.files.retain(|p| !removed.contains(p));
        changed |= self.files.len() != n;
        for p in &change.removed {
            self.cache.remove(p);
            self.lru.retain(|q| q != p);
        }
        for p in &change.added {
            if !self.files.contains(p) {
                self.files.push(p.clone());
                changed = true;
            }
        }
        let mut reload_current = false;
        for p in &change.modified {
            if self.cache.remove(p).is_some() {
                self.lru.retain(|q| q != p);
            }
            if self.full.as_ref().is_some_and(|(f, _)| f == p) {
                self.full = None;
                self.want_full = None;
            }
            reload_current |= before.as_ref() == Some(p);
        }
        if !changed && change.modified.is_empty() {
            return Applied::Nothing;
        }
        // The current path after any rename.
        let current = before.as_ref().map(|b| {
            change
                .renamed
                .iter()
                .find(|(o, _)| o == b)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| b.clone())
        });
        if changed {
            super::folder::sort_files(&mut self.files, sort, desc);
        }
        let result = match current.and_then(|c| self.files.iter().position(|p| *p == c)) {
            Some(i) => {
                self.index = i;
                if reload_current {
                    Applied::CurrentReloaded
                } else {
                    Applied::Kept
                }
            }
            None if self.files.is_empty() => {
                self.index = 0;
                Applied::Emptied
            }
            None => {
                self.index = kept_before.min(self.files.len() - 1);
                Applied::CurrentRemoved
            }
        };
        self.drop_full_unless_current();
        self.request();
        result
    }

    /// Adds a decoded image that did not come from the loader (e.g. pasted).
    pub fn insert_decoded(&mut self, path: PathBuf, decoded: Arc<Decoded>) {
        self.insert(path, Slot::Ready(decoded));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn lib_with(n: usize) -> (Library, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "muud-lib-{}-{n}-{}",
            std::process::id(),
            rand_suffix()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut files = Vec::new();
        for i in 0..n {
            let p = dir.join(format!("{i:02}.png"));
            image::RgbaImage::from_pixel(4, 4, image::Rgba([i as u8, 0, 0, 255]))
                .save(&p)
                .unwrap();
            files.push(p);
        }
        let mut lib = Library::new(Loader::new(eframe::egui::Context::default(), 2), 1 << 30);
        lib.set_files(files, 0);
        (lib, dir)
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    fn wait_for(lib: &mut Library, path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while lib.get(path).is_none() && Instant::now() < deadline {
            lib.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(lib.get(path).is_some(), "decoded in time");
    }

    #[test]
    fn stepping_clamps_and_reports_ends() {
        let (mut lib, dir) = lib_with(3);
        assert_eq!(lib.step(-1), Step::AtStart);
        assert_eq!(lib.step(1), Step::Moved);
        assert_eq!(lib.step(10), Step::Moved);
        assert_eq!(lib.index, 2);
        assert_eq!(lib.step(1), Step::AtEnd);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prefetch_follows_direction() {
        let (mut lib, dir) = lib_with(10);
        lib.go_to(5);
        let w: Vec<String> = lib
            .wanted()
            .iter()
            .map(|p| crate::core::folder::file_name(p))
            .collect();
        assert_eq!(w, ["05.png", "06.png", "07.png", "04.png", "08.png"]);
        lib.go_to(4);
        let w: Vec<String> = lib
            .wanted()
            .iter()
            .map(|p| crate::core::folder::file_name(p))
            .collect();
        assert_eq!(w, ["04.png", "03.png", "02.png", "05.png", "01.png"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn decodes_current_and_neighbours() {
        let (mut lib, dir) = lib_with(4);
        let first = lib.files[0].clone();
        let second = lib.files[1].clone();
        wait_for(&mut lib, &first);
        wait_for(&mut lib, &second);
        assert!(lib.current_decoded().is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remove_and_restore() {
        let (mut lib, dir) = lib_with(3);
        lib.go_to(2);
        let (p, at) = lib.remove_current().unwrap();
        assert_eq!(at, 2);
        assert_eq!(lib.len(), 2);
        assert_eq!(lib.index, 1);
        lib.restore(p.clone(), at);
        assert_eq!(lib.current(), Some(&p));
        assert_eq!(lib.len(), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rename_keeps_position() {
        let (mut lib, dir) = lib_with(3);
        lib.go_to(1);
        let old = lib.files[1].clone();
        let new = dir.join("renamed.png");
        lib.replace_path(&old, new.clone());
        assert_eq!(lib.current(), Some(&new));
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn names(paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .map(|p| crate::core::folder::file_name(p))
            .collect()
    }

    #[test]
    fn reads_ten_ahead_in_the_direction_of_travel() {
        let (mut lib, dir) = lib_with(30);
        lib.go_to(5);
        let r = names(&lib.read_list());
        assert_eq!(r.len(), 13);
        assert_eq!(r[0], "05.png");
        assert_eq!(r[10], "15.png");
        assert_eq!(&r[11..], ["04.png", "03.png"]);
        lib.go_to(4);
        let r = names(&lib.read_list());
        assert_eq!(&r[..3], ["04.png", "03.png", "02.png"]);
        // Clipped at the start of the folder.
        assert!(!r.iter().any(|n| n.starts_with('-')));
        assert_eq!(r.len(), 7); // 04..00 and 05, 06
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn holding_a_key_skips_the_images_in_between() {
        let (mut lib, dir) = lib_with(10);
        let first = lib.files[0].clone();
        wait_for(&mut lib, &first);
        lib.set_rapid(true);
        for _ in 0..6 {
            lib.step(1);
            // Only where we are now is decoded or read.
            assert_eq!(lib.wanted(), vec![lib.files[lib.index].clone()]);
            assert_eq!(lib.read_list(), lib.wanted());
        }
        lib.set_rapid(false);
        assert_eq!(lib.wanted().len(), 5);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nearest_ready_shows_progress_while_flying_past() {
        let (mut lib, dir) = lib_with(8);
        let shown = lib.files[0].clone();
        wait_for(&mut lib, &shown);
        let three = lib.files[3].clone();
        lib.go_to(3);
        wait_for(&mut lib, &three);
        lib.go_to(6);
        // 6 may or may not be decoded yet; force "not yet" by asking about
        // an image whose decode we drop.
        lib.cache.remove(&lib.files[6].clone());
        lib.cache.remove(&lib.files[5].clone());
        lib.cache.remove(&lib.files[4].clone());
        assert_eq!(lib.nearest_ready(Some(&shown)), Some(three.clone()));
        // Nothing between: keep what is shown.
        assert_eq!(lib.nearest_ready(Some(&lib.files[5].clone())), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn full_resolution_only_for_the_current_reduced_image() {
        let (mut lib, dir) = lib_with(3);
        let first = lib.files[0].clone();
        wait_for(&mut lib, &first);
        // A 4x4 image is never shrunk, so there is nothing more to load.
        assert!(!lib.request_full());
        // Pretend it was shrunk for the screen.
        let mut d = crate::core::decode::Decoded::from_color_image(
            eframe::egui::ColorImage::new([2, 2], vec![eframe::egui::Color32::RED; 4]),
            "PNG",
        );
        d.reduced = true;
        lib.insert_decoded(first.clone(), Arc::new(d));
        assert!(lib.request_full());
        assert!(!lib.request_full(), "asked once");
        assert_eq!(lib.jobs()[0], Job::full(first.clone()));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !lib.has_full(&first) && Instant::now() < deadline {
            lib.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(lib.has_full(&first));
        assert_eq!(lib.current_decoded().unwrap().texture_size(), [4, 4]);
        // Moving on frees it.
        lib.go_to(1);
        assert!(!lib.has_full(&first));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folder_changes_keep_the_current_image() {
        use crate::core::settings::SortBy;
        use crate::core::watch::Change;
        let (mut lib, dir) = lib_with(5);
        lib.go_to(2);
        let cur = lib.files[2].clone();
        // A new file sorts in before the current one: position moves, image stays.
        let change = Change {
            added: vec![dir.join("00a.png")],
            ..Default::default()
        };
        assert_eq!(
            lib.apply_change(&change, SortBy::Name, false),
            Applied::Kept
        );
        assert_eq!(lib.current(), Some(&cur));
        assert_eq!(lib.index, 3);
        // Applying the same change again does nothing.
        assert_eq!(
            lib.apply_change(&change, SortBy::Name, false),
            Applied::Nothing
        );
        // Renamed elsewhere: follow it.
        let renamed = dir.join("zz.png");
        let change = Change {
            renamed: vec![(cur.clone(), renamed.clone())],
            ..Default::default()
        };
        assert_eq!(
            lib.apply_change(&change, SortBy::Name, false),
            Applied::Kept
        );
        assert_eq!(lib.current(), Some(&renamed));
        assert_eq!(lib.index, lib.len() - 1);
        // Removed: the image that took its place (here the new last one).
        let change = Change {
            removed: vec![renamed.clone()],
            ..Default::default()
        };
        assert_eq!(
            lib.apply_change(&change, SortBy::Name, false),
            Applied::CurrentRemoved
        );
        assert_eq!(lib.index, lib.len() - 1);
        // Removing one before the current keeps the current.
        lib.go_to(2);
        let cur = lib.files[2].clone();
        let change = Change {
            removed: vec![lib.files[0].clone()],
            ..Default::default()
        };
        assert_eq!(
            lib.apply_change(&change, SortBy::Name, false),
            Applied::Kept
        );
        assert_eq!(lib.current(), Some(&cur));
        assert_eq!(lib.index, 1);
        // Rewritten: decoded again.
        let change = Change {
            modified: vec![cur.clone()],
            ..Default::default()
        };
        assert_eq!(
            lib.apply_change(&change, SortBy::Name, false),
            Applied::CurrentReloaded
        );
        // Everything gone.
        let change = Change {
            removed: lib.files.clone(),
            ..Default::default()
        };
        assert_eq!(
            lib.apply_change(&change, SortBy::Name, false),
            Applied::Emptied
        );
        assert!(lib.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn eviction_keeps_wanted_images() {
        let (mut lib, dir) = lib_with(8);
        lib.budget = 0; // evict everything that is not wanted
        for i in 0..8 {
            lib.go_to(i);
            let p = lib.files[i].clone();
            wait_for(&mut lib, &p);
        }
        let wanted: HashSet<PathBuf> = lib.wanted().into_iter().collect();
        assert!(lib.cache.keys().all(|k| wanted.contains(k)));
        assert!(lib.current_decoded().is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
