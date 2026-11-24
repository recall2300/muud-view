//! Turning key presses and timers into actions.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Vec2, ViewportCommand};

use crate::app::{MuudApp, RAPID_GRACE};
use crate::core::decode::{shrink, upright};
use crate::core::folder;
use crate::core::history::{Action, Undone};
use crate::core::keymap::Cmd;
use crate::core::photo_folders;
use crate::core::settings::Background;
use crate::core::view::{step_zoom, ZoomMode};
use crate::platform;

impl MuudApp {
    pub(crate) fn run(&mut self, ctx: &egui::Context, cmd: Cmd) {
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        match cmd {
            Cmd::OpenFile => {
                if platform::dialogs_available() {
                    if let Some(p) = platform::pick_file(self.last_dir.as_deref()) {
                        self.open(&p);
                    }
                } else {
                    self.notify("DRAG A FILE ONTO THE WINDOW");
                }
            }
            Cmd::OpenFolder => {
                if let Some(p) = platform::pick_folder(self.last_dir.as_deref()) {
                    self.open(&p);
                }
            }
            Cmd::CloseFile => self.close_file(),
            Cmd::Exit => ctx.send_viewport_cmd(ViewportCommand::Close),
            Cmd::Escape if self.compare.is_some() => {
                self.end_compare();
                self.notify("COMPARE OFF");
            }
            Cmd::Escape => {
                if fullscreen {
                    ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
                } else {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
            Cmd::Next => self.step(1),
            Cmd::Prev => self.step(-1),
            Cmd::First => self.go_to(0),
            Cmd::Last => self.go_to(self.lib.len().saturating_sub(1)),
            Cmd::Forward10 => self.step(10),
            Cmd::Back10 => self.step(-10),
            Cmd::FitWindow => {
                self.view.mode = ZoomMode::Fit;
                self.view.offset = Vec2::ZERO;
            }
            Cmd::FitWidth => {
                self.view.mode = ZoomMode::FitWidth;
                self.view.align_top = true;
            }
            Cmd::Original => {
                self.view.mode = ZoomMode::Original;
                self.view.offset = Vec2::ZERO;
            }
            Cmd::ZoomIn | Cmd::ZoomOut => {
                let new = step_zoom(self.last_scale, cmd == Cmd::ZoomIn);
                self.view.offset *= new / self.last_scale;
                self.view.mode = ZoomMode::Free(new);
            }
            Cmd::StretchSmall => {
                self.settings.stretch_small = !self.settings.stretch_small;
                let s = if self.settings.stretch_small {
                    "ON"
                } else {
                    "OFF"
                };
                self.notify(format!("STRETCH SMALL IMAGES {s}"));
            }
            Cmd::Fullscreen => ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!fullscreen)),
            Cmd::Maximize => {
                let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!max));
            }
            Cmd::AlwaysOnTop => {
                self.on_top = !self.on_top;
                ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if self.on_top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                }));
                self.notify(if self.on_top {
                    "ALWAYS ON TOP"
                } else {
                    "NORMAL WINDOW"
                });
            }
            Cmd::SmoothOn | Cmd::SmoothOff => {
                self.settings.smooth = cmd == Cmd::SmoothOn;
                self.textures.clear();
                self.notify(if self.settings.smooth {
                    "SMOOTH SCALING"
                } else {
                    "PIXEL SCALING"
                });
            }
            Cmd::CycleBackground => {
                self.settings.background = match self.settings.background {
                    Background::Ink => Background::Paper,
                    Background::Paper => Background::Gray,
                    Background::Gray => Background::Ink,
                };
            }
            Cmd::RotateCw => self.view.rotate(1),
            Cmd::RotateCcw => self.view.rotate(-1),
            Cmd::Rotate180 => self.view.rotate(2),
            Cmd::ResetRotation => {
                self.view.turns = 0;
                self.view.flip_h = false;
                self.view.flip_v = false;
            }
            Cmd::FlipH => self.view.flip_h = !self.view.flip_h,
            Cmd::FlipV => self.view.flip_v = !self.view.flip_v,
            Cmd::ToggleInfo => self.settings.show_info = !self.settings.show_info,
            Cmd::Compare => {
                if self.compare.is_some() {
                    self.end_compare();
                    self.notify("COMPARE OFF");
                } else {
                    self.start_compare();
                }
            }
            Cmd::CompareMode => {
                if let Some(c) = &mut self.compare {
                    c.mode = c.mode.next();
                    let label = c.mode.label();
                    self.notify(format!("COMPARE · {label}"));
                } else {
                    self.start_compare();
                }
            }
            Cmd::ShareCopy => {
                let Some(path) = self.lib.current().cloned().filter(|p| p.exists()) else {
                    return;
                };
                let max = self.settings.share_size;
                self.spawn_job(ctx, move || match crate::core::share::export(&path, max) {
                    Ok(out) => {
                        // Ready to paste into a mail or messenger.
                        let _ = crate::platform::clipboard::write(None, std::slice::from_ref(&out));
                        format!("SAVED FOR SHARING · {}", folder::file_name(&out))
                    }
                    Err(e) => format!("SHARE FAILED · {e}"),
                });
            }
            Cmd::CopyVisible => {
                let (Some(path), Some(dest)) = (self.lib.current().cloned(), self.last_dest) else {
                    return;
                };
                if !path.exists() {
                    return;
                }
                let part = crate::core::view::visible_part(dest, self.last_canvas);
                let (turns, fh, fv) = (self.view.turns, self.view.flip_h, self.view.flip_v);
                self.spawn_job(ctx, move || {
                    let opts = crate::core::decode::DecodeOpts::full(1 << 15);
                    let d = match crate::core::loader::decode_file(&path, opts) {
                        Ok(d) => d,
                        Err(e) => return format!("COPY FAILED · {e}"),
                    };
                    let stored = &d.frames[0].image;
                    let uvs = crate::core::view::compose_uvs(d.orientation, turns, fh, fv);
                    // Output pixels: the visible share of the drawn image at
                    // its decoded resolution.
                    let [sw, sh] = stored.size;
                    let swap = d.orientation.swaps_axes() ^ (turns % 2 == 1);
                    let (dw, dh) = if swap { (sh, sw) } else { (sw, sh) };
                    let size = [
                        (part.width() * dw as f32).round() as usize,
                        (part.height() * dh as f32).round() as usize,
                    ];
                    let img = crate::core::pixel::crop_drawn(stored, uvs, part, size);
                    match crate::platform::clipboard::write(Some(&img), &[]) {
                        Ok(()) => format!("COPIED VISIBLE AREA · {} × {}", size[0], size[1]),
                        Err(e) => format!("COPY FAILED · {e}"),
                    }
                });
            }
            Cmd::Star => {
                if let Some(p) = self.lib.current().cloned().filter(|p| p.exists()) {
                    let on = self.stars.toggle(&p);
                    self.notify(if on { "STARRED" } else { "UNSTARRED" });
                }
            }
            Cmd::StarredOnly => self.toggle_starred_only(),
            Cmd::Grid => {
                self.grid = !self.grid && !self.lib.is_empty();
                self.grid_scroll_to = true;
                self.grid_wanted = (0, 0);
            }
            Cmd::PixelInfo => {
                self.pixel_info = !self.pixel_info;
                self.notify(if self.pixel_info {
                    "PIXEL INFO ON"
                } else {
                    "PIXEL INFO OFF"
                });
            }
            Cmd::Minimap => {
                self.settings.minimap = !self.settings.minimap;
                self.notify(if self.settings.minimap {
                    "MINIMAP ON"
                } else {
                    "MINIMAP OFF"
                });
            }
            Cmd::CopyImage | Cmd::CopyResized => {
                let (Some(d), Some(path)) = (
                    self.lib.current_decoded().cloned(),
                    self.lib.current().cloned(),
                ) else {
                    return;
                };
                let frame = d.frames[self.frame_index.min(d.frames.len() - 1)]
                    .image
                    .clone();
                let small = cmd == Cmd::CopyResized;
                self.notify(if small {
                    "COPYING SMALL …"
                } else {
                    "COPYING …"
                });
                // Copy the original pixels, not the screen-size texture (or a
                // tiled image's preview), unless the texture already has
                // every pixel the copy needs.
                let image_long = d.width.max(d.height) as usize;
                let want_long = if small {
                    image_long.min(1600)
                } else {
                    image_long
                };
                let redecode = frame.size[0].max(frame.size[1]) < want_long && !d.is_animated();
                self.spawn_job(ctx, move || {
                    let full = if redecode && path.exists() {
                        crate::core::loader::decode_file(
                            &path,
                            crate::core::decode::DecodeOpts::full(1 << 15),
                        )
                        .ok()
                        .map(|f| upright(&f.frames[0].image, f.orientation))
                    } else {
                        None
                    };
                    let mut img = full.unwrap_or_else(|| upright(&frame, d.orientation));
                    if small {
                        img = shrink(&img, 1600);
                    }
                    let files: Vec<PathBuf> = if !small && path.exists() {
                        vec![path]
                    } else {
                        Vec::new()
                    };
                    match platform::clipboard::write(Some(&img), &files) {
                        Ok(()) if small => format!("COPIED {} × {}", img.size[0], img.size[1]),
                        Ok(()) if !files.is_empty() => {
                            format!("COPIED {} × {} + FILE", img.size[0], img.size[1])
                        }
                        Ok(()) => format!("COPIED {} × {}", img.size[0], img.size[1]),
                        Err(e) => format!("COPY FAILED: {e}"),
                    }
                });
            }
            Cmd::Paste => match platform::clipboard::read() {
                platform::clipboard::Pasted::Files(files) => {
                    match files.iter().find(|f| f.is_dir() || folder::is_image(f)) {
                        Some(f) => {
                            let f = f.clone();
                            self.open(&f);
                        }
                        None => self.notify("NO IMAGE ON THE CLIPBOARD"),
                    }
                }
                platform::clipboard::Pasted::Image(img) => {
                    let [w, h] = img.size;
                    self.show_pasted(img);
                    self.notify(format!("PASTED {w} × {h}"));
                }
                platform::clipboard::Pasted::Nothing => self.notify("NO IMAGE ON THE CLIPBOARD"),
            },
            Cmd::CopyPath => {
                if let Some(p) = self.lib.current().cloned() {
                    ctx.copy_text(p.display().to_string());
                    self.notify("PATH COPIED");
                }
            }
            Cmd::CopyExif => match self.lib.current_decoded().and_then(|d| d.exif.clone()) {
                Some(e) => {
                    ctx.copy_text(e.summary());
                    self.notify("EXIF COPIED");
                }
                None => self.notify("NO EXIF"),
            },
            Cmd::ShowInExplorer => {
                if let Some(p) = self.lib.current() {
                    if platform::show_in_explorer(p).is_err() {
                        self.notify("CANNOT OPEN EXPLORER");
                    }
                }
            }
            Cmd::Trash => {
                if let Some(p) = self.lib.current().cloned() {
                    match platform::move_to_trash(&p) {
                        Ok(()) => {
                            if let Some((path, index)) = self.forget_current() {
                                self.history.push(Action::Trashed { path, index });
                            }
                            self.notify("MOVED TO RECYCLE BIN · CTRL+Z UNDO");
                        }
                        Err(e) => self.notify(format!("DELETE FAILED: {e}")),
                    }
                }
            }
            Cmd::CopyToFolder(n) => {
                if let Some(p) = self.lib.current().cloned() {
                    let dir = photo_folders::resolve(&self.settings.photo_folders, n);
                    match photo_folders::copy_to(&p, &dir) {
                        Ok(dest) => {
                            self.history.push(Action::Copied { dest });
                            self.notify(format!("COPIED → FOLDER {n} · {}", dir.display()));
                        }
                        Err(e) => self.notify(format!("COPY FAILED: {e}")),
                    }
                }
            }
            Cmd::MoveToFolder(n) => {
                if let Some(p) = self.lib.current().cloned() {
                    let dir = photo_folders::resolve(&self.settings.photo_folders, n);
                    match photo_folders::move_to(&p, &dir) {
                        Ok(to) => {
                            if let Some((from, index)) = self.forget_current() {
                                self.history.push(Action::Moved { from, to, index });
                            }
                            self.notify(format!("MOVED → FOLDER {n} · CTRL+Z UNDO"));
                        }
                        Err(e) => self.notify(format!("MOVE FAILED: {e}")),
                    }
                }
            }
            Cmd::Undo => match self.history.undo() {
                None => self.notify("NOTHING TO UNDO"),
                Some(Ok(Undone::RemovedCopy)) => self.notify("UNDO · COPY REMOVED"),
                Some(Ok(Undone::RenamedBack { current, restored })) => {
                    self.lib.replace_path(&current, restored);
                    self.notify("UNDO · FILE RESTORED");
                }
                Some(Ok(Undone::Returned { path, index })) => {
                    let same_folder = self
                        .lib
                        .files
                        .first()
                        .and_then(|f| f.parent())
                        .zip(path.parent())
                        .is_some_and(|(a, b)| a == b);
                    if same_folder {
                        self.lib.restore(path, index);
                        self.reset_view();
                    } else {
                        self.open(&path);
                    }
                    self.notify("UNDO · FILE RESTORED");
                }
                Some(Err(e)) => self.notify(format!("UNDO FAILED: {e}")),
            },
            Cmd::DeletePermanently => {
                if self.lib.current().is_some() {
                    self.confirm_delete = true;
                }
            }
            Cmd::Slideshow(n) => {
                let d = Duration::from_secs(n as u64);
                self.slideshow = Some((d, Instant::now() + d));
                self.notify(format!("SLIDESHOW {n}S"));
            }
            Cmd::StopSlideshow => {
                if self.slideshow.take().is_some() {
                    self.notify("SLIDESHOW STOPPED");
                }
            }
            Cmd::Help => self.show_help = !self.show_help,
            Cmd::Setup => {
                self.show_settings = !self.show_settings;
                self.capture = None;
            }
            Cmd::Folders => self.show_folders = !self.show_folders,
            Cmd::RandomNext | Cmd::RandomPrev => {
                if self.lib.len() > 1 {
                    let i = self.random_index();
                    self.go_to(i);
                }
            }
            Cmd::LockZoom => {
                self.zoom_locked = !self.zoom_locked;
                self.notify(if self.zoom_locked {
                    "ZOOM LOCKED"
                } else {
                    "ZOOM UNLOCKED"
                });
            }
            Cmd::CycleOsd => {
                use crate::core::settings::Osd;
                self.settings.osd = match self.settings.osd {
                    Osd::Off => Osd::Name,
                    Osd::Name => Osd::Full,
                    Osd::Full => Osd::Off,
                };
                self.notify(match self.settings.osd {
                    Osd::Off => "ON-SCREEN INFO OFF",
                    Osd::Name => "ON-SCREEN INFO NAME",
                    Osd::Full => "ON-SCREEN INFO FULL",
                });
            }
            Cmd::Rename => {
                if let Some(p) = self.lib.current().filter(|p| p.exists()) {
                    self.rename = Some(folder::file_name(p));
                }
            }
            Cmd::PinTop => {
                self.settings.pin_top = !self.settings.pin_top;
                self.notify(if self.settings.pin_top {
                    "TOP BAR PINNED"
                } else {
                    "TOP BAR AUTO-HIDE"
                });
            }
            Cmd::PinBottom => {
                self.settings.pin_bottom = !self.settings.pin_bottom;
                self.notify(if self.settings.pin_bottom {
                    "BOTTOM BAR PINNED"
                } else {
                    "BOTTOM BAR AUTO-HIDE"
                });
            }
        }
    }

    /// Arrow keys, Enter and Esc in the thumbnail grid. Returns true if the
    /// key was used.
    fn grid_key(&mut self, key: egui::Key) -> bool {
        use crate::ui::grid::moved;
        let (len, cols, i) = (self.lib.len(), self.grid_cols.max(1), self.lib.index);
        let target = match key {
            egui::Key::ArrowLeft => moved(i, len, cols, -1, 0),
            egui::Key::ArrowRight => moved(i, len, cols, 1, 0),
            egui::Key::ArrowUp => moved(i, len, cols, 0, -1),
            egui::Key::ArrowDown => moved(i, len, cols, 0, 1),
            egui::Key::PageUp => moved(i, len, cols, 0, -4),
            egui::Key::PageDown => moved(i, len, cols, 0, 4),
            egui::Key::Home => 0,
            egui::Key::End => len.saturating_sub(1),
            egui::Key::Enter | egui::Key::Escape => {
                self.grid = false;
                return true;
            }
            _ => return false,
        };
        self.go_to(target);
        self.grid_scroll_to = true;
        true
    }

    pub(crate) fn handle_keys(&mut self, ctx: &egui::Context) {
        let presses: Vec<(egui::Key, egui::Modifiers, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        repeat,
                        ..
                    } => Some((*key, *modifiers, *repeat)),
                    _ => None,
                })
                .collect()
        });
        for (key, mods, repeat) in presses {
            if self.confirm_delete {
                match key {
                    egui::Key::Enter | egui::Key::Y => {
                        self.confirm_delete = false;
                        if let Some(p) = self.lib.current().cloned() {
                            match platform::delete_permanently(&p) {
                                Ok(()) => {
                                    self.forget_current();
                                    self.notify("DELETED");
                                }
                                Err(e) => self.notify(format!("DELETE FAILED: {e}")),
                            }
                        }
                    }
                    egui::Key::Escape | egui::Key::N => self.confirm_delete = false,
                    _ => {}
                }
                continue;
            }
            if self.capture.is_some() {
                self.capture_key(key, mods);
                continue;
            }
            if self.rename.is_some() {
                match key {
                    egui::Key::Enter => self.finish_rename(),
                    egui::Key::Escape => self.rename = None,
                    _ => {}
                }
                continue;
            }
            if self.show_settings {
                if matches!(key, egui::Key::Escape)
                    || self.keymap.lookup(key, mods) == Some(Cmd::Setup)
                {
                    self.show_settings = false;
                }
                continue;
            }
            if self.show_folders {
                let list = self.folder_list();
                let n = match key {
                    egui::Key::Num1 => Some(0),
                    egui::Key::Num2 => Some(1),
                    egui::Key::Num3 => Some(2),
                    egui::Key::Num4 => Some(3),
                    egui::Key::Num5 => Some(4),
                    egui::Key::Num6 => Some(5),
                    egui::Key::Num7 => Some(6),
                    egui::Key::Num8 => Some(7),
                    egui::Key::Num9 => Some(8),
                    _ => None,
                };
                if let Some(f) = n.and_then(|i| list.get(i)) {
                    let f = f.clone();
                    self.show_folders = false;
                    self.open(std::path::Path::new(&f));
                } else if key == egui::Key::S {
                    if let Some(dir) = self
                        .lib
                        .current()
                        .and_then(|c| c.parent())
                        .map(|d| d.display().to_string())
                    {
                        let starred = self.settings.toggle_favorite(&dir);
                        self.notify(if starred {
                            "FOLDER STARRED"
                        } else {
                            "FOLDER UNSTARRED"
                        });
                    }
                } else if key == egui::Key::Escape
                    || self.keymap.lookup(key, mods) == Some(Cmd::Folders)
                {
                    self.show_folders = false;
                }
                continue;
            }
            if self.show_help {
                // Any key closes the sheet; F1 and Esc are swallowed.
                self.show_help = false;
                if matches!(key, egui::Key::F1 | egui::Key::Escape) {
                    continue;
                }
            }
            if self.grid && mods.is_none() && self.grid_key(key) {
                continue;
            }
            if let Some(cmd) = self.keymap.lookup(key, mods) {
                if repeat && matches!(cmd, Cmd::Next | Cmd::Prev | Cmd::Forward10 | Cmd::Back10) {
                    // Key held down: skip decoding the images flown past.
                    self.rapid_until = Some(Instant::now() + RAPID_GRACE);
                    self.lib.set_rapid(true);
                }
                self.run(ctx, cmd);
            }
        }
        // Keep egui from moving keyboard focus with Tab.
        ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
        });
    }

    pub(crate) fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if let Some(p) = dropped.into_iter().next() {
            self.open(&p);
        }
    }

    pub(crate) fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        // Animation.
        if let (Some(d), Some(shown)) = (self.lib.current_decoded().cloned(), self.shown.clone()) {
            if d.is_animated() && self.lib.current() == Some(&shown) {
                let n = d.frames.len();
                let next = *self
                    .next_frame_at
                    .get_or_insert(now + d.frames[self.frame_index % n].delay);
                let waiting = !d.complete && self.frame_index + 1 >= n;
                if waiting {
                    // Still decoding: hold the last frame until more arrive.
                    self.next_frame_at = Some(now + Duration::from_millis(30));
                } else if now >= next {
                    self.frame_index = (self.frame_index + 1) % n;
                    let delay = d.frames[self.frame_index].delay;
                    let mut at = next + delay;
                    if at < now {
                        at = now + delay;
                    }
                    self.next_frame_at = Some(at);
                }
                if let Some(at) = self.next_frame_at {
                    ctx.request_repaint_after(at.saturating_duration_since(now));
                }
            }
        }
        // Slideshow.
        if let Some((every, at)) = self.slideshow {
            if self.lib.is_empty() {
                self.slideshow = None;
            } else if now >= at {
                let next = if self.lib.index + 1 >= self.lib.len() {
                    0
                } else {
                    self.lib.index + 1
                };
                if next == self.lib.index {
                    self.slideshow = None;
                } else {
                    self.go_to(next);
                    self.slideshow = Some((every, now + every));
                }
            } else {
                ctx.request_repaint_after(at - now);
            }
        }
        // Toast.
        if let Some((_, since)) = &self.toast {
            let age = now - *since;
            if age > Duration::from_millis(1800) {
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
    }

    /// Applies the rename sheet's text.
    fn finish_rename(&mut self) {
        let Some(new_name) = self.rename.take() else {
            return;
        };
        let new_name = new_name.trim().to_string();
        let Some(old) = self.lib.current().cloned() else {
            return;
        };
        if new_name.is_empty() || new_name == folder::file_name(&old) {
            return;
        }
        if new_name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
            self.notify("RENAME FAILED: \\ / : * ? \" < > |");
            return;
        }
        let new = old.with_file_name(&new_name);
        if new.exists() {
            self.notify(format!("RENAME FAILED: {new_name} exists"));
            return;
        }
        match std::fs::rename(&old, &new) {
            Ok(()) => {
                self.lib.replace_path(&old, new.clone());
                self.stars.rename(&old, &new);
                self.history.push(Action::Renamed { from: old, to: new });
                self.notify(format!("RENAMED · {new_name}"));
            }
            Err(e) => self.notify(format!("RENAME FAILED: {e}")),
        }
    }
}
