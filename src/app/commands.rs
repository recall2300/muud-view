//! Turning key presses and timers into actions.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Vec2, ViewportCommand};

use crate::app::MuudApp;
use crate::core::decode::upright;
use crate::core::keymap::{command_for, Cmd};
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
            Cmd::CopyImage => {
                if let Some(d) = self.lib.current_decoded().cloned() {
                    let frame = d.frames[self.frame_index.min(d.frames.len() - 1)]
                        .image
                        .clone();
                    let downscaled = d.is_downscaled();
                    ctx.copy_image(upright(&frame, d.orientation));
                    self.notify(if downscaled {
                        "IMAGE COPIED (DOWNSCALED)"
                    } else {
                        "IMAGE COPIED"
                    });
                }
            }
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
                            self.forget_current();
                            self.notify("MOVED TO RECYCLE BIN");
                        }
                        Err(e) => self.notify(format!("DELETE FAILED: {e}")),
                    }
                }
            }
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
        }
    }

    pub(crate) fn handle_keys(&mut self, ctx: &egui::Context) {
        let presses: Vec<(egui::Key, egui::Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (key, mods) in presses {
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
            if self.show_help {
                // Any key closes the sheet; F1 and Esc are swallowed.
                self.show_help = false;
                if matches!(key, egui::Key::F1 | egui::Key::Escape) {
                    continue;
                }
            }
            if let Some(cmd) = command_for(key, mods) {
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
                if now >= next {
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
}
