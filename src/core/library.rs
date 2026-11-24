//! The open folder: file list, current position, decoded-image cache and
//! prefetching. No UI code here, so it is tested directly.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::loader::{Decoded, Loader};

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
        self.request();
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.index = 0;
        self.loader.want(Vec::new());
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

    pub fn get(&self, path: &Path) -> Option<&Slot> {
        self.cache.get(path)
    }

    pub fn current_decoded(&self) -> Option<&Arc<Decoded>> {
        match self.cache.get(self.current()?) {
            Some(Slot::Ready(d)) => Some(d),
            _ => None,
        }
    }

    /// Files worth having decoded, most important first: the current one,
    /// two ahead in the direction of travel, one behind, then one more ahead.
    pub fn wanted(&self) -> Vec<PathBuf> {
        if self.files.is_empty() {
            return Vec::new();
        }
        let i = self.index as i64;
        let d = if self.direction < 0 { -1 } else { 1 };
        let mut out = Vec::new();
        for k in [0, d, 2 * d, -d, 3 * d] {
            let j = i + k;
            if j >= 0 && (j as usize) < self.files.len() {
                let p = &self.files[j as usize];
                if !out.contains(p) {
                    out.push(p.clone());
                }
            }
        }
        out
    }

    /// Asks the loader for whatever is wanted and not cached yet.
    pub fn request(&self) {
        let todo: Vec<PathBuf> = self
            .wanted()
            .into_iter()
            .filter(|p| !self.cache.contains_key(p))
            .collect();
        self.loader.want(todo);
    }

    /// Collects finished decodes. Returns true if anything arrived.
    pub fn poll(&mut self) -> bool {
        let results = self.loader.poll();
        let got = !results.is_empty();
        for (path, result) in results {
            let slot = match result {
                Ok(d) => Slot::Ready(d),
                Err(e) => Slot::Failed(e),
            };
            self.insert(path, slot);
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
        self.request();
        Some((path, at))
    }

    /// Puts a file back at `at` and shows it (used by undo).
    #[allow(dead_code)] // wired up in M3
    pub fn restore(&mut self, path: PathBuf, at: usize) {
        let at = at.min(self.files.len());
        self.files.insert(at, path);
        self.index = at;
        self.request();
    }

    /// Adds a decoded image that did not come from the loader (e.g. pasted).
    #[allow(dead_code)] // wired up in M4
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
