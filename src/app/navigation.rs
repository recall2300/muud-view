//! Opening files and folders, moving between pictures, comparing two.

use std::path::{Path, PathBuf};

use eframe::egui::{self};

use crate::core::folder;
use crate::core::view::{View, ZoomMode};

use super::*;

impl MuudApp {
    pub(crate) fn open(&mut self, path: &Path) {
        // Let go of archives from before (Windows keeps their folders busy).
        crate::core::archive::close_all();
        if crate::core::archive::is_archive(path) && path.is_file() {
            self.open_archive(path);
            return;
        }
        match folder::list_sorted(path, self.settings.sort, self.settings.sort_desc) {
            Ok(l) if !l.files.is_empty() => {
                let folder_path = l.files[l.index].parent().map(Path::to_path_buf);
                let folder_key = folder_path
                    .as_ref()
                    .map(|f| f.display().to_string())
                    .unwrap_or_default();
                let mut index = l.index;
                // Opening a folder (not a specific file) resumes where we were.
                if path.is_dir() && self.settings.resume {
                    if let Some(name) = self.settings.position_in(&folder_key) {
                        if let Some(i) = l.files.iter().position(|f| folder::file_name(f) == name) {
                            index = i;
                        }
                    }
                }
                self.settings.add_recent(&folder_key);
                if let Some(dir) = &folder_path {
                    if self.watcher.as_ref().map(|w| &w.dir) != Some(dir) {
                        self.watcher = Some(crate::core::watch::Watcher::start(
                            dir.clone(),
                            self.ctx.clone(),
                            crate::core::watch::INTERVAL,
                        ));
                    }
                }
                self.last_dir = folder_path;
                self.open_error = None;
                self.lib.set_files(l.files, index);
                self.reset_view();
            }
            Ok(_) => self.notify("NO IMAGES IN THIS FOLDER"),
            Err(e) => {
                self.open_error = Some(e.to_string());
                self.notify("CANNOT OPEN");
            }
        }
    }

    /// Opens a zip/cbz like a folder: its images, by name.
    pub(super) fn open_archive(&mut self, archive: &Path) {
        match crate::core::archive::list(archive) {
            Ok(files) if !files.is_empty() => {
                let key = archive.display().to_string();
                let mut index = 0;
                if self.settings.resume {
                    if let Some(name) = self.settings.position_in(&key) {
                        if let Some(i) = files.iter().position(|f| folder::file_name(f) == name) {
                            index = i;
                        }
                    }
                }
                self.settings.add_recent(&key);
                self.watcher = None;
                self.all_files = None;
                self.end_compare();
                self.last_dir = archive.parent().map(Path::to_path_buf);
                self.open_error = None;
                self.lib.set_files(files, index);
                self.reset_view();
                self.grid_wanted = (0, 0);
            }
            Ok(_) => self.notify("NO IMAGES IN THIS FOLDER"),
            Err(e) => {
                self.open_error = Some(e.to_string());
                self.notify("CANNOT OPEN");
            }
        }
    }

    /// The folder or archive the current image lives in.
    pub(crate) fn container(&self) -> Option<PathBuf> {
        let cur = self.lib.current()?;
        match crate::core::archive::split(cur) {
            Some((archive, _)) if !cur.exists() => Some(archive),
            _ => cur.parent().map(Path::to_path_buf),
        }
    }

    pub(crate) fn close_file(&mut self) {
        crate::core::archive::close_all();
        self.watcher = None;
        self.all_files = None;
        self.grid = false;
        self.end_compare();
        self.lib.clear();
        self.shown = None;
        self.textures.clear();
        self.slideshow = None;
    }

    pub(crate) fn reset_view(&mut self) {
        self.frame_index = 0;
        self.next_frame_at = None;
        self.wheel_accum = 0.0;
        self.edge_accum = 0.0;
        if self.zoom_locked || self.compare.is_some() {
            // Ctrl+K, or comparing: keep the same zoom and position for the
            // next image.
            return;
        }
        let mode = match self.view.mode {
            ZoomMode::Free(_) => ZoomMode::Fit,
            m => m,
        };
        self.view = View {
            mode,
            align_top: mode == ZoomMode::FitWidth,
            ..View::default()
        };
        self.frame_index = 0;
        self.next_frame_at = None;
        self.wheel_accum = 0.0;
    }

    pub(crate) fn go_to(&mut self, i: usize) {
        if self.lib.go_to(i) {
            self.reset_view();
        }
    }

    pub(crate) fn step(&mut self, delta: i64) {
        use crate::core::library::Step;
        use crate::core::reading::ReadingMode;
        match self.reading {
            ReadingMode::Webtoon => {
                // Navigation keys scroll the strip by most of a screen.
                let page = self.last_canvas.height() * 0.85;
                self.webtoon_scroll(page * delta.signum() as f32 * (delta.abs().min(3) as f32));
                return;
            }
            m if m.is_two_page() => {
                // Two pages at a time.
                let delta = delta * 2;
                if let Step::Moved = self.lib.step(delta) {
                    self.reset_view();
                }
                return;
            }
            _ => {}
        }
        match self.lib.step(delta) {
            Step::Moved => self.reset_view(),
            Step::AtEnd => self.notify("LAST IMAGE"),
            Step::AtStart => self.notify("FIRST IMAGE"),
            Step::Empty => {}
        }
    }

    /// Forgets the current file after it was moved or deleted.
    pub(crate) fn forget_current(&mut self) -> Option<(PathBuf, usize)> {
        let removed = self.lib.remove_current()?;
        self.textures.remove(&removed.0);
        if self.shown.as_ref() == Some(&removed.0) {
            self.shown = None;
        }
        self.reset_view();
        Some(removed)
    }

    /// A pseudo-random index other than the current one.
    pub(crate) fn random_index(&mut self) -> usize {
        let n = self.lib.len().max(1);
        // xorshift64
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        let mut i = (self.rng % n as u64) as usize;
        if n > 1 && i == self.lib.index {
            i = (i + 1) % n;
        }
        i
    }

    /// Shows only starred images, or the whole folder again.
    pub(crate) fn toggle_starred_only(&mut self) {
        let current = self.lib.current().cloned();
        if let Some(all) = self.all_files.take() {
            let i = current
                .and_then(|c| all.iter().position(|p| *p == c))
                .unwrap_or(0);
            self.lib.set_files(all, i);
            self.notify("ALL IMAGES");
        } else {
            let starred = self.stars.filter(&self.lib.files);
            if starred.is_empty() {
                self.notify("NO STARRED IMAGES");
                return;
            }
            let i = current
                .and_then(|c| starred.iter().position(|p| *p == c))
                .unwrap_or(0);
            self.all_files = Some(self.lib.files.clone());
            let n = starred.len();
            self.lib.set_files(starred, i);
            self.notify(format!("STARRED ONLY · {n}"));
        }
        self.reset_view();
        self.grid_scroll_to = true;
        self.grid_wanted = (0, 0);
    }

    /// Starts comparing: the current image becomes A, the next one B.
    pub(crate) fn start_compare(&mut self) {
        if self.lib.len() < 2 {
            self.notify("NEED TWO IMAGES");
            return;
        }
        let Some(a) = self.lib.current().cloned() else {
            return;
        };
        let mode = crate::core::compare::CompareMode::SideBySide;
        self.compare = Some(Compare {
            a: a.clone(),
            mode,
            split: 0.5,
            dragging_split: false,
            diff: None,
        });
        self.lib.set_pinned(Some(a));
        let delta = if self.lib.index + 1 < self.lib.len() {
            1
        } else {
            -1
        };
        self.lib.step(delta);
        self.view.offset = egui::Vec2::ZERO;
        self.view.mode = ZoomMode::Fit;
        self.notify(format!("COMPARE · {}", mode.label()));
    }

    pub(crate) fn end_compare(&mut self) {
        if self.compare.take().is_some() {
            self.lib.set_pinned(None);
        }
    }

    /// Ends comparing if image A is gone (moved, deleted, renamed away).
    pub(super) fn check_compare(&mut self) {
        let gone = self
            .compare
            .as_ref()
            .is_some_and(|c| !self.lib.files.contains(&c.a));
        if gone {
            self.end_compare();
            self.notify("COMPARE OFF");
        }
    }
}
