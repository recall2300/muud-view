use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Align2, CentralPanel, Color32, CornerRadius, CursorIcon, Event, Id, LayerId,
    Mesh, MouseWheelUnit, Order, Panel, PointerButton, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, TextureFilter, TextureHandle, TextureOptions, Vec2, ViewportCommand,
};

use crate::input::{command_for, Cmd, HelpSection, HELP};
use crate::loader::{Decoded, Loader};
use crate::theme::*;
use crate::view::{clamp_offset, corner_uvs, corners, step_zoom, zoom_about, View, ZoomMode};
use crate::{folder, platform};

const CACHE_BUDGET: usize = 1024 * 1024 * 1024;
const TOP_BAR: f32 = 40.0;
const INFO_W: f32 = 340.0;
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Background {
    Ink,
    Paper,
    Gray,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub background: Background,
    pub smooth: bool,
    pub show_info: bool,
    pub stretch_small: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            background: Background::Ink,
            smooth: true,
            show_info: false,
            stretch_small: false,
        }
    }
}

enum Slot {
    Ready(Arc<Decoded>),
    Failed(String),
}

/// Debug/README helper: `MUUD_SCREENSHOT=out.png` saves the window and exits.
struct ScreenshotJob {
    path: PathBuf,
    frames: u32,
    requested: bool,
}

pub struct MuudApp {
    settings: Settings,
    loader: Loader,
    files: Vec<PathBuf>,
    index: usize,
    direction: i32,
    cache: HashMap<PathBuf, Slot>,
    lru: VecDeque<PathBuf>,
    textures: HashMap<PathBuf, Vec<TextureHandle>>,
    shown: Option<PathBuf>,
    view: View,
    last_scale: f32,
    last_canvas: Rect,
    frame_index: usize,
    next_frame_at: Option<Instant>,
    slideshow: Option<(Duration, Instant)>,
    toast: Option<(String, Instant)>,
    show_help: bool,
    confirm_delete: bool,
    on_top: bool,
    wheel_accum: f32,
    title: String,
    checker: Option<TextureHandle>,
    started: bool,
    pending_open: Option<PathBuf>,
    last_dir: Option<PathBuf>,
    open_error: Option<String>,
    screenshot: Option<ScreenshotJob>,
}

impl MuudApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        install_fonts(&cc.egui_ctx);
        apply_style(&cc.egui_ctx);
        let mut settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        if let Ok(show) = std::env::var("MUUD_SHOW") {
            settings.show_info = show.contains("info");
        }
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 4)
            - 1;
        let screenshot = std::env::var_os("MUUD_SCREENSHOT").map(|p| ScreenshotJob {
            path: PathBuf::from(p),
            frames: 0,
            requested: false,
        });
        let show_help = std::env::var("MUUD_SHOW")
            .map(|s| s.contains("help"))
            .unwrap_or(false);
        Self {
            settings,
            loader: Loader::new(cc.egui_ctx.clone(), threads.max(2)),
            files: Vec::new(),
            index: 0,
            direction: 1,
            cache: HashMap::new(),
            lru: VecDeque::new(),
            textures: HashMap::new(),
            shown: None,
            view: View::default(),
            last_scale: 1.0,
            last_canvas: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
            frame_index: 0,
            next_frame_at: None,
            slideshow: None,
            toast: None,
            show_help,
            confirm_delete: false,
            on_top: false,
            wheel_accum: 0.0,
            title: String::new(),
            checker: None,
            started: false,
            pending_open: initial,
            last_dir: None,
            open_error: None,
            screenshot,
        }
    }

    // -----------------------------------------------------------------------
    // Files and cache

    fn open(&mut self, path: &Path) {
        match folder::list_for(path) {
            Ok(l) if !l.files.is_empty() => {
                self.last_dir = l.files[l.index].parent().map(Path::to_path_buf);
                self.files = l.files;
                self.index = l.index;
                self.direction = 1;
                self.open_error = None;
                self.reset_view();
                self.request();
            }
            Ok(_) => self.notify("NO IMAGES IN THIS FOLDER"),
            Err(e) => {
                self.open_error = Some(e.to_string());
                self.notify("CANNOT OPEN");
            }
        }
    }

    fn current_path(&self) -> Option<&PathBuf> {
        self.files.get(self.index)
    }

    fn current_decoded(&self) -> Option<&Arc<Decoded>> {
        match self.cache.get(self.current_path()?) {
            Some(Slot::Ready(d)) => Some(d),
            _ => None,
        }
    }

    /// Files worth having decoded, most important first.
    fn wanted(&self) -> Vec<PathBuf> {
        if self.files.is_empty() {
            return Vec::new();
        }
        let i = self.index as i64;
        let d = self.direction.signum() as i64;
        let d = if d == 0 { 1 } else { d };
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

    fn request(&self) {
        let todo: Vec<PathBuf> = self
            .wanted()
            .into_iter()
            .filter(|p| !self.cache.contains_key(p))
            .collect();
        self.loader.want(todo);
    }

    fn poll(&mut self) {
        let mut got = false;
        for (path, result) in self.loader.poll() {
            let slot = match result {
                Ok(d) => Slot::Ready(d),
                Err(e) => Slot::Failed(e),
            };
            self.lru.retain(|p| p != &path);
            self.lru.push_back(path.clone());
            self.cache.insert(path, slot);
            got = true;
        }
        if got {
            self.evict();
        }
    }

    fn evict(&mut self) {
        let wanted: HashSet<PathBuf> = self.wanted().into_iter().collect();
        let size = |s: &Slot| match s {
            Slot::Ready(d) => d.bytes(),
            Slot::Failed(_) => 0,
        };
        let mut total: usize = self.cache.values().map(size).sum();
        let mut keep = VecDeque::new();
        while let Some(p) = self.lru.pop_front() {
            if total > CACHE_BUDGET && !wanted.contains(&p) {
                if let Some(s) = self.cache.remove(&p) {
                    total -= size(&s);
                }
            } else {
                keep.push_back(p);
            }
        }
        self.lru = keep;
        let shown = self.shown.clone();
        self.textures
            .retain(|p, _| wanted.contains(p) || Some(p) == shown.as_ref());
    }

    fn texture_options(&self) -> TextureOptions {
        TextureOptions {
            magnification: if self.settings.smooth {
                TextureFilter::Linear
            } else {
                TextureFilter::Nearest
            },
            minification: TextureFilter::Linear,
            mipmap_mode: Some(TextureFilter::Linear),
            ..TextureOptions::LINEAR
        }
    }

    fn upload(&mut self, ctx: &egui::Context, path: &Path) -> bool {
        if self.textures.contains_key(path) {
            return false;
        }
        let Some(Slot::Ready(d)) = self.cache.get(path) else {
            return false;
        };
        let opts = self.texture_options();
        let name = path.display().to_string();
        let handles = d
            .frames
            .iter()
            .map(|f| ctx.load_texture(name.clone(), f.image.clone(), opts))
            .collect();
        self.textures.insert(path.to_path_buf(), handles);
        true
    }

    fn ensure_textures(&mut self, ctx: &egui::Context) {
        let wanted = self.wanted();
        let Some(cur) = wanted.first().cloned() else {
            return;
        };
        let ready = self.upload(ctx, &cur) || self.textures.contains_key(&cur);
        if ready && self.shown.as_ref() != Some(&cur) {
            self.shown = Some(cur.clone());
            self.frame_index = 0;
            self.next_frame_at = None;
        }
        // Pre-upload one neighbour per frame so switching is instant.
        for p in wanted.iter().skip(1) {
            if self.upload(ctx, p) {
                ctx.request_repaint();
                break;
            }
        }
    }

    fn reset_view(&mut self) {
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

    fn go_to(&mut self, i: usize) {
        if self.files.is_empty() {
            return;
        }
        let i = i.min(self.files.len() - 1);
        if i == self.index {
            return;
        }
        self.direction = if i > self.index { 1 } else { -1 };
        self.index = i;
        self.reset_view();
        self.request();
    }

    fn step(&mut self, delta: i64) {
        if self.files.is_empty() {
            return;
        }
        let last = self.files.len() as i64 - 1;
        let target = self.index as i64 + delta;
        if target > last && self.index as i64 == last {
            self.notify("LAST IMAGE");
        } else if target < 0 && self.index == 0 {
            self.notify("FIRST IMAGE");
        }
        self.go_to(target.clamp(0, last.max(0)) as usize);
    }

    fn remove_current(&mut self) {
        let Some(path) = self.current_path().cloned() else {
            return;
        };
        self.files.remove(self.index);
        self.cache.remove(&path);
        self.textures.remove(&path);
        self.lru.retain(|p| p != &path);
        if self.shown.as_ref() == Some(&path) {
            self.shown = None;
        }
        if self.files.is_empty() {
            self.index = 0;
        } else {
            self.index = self.index.min(self.files.len() - 1);
        }
        self.reset_view();
        self.request();
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    // -----------------------------------------------------------------------
    // Commands

    fn run(&mut self, ctx: &egui::Context, cmd: Cmd) {
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
            Cmd::CloseFile => {
                self.files.clear();
                self.shown = None;
                self.textures.clear();
                self.slideshow = None;
            }
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
            Cmd::Last => self.go_to(self.files.len().saturating_sub(1)),
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
                if let Some(d) = self.current_decoded().cloned() {
                    let frame = d.frames[self.frame_index.min(d.frames.len() - 1)]
                        .image
                        .clone();
                    let downscaled = frame.size[0] < d.width as usize;
                    ctx.copy_image((*frame).clone());
                    self.notify(if downscaled {
                        "IMAGE COPIED (DOWNSCALED)"
                    } else {
                        "IMAGE COPIED"
                    });
                }
            }
            Cmd::CopyPath => {
                if let Some(p) = self.current_path().cloned() {
                    ctx.copy_text(p.display().to_string());
                    self.notify("PATH COPIED");
                }
            }
            Cmd::CopyExif => match self.current_decoded().and_then(|d| d.exif.clone()) {
                Some(e) => {
                    ctx.copy_text(e.summary());
                    self.notify("EXIF COPIED");
                }
                None => self.notify("NO EXIF"),
            },
            Cmd::ShowInExplorer => {
                if let Some(p) = self.current_path() {
                    if platform::show_in_explorer(p).is_err() {
                        self.notify("CANNOT OPEN EXPLORER");
                    }
                }
            }
            Cmd::Trash => {
                if let Some(p) = self.current_path().cloned() {
                    match platform::move_to_trash(&p) {
                        Ok(()) => {
                            self.remove_current();
                            self.notify("MOVED TO RECYCLE BIN");
                        }
                        Err(e) => self.notify(format!("DELETE FAILED: {e}")),
                    }
                }
            }
            Cmd::DeletePermanently => {
                if self.current_path().is_some() {
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

    fn handle_keys(&mut self, ctx: &egui::Context) {
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
                        if let Some(p) = self.current_path().cloned() {
                            match platform::delete_permanently(&p) {
                                Ok(()) => {
                                    self.remove_current();
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

    fn handle_drops(&mut self, ctx: &egui::Context) {
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

    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        // Animation.
        if let (Some(d), Some(shown)) = (self.current_decoded().cloned(), self.shown.clone()) {
            if d.is_animated() && self.current_path() == Some(&shown) {
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
            if self.files.is_empty() {
                self.slideshow = None;
            } else if now >= at {
                let next = if self.index + 1 >= self.files.len() {
                    0
                } else {
                    self.index + 1
                };
                if next == self.index {
                    self.slideshow = None;
                } else {
                    self.direction = 1;
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

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.current_path() {
            Some(p) => format!("{} — muud view", folder::file_name(p)),
            None => "muud view".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn canvas_color(&self) -> Color32 {
        match self.settings.background {
            Background::Ink => CANVAS_INK,
            Background::Paper => Color32::from_rgb(0xE6, 0xE4, 0xDE),
            Background::Gray => CANVAS_GRAY,
        }
    }

    fn on_canvas_color(&self) -> Color32 {
        match self.settings.background {
            Background::Paper => INK,
            _ => PAPER,
        }
    }

    // -----------------------------------------------------------------------
    // Top bar

    fn status_pills(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Some(d) = self.current_decoded() {
            v.push(format!("{} × {}", d.width, d.height));
            v.push(human_size(d.file_size));
            v.push(d.format.to_string());
            if d.is_animated() {
                v.push(format!("{} FRAMES", d.frames.len()));
            }
            v.push(format!("{:.0}%", self.last_scale * 100.0));
            v.push(
                match self.view.mode {
                    ZoomMode::Fit => "FIT",
                    ZoomMode::FitWidth => "WIDTH",
                    ZoomMode::Original => "1:1",
                    ZoomMode::Free(_) => "ZOOM",
                }
                .to_string(),
            );
            if self.view.turns != 0 {
                v.push(format!("R{}°", self.view.turns as u32 * 90));
            }
            if self.view.flip_h || self.view.flip_v {
                v.push("FLIP".into());
            }
        }
        if let Some((every, _)) = self.slideshow {
            v.push(format!("▶ {}S", every.as_secs()));
        }
        if self.on_top {
            v.push("PIN".into());
        }
        v
    }

    fn top_bar(&self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let p = ui.painter();
        p.line_segment(
            [
                rect.left_bottom() - vec2(0.0, 0.5),
                rect.right_bottom() - vec2(0.0, 0.5),
            ],
            hair(),
        );
        let cy = rect.center().y;

        let brand = p.layout_no_wrap("MUUD VIEW".into(), grotesk(14.0), INK);
        let brand_x = rect.right() - 16.0 - brand.size().x;
        p.galley(pos2(brand_x, cy - brand.size().y / 2.0), brand, INK);
        let hint = p.layout_no_wrap("F1 KEYS".into(), mono(11.0), MUTED);
        let hint_x = brand_x - 14.0 - hint.size().x;
        p.galley(pos2(hint_x, cy - hint.size().y / 2.0), hint, MUTED);
        let right_limit = hint_x - 18.0;

        let mut x = rect.left() + 16.0;
        let Some(path) = self.current_path() else {
            p.text(
                pos2(x, cy),
                Align2::LEFT_CENTER,
                "NO FILE",
                mono(12.0),
                MUTED,
            );
            return;
        };
        let counter = format!("{:03} / {:03}", self.index + 1, self.files.len());
        let r = p.text(
            pos2(x, cy),
            Align2::LEFT_CENTER,
            counter,
            mono_medium(12.0),
            INK,
        );
        x = r.right() + 14.0;

        let pills = self.status_pills();
        let pills_w: f32 = pills.iter().map(|t| pill_width(p, t) + 6.0).sum();
        let name_max = (right_limit - x - pills_w - 14.0).max(90.0);
        let name = ellipsize(p, &folder::file_name(path), grotesk_semi(14.5), name_max);
        let r = p.text(
            pos2(x, cy),
            Align2::LEFT_CENTER,
            name,
            grotesk_semi(14.5),
            INK,
        );
        x = r.right() + 14.0;
        for t in pills {
            let w = pill_width(p, &t);
            if x + w > right_limit {
                break;
            }
            pill(p, pos2(x, cy), &t, INK);
            x += w + 6.0;
        }
    }

    // -----------------------------------------------------------------------
    // Canvas

    fn checker_texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        self.checker
            .get_or_insert_with(|| {
                let a = Color32::from_gray(0xD9);
                let b = Color32::from_gray(0xF2);
                let img = egui::ColorImage::new([2, 2], vec![a, b, b, a]);
                ctx.load_texture("checker", img, TextureOptions::NEAREST_REPEAT)
            })
            .id()
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let p = ui.painter_at(rect);
        self.last_canvas = rect;

        if self.files.is_empty() {
            self.empty_state(&p, rect);
            return;
        }
        let cur = self.current_path().cloned();
        if let Some(Slot::Failed(err)) = cur.as_ref().and_then(|c| self.cache.get(c)) {
            let name = cur
                .as_ref()
                .map(|c| folder::file_name(c))
                .unwrap_or_default();
            message_card(&p, rect.center(), "CANNOT OPEN", &name, err);
            self.wheel_navigation(&ctx, &resp);
            return;
        }
        let draw_path = self.shown.clone().filter(|s| self.textures.contains_key(s));
        let Some(draw_path) = draw_path else {
            let name = cur
                .as_ref()
                .map(|c| folder::file_name(c))
                .unwrap_or_default();
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "DECODING …",
                mono(12.0),
                self.on_canvas_color(),
            );
            p.text(
                rect.center() + vec2(0.0, 20.0),
                Align2::CENTER_CENTER,
                name,
                mono(11.0),
                self.on_canvas_color().gamma_multiply(0.6),
            );
            return;
        };
        let Some(Slot::Ready(d)) = self.cache.get(&draw_path) else {
            return;
        };
        let d = d.clone();

        let ppp = ctx.pixels_per_point();
        let img = self.view.rotated(d.width as f32, d.height as f32);
        let mut scale = self
            .view
            .scale(img, rect.size(), ppp, self.settings.stretch_small);
        let mut shown = img * scale / ppp;
        if self.view.align_top {
            self.view.offset = vec2(0.0, (shown.y - rect.height()) / 2.0);
            self.view.align_top = false;
        }

        // Ctrl+wheel zooms at the cursor; the wheel alone scrolls a tall image
        // or moves to the next/previous image.
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let wheel: Vec<(f32, bool)> = if resp.hovered() {
            ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        Event::MouseWheel {
                            unit,
                            delta,
                            modifiers,
                            ..
                        } => {
                            let notches = match unit {
                                MouseWheelUnit::Line => delta.y,
                                MouseWheelUnit::Point => delta.y / 50.0,
                                MouseWheelUnit::Page => delta.y * 3.0,
                            };
                            Some((notches, modifiers.ctrl || modifiers.command))
                        }
                        _ => None,
                    })
                    .collect()
            })
        } else {
            Vec::new()
        };
        let overflow_y = shown.y > rect.height() + 0.5;
        for (notches, ctrl) in wheel {
            if ctrl {
                let new = (scale * 1.15f32.powf(notches))
                    .clamp(crate::view::MIN_SCALE, crate::view::MAX_SCALE);
                let anchor = pointer.unwrap_or(rect.center());
                self.view.offset = zoom_about(self.view.offset, rect.center(), anchor, scale, new);
                self.view.mode = ZoomMode::Free(new);
                scale = new;
                shown = img * scale / ppp;
            } else if overflow_y {
                self.view.offset.y += notches * 90.0;
            } else {
                self.wheel_accum -= notches;
                while self.wheel_accum >= 1.0 {
                    self.wheel_accum -= 1.0;
                    self.run(&ctx, Cmd::Next);
                }
                while self.wheel_accum <= -1.0 {
                    self.wheel_accum += 1.0;
                    self.run(&ctx, Cmd::Prev);
                }
            }
        }

        if resp.dragged_by(PointerButton::Primary) {
            self.view.offset += resp.drag_delta();
        }
        if resp.double_clicked() {
            self.run(&ctx, Cmd::Maximize);
        }
        if resp.clicked_by(PointerButton::Middle) {
            if ctx.input(|i| i.modifiers.ctrl) {
                self.run(&ctx, Cmd::Original);
            } else {
                self.run(&ctx, Cmd::Fullscreen);
            }
        }

        self.view.offset = clamp_offset(self.view.offset, shown, rect.size());
        self.last_scale = scale;
        let can_pan = shown.x > rect.width() + 0.5 || shown.y > rect.height() + 0.5;
        if can_pan && resp.hovered() {
            ctx.set_cursor_icon(if resp.dragged() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }

        // Snap to physical pixels so 100% stays crisp.
        let center = rect.center() + self.view.offset;
        let min = center - shown / 2.0;
        let min = pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp);
        let dest = Rect::from_min_size(min, shown);

        if d.has_alpha {
            let checker = self.checker_texture(&ctx);
            let tiles = dest.size() / 16.0;
            p.image(
                checker,
                dest,
                Rect::from_min_max(Pos2::ZERO, pos2(tiles.x / 2.0, tiles.y / 2.0)),
                Color32::WHITE,
            );
        }

        let textures = &self.textures[&draw_path];
        let tex = &textures[self.frame_index.min(textures.len() - 1)];
        let mut mesh = Mesh::with_texture(tex.id());
        let uvs = corner_uvs(self.view.turns, self.view.flip_h, self.view.flip_v);
        for (pos, uv) in corners(dest).into_iter().zip(uvs) {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv,
                color: Color32::WHITE,
            });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        p.add(Shape::mesh(mesh));
    }

    fn wheel_navigation(&mut self, ctx: &egui::Context, resp: &egui::Response) {
        if !resp.hovered() {
            return;
        }
        let notches: f32 = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } if !modifiers.ctrl => Some(match unit {
                        MouseWheelUnit::Line => delta.y,
                        MouseWheelUnit::Point => delta.y / 50.0,
                        MouseWheelUnit::Page => delta.y * 3.0,
                    }),
                    _ => None,
                })
                .sum()
        });
        self.wheel_accum -= notches;
        if self.wheel_accum >= 1.0 {
            self.wheel_accum = 0.0;
            self.run(ctx, Cmd::Next);
        } else if self.wheel_accum <= -1.0 {
            self.wheel_accum = 0.0;
            self.run(ctx, Cmd::Prev);
        }
    }

    fn empty_state(&self, p: &egui::Painter, rect: Rect) {
        let w = 440.0f32.min(rect.width() - 32.0);
        let h = 372.0;
        let card = Rect::from_center_size(rect.center(), vec2(w, h));
        p.rect_filled(card, CornerRadius::ZERO, PAPER);
        p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
        let x0 = card.left() + 24.0;
        let x1 = card.right() - 24.0;
        let mut y = card.top() + 22.0;
        let r = p.text(
            pos2(x0, y),
            Align2::LEFT_TOP,
            "MUUD VIEW",
            grotesk(40.0),
            INK,
        );
        y = r.bottom() + 12.0;
        let mut x = x0;
        for t in [
            format!("v{VERSION}"),
            "NO ADS".into(),
            "OFFLINE".into(),
            "RUST".into(),
        ] {
            x += pill(p, pos2(x, y + 11.0), &t, INK) + 6.0;
        }
        y += 22.0 + 22.0;

        // Boxed section, like a spec sheet cell.
        let boxr = Rect::from_min_max(
            pos2(card.left() + 12.0, y),
            pos2(card.right() - 12.0, card.bottom() - 12.0),
        );
        p.rect_stroke(boxr, CornerRadius::ZERO, hair(), StrokeKind::Inside);
        let bx = boxr.left() + 16.0;
        section_title(p, pos2(bx, boxr.top() + 14.0), "OPEN");
        let cx = boxr.center().x;
        let mut yy = boxr.top() + 50.0;
        p.text(
            pos2(cx, yy),
            Align2::CENTER_TOP,
            "DROP AN IMAGE",
            grotesk(26.0),
            INK,
        );
        yy += 42.0;
        dotted_rule(p, pos2(bx, yy), pos2(boxr.right() - 16.0, yy), INK);
        yy += 16.0;
        let lines: &[&str] = if platform::dialogs_available() {
            &[
                "CTRL+O  OPEN FILE",
                "F  OPEN FOLDER",
                "↓",
                "F1  ALL SHORTCUTS",
            ]
        } else {
            &["OR START WITH  muud-view <FILE>", "↓", "F1  ALL SHORTCUTS"]
        };
        for line in lines {
            let r = p.text(pos2(cx, yy), Align2::CENTER_TOP, *line, mono(12.5), INK);
            yy = r.bottom() + 8.0;
        }
        let _ = x1;
    }

    // -----------------------------------------------------------------------
    // Info panel

    fn info_panel(&self, ui: &mut egui::Ui) {
        let (Some(d), Some(path)) = (self.current_decoded(), self.current_path()) else {
            return;
        };
        let outer = ui.max_rect();
        ui.painter().line_segment(
            [
                outer.left_top() + vec2(0.5, 0.0),
                outer.left_bottom() + vec2(0.5, 0.0),
            ],
            hair(),
        );
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let top = ui.cursor().top();
                let left = ui.max_rect().left();
                let width = ui.available_width();
                let p = ui.painter().clone();
                let x0 = left + 16.0;
                let x1 = left + width - 16.0;
                let mut y = top + 18.0;

                // Header: name, pills, colour line.
                let name = p.layout(folder::file_name(path), grotesk(21.0), INK, x1 - x0);
                let h = name.size().y;
                p.galley(pos2(x0, y), name, INK);
                y += h + 12.0;
                let mut x = x0;
                let mp = d.width as f64 * d.height as f64 / 1e6;
                for t in [
                    d.format.to_string(),
                    human_size(d.file_size),
                    format!("{} MP", trim1(mp)),
                ] {
                    x += pill(&p, pos2(x, y + 11.0), &t, INK) + 6.0;
                }
                y += 22.0 + 14.0;
                let r = p.text(
                    pos2(x0, y),
                    Align2::LEFT_TOP,
                    "Color",
                    grotesk_semi(13.0),
                    INK,
                );
                let alpha = if d.has_alpha { "ALPHA" } else { "OPAQUE" };
                p.text(
                    pos2(r.right() + 8.0, y + 1.0),
                    Align2::LEFT_TOP,
                    format!("{} · {}", d.color, alpha),
                    mono(12.0),
                    INK,
                );
                y += 30.0;

                y = self.box_resolution(&p, x0 - 4.0, x1 + 4.0, y, d);
                if let Some(e) = &d.exif {
                    y = box_camera(&p, x0 - 4.0, x1 + 4.0, y, e);
                }
                y = self.box_file(&p, x0 - 4.0, x1 + 4.0, y, d, path);
                y = box_histogram(&p, x0 - 4.0, x1 + 4.0, y, &d.histogram);
                if let Some((lat, lon)) = d.exif.as_ref().and_then(|e| e.gps) {
                    let alt = d.exif.as_ref().and_then(|e| e.altitude.clone());
                    y = box_location(&p, x0 - 4.0, x1 + 4.0, y, lat, lon, alt);
                }
                ui.allocate_space(vec2(width, y - top + 24.0));
            });
    }

    fn box_resolution(&self, p: &egui::Painter, bx0: f32, bx1: f32, y: f32, d: &Decoded) -> f32 {
        let pad = 14.0;
        let mut yy = y + pad;
        section_title(p, pos2(bx0 + pad, yy), "RESOLUTION");
        yy += 30.0;
        let cx = (bx0 + bx1) / 2.0;
        let mp = d.width as f64 * d.height as f64 / 1e6;
        let big = p.layout_no_wrap(trim1(mp), grotesk(54.0), INK);
        let unit = p.layout_no_wrap("MP".into(), mono(24.0), INK);
        let total = big.size().x + 6.0 + unit.size().x;
        let bx = cx - total / 2.0;
        let big_h = big.size().y;
        p.galley(pos2(bx, yy), big.clone(), INK);
        p.galley(
            pos2(bx + big.size().x + 6.0, yy + big_h - unit.size().y - 6.0),
            unit,
            INK,
        );
        yy += big_h + 8.0;
        dotted_rule(p, pos2(bx0 + pad, yy), pos2(bx1 - pad, yy), INK);
        yy += 14.0;
        let mut lines = vec![
            format!("{} × {} PX", d.width, d.height),
            format!("ASPECT {}", aspect(d.width, d.height)),
        ];
        let tex = d.texture_size();
        if tex[0] < d.width as usize {
            lines.push(format!("TEXTURE {} × {}", tex[0], tex[1]));
        }
        lines.push("↓".into());
        lines.push(format!("SHOWN AT {:.0}%", self.last_scale * 100.0));
        for l in lines {
            let r = p.text(pos2(cx, yy), Align2::CENTER_TOP, l, mono(12.5), INK);
            yy = r.bottom() + 6.0;
        }
        yy += pad - 6.0;
        let r = Rect::from_min_max(pos2(bx0, y), pos2(bx1, yy));
        p.rect_stroke(r, CornerRadius::ZERO, hair(), StrokeKind::Inside);
        yy - HAIR
    }

    fn box_file(
        &self,
        p: &egui::Painter,
        bx0: f32,
        bx1: f32,
        y: f32,
        d: &Decoded,
        path: &Path,
    ) -> f32 {
        let pad = 14.0;
        let mut yy = y + pad;
        section_title(p, pos2(bx0 + pad, yy), "FILE");
        yy += 28.0;
        let kx = bx0 + pad;
        let vx = bx0 + pad + 96.0;
        let vw = bx1 - pad - vx;
        let modified = d.modified.map(|t| {
            chrono::DateTime::<chrono::Local>::from(t)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        });
        let folder = path
            .parent()
            .map(|f| f.display().to_string())
            .unwrap_or_default();
        let rows: Vec<(&str, String)> = vec![
            (
                "INDEX",
                format!("{} / {}", self.index + 1, self.files.len()),
            ),
            ("MODIFIED", modified.unwrap_or_else(|| "—".into())),
            ("DECODE", format!("{} MS", d.decode_time.as_millis())),
            ("FOLDER", folder),
        ];
        for (k, v) in rows {
            yy += kv_row(p, kx, vx, yy, k, &v, vw);
        }
        yy += pad - 3.0;
        p.rect_stroke(
            Rect::from_min_max(pos2(bx0, y), pos2(bx1, yy)),
            CornerRadius::ZERO,
            hair(),
            StrokeKind::Inside,
        );
        yy - HAIR
    }

    // -----------------------------------------------------------------------
    // Overlays

    fn overlays(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let p = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("muud-overlay")));

        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if hovering {
            p.rect_filled(screen, CornerRadius::ZERO, INK.gamma_multiply(0.7));
            let card = Rect::from_center_size(screen.center(), vec2(320.0, 120.0));
            p.rect_filled(card, CornerRadius::ZERO, PAPER);
            p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
            p.text(
                card.center() - vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                "DROP TO OPEN",
                grotesk(24.0),
                INK,
            );
            p.text(
                card.center() + vec2(0.0, 22.0),
                Align2::CENTER_CENTER,
                "↓",
                mono(14.0),
                INK,
            );
        }

        if self.show_help {
            p.rect_filled(screen, CornerRadius::ZERO, INK.gamma_multiply(0.75));
            help_sheet(&p, screen);
            if ctx.input(|i| i.pointer.any_pressed()) {
                self.show_help = false;
            }
        }

        if self.confirm_delete {
            p.rect_filled(screen, CornerRadius::ZERO, INK.gamma_multiply(0.75));
            let name = self
                .current_path()
                .map(|c| folder::file_name(c))
                .unwrap_or_default();
            message_card(
                &p,
                screen.center(),
                "DELETE PERMANENTLY?",
                &name,
                "ENTER  DELETE      ESC  CANCEL",
            );
        }

        if let Some((text, since)) = &self.toast {
            let age = since.elapsed().as_secs_f32();
            let alpha = if age > 1.4 {
                ((1.8 - age) / 0.4).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let galley =
                p.layout_no_wrap(text.clone(), mono_medium(12.0), INK.gamma_multiply(alpha));
            let size = galley.size() + vec2(28.0, 14.0);
            let r = Rect::from_center_size(pos2(screen.center().x, screen.bottom() - 48.0), size);
            p.rect_filled(r, CornerRadius::same(255), PAPER.gamma_multiply(alpha));
            p.rect_stroke(
                r,
                CornerRadius::same(255),
                Stroke::new(1.0, INK.gamma_multiply(alpha)),
                StrokeKind::Inside,
            );
            p.galley(r.min + vec2(14.0, 7.0), galley, INK);
        }
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.screenshot else {
            return;
        };
        job.frames += 1;
        // Wait until the first image is on screen (or a few seconds pass).
        let ready = self.files.is_empty() || self.shown.is_some();
        if !job.requested && ((ready && job.frames > 20) || job.frames > 600) {
            job.requested = true;
            let path = job.path.clone();
            let c = ctx.clone();
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let mut buf = Vec::with_capacity(w * h * 4);
                for px in &img.pixels {
                    buf.extend_from_slice(&px.to_srgba_unmultiplied());
                }
                if let Some(rgba) = image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                    let _ = rgba.save(&path);
                }
                c.send_viewport_cmd(ViewportCommand::Close);
                c.request_repaint();
            });
        }
        ctx.request_repaint();
    }
}

impl eframe::App for MuudApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.started {
            // The real texture limit is only known once the renderer runs.
            let side = ctx.input(|i| i.max_texture_side).clamp(2048, 16384);
            self.loader.set_max_texture_side(side);
            self.started = true;
            if let Some(p) = self.pending_open.take() {
                self.open(&p);
            }
        }

        self.poll();
        self.handle_drops(&ctx);
        self.handle_keys(&ctx);
        self.tick(&ctx);
        self.ensure_textures(&ctx);
        self.update_title(&ctx);

        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        if !fullscreen {
            Panel::top("top")
                .exact_size(TOP_BAR)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(PAPER))
                .show(ui, |ui| self.top_bar(ui));
        }
        if self.settings.show_info && self.current_decoded().is_some() {
            Panel::right("info")
                .exact_size(INFO_W)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(PAPER))
                .show(ui, |ui| self.info_panel(ui));
        }
        let bg = self.canvas_color();
        CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(bg))
            .show(ui, |ui| self.canvas(ui));

        self.overlays(&ctx);
        self.screenshot(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.canvas_color().to_normalized_gamma_f32()
    }
}

// ---------------------------------------------------------------------------
// Free-standing sheet sections

fn trim1(v: f64) -> String {
    crate::exif_info::trim_float(v, 1)
}

fn aspect(w: u32, h: u32) -> String {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }
    let g = gcd(w, h).max(1);
    let (a, b) = (w / g, h / g);
    if a <= 32 && b <= 32 {
        format!("{a}:{b}")
    } else {
        format!("{:.2}:1", w as f64 / h.max(1) as f64)
    }
}

fn box_camera(
    p: &egui::Painter,
    bx0: f32,
    bx1: f32,
    y: f32,
    e: &crate::exif_info::ExifInfo,
) -> f32 {
    let pad = 14.0;
    let mut yy = y + pad;
    section_title(p, pos2(bx0 + pad, yy), "CAMERA");
    yy += 30.0;
    let kx = bx0 + pad;
    let vx = bx0 + pad + 96.0;
    let vw = bx1 - pad - vx;
    let body: Vec<(&str, Option<String>)> = vec![
        ("MODEL", e.camera()),
        ("LENS", e.lens.clone()),
        ("TAKEN", e.taken.clone()),
    ];
    if body.iter().any(|(_, v)| v.is_some()) {
        yy += sub_label(p, pos2(kx, yy), "BODY") + 6.0;
        for (k, v) in body {
            if let Some(v) = v {
                yy += kv_row(p, kx, vx, yy, k, &v, vw);
            }
        }
        yy += 10.0;
    }
    let focal = match (&e.focal, &e.focal_35) {
        (Some(f), Some(f35)) if f != f35 => Some(format!("{f} ({f35} EQ)")),
        (Some(f), _) => Some(f.clone()),
        (None, f35) => f35.clone(),
    };
    let exposure: Vec<(&str, Option<String>)> = vec![
        ("APERTURE", e.aperture.clone()),
        ("SHUTTER", e.shutter.clone()),
        ("ISO", e.iso.clone()),
        ("FOCAL", focal),
        ("BIAS", e.bias.clone()),
        ("FLASH", e.flash.clone()),
    ];
    if exposure.iter().any(|(_, v)| v.is_some()) {
        yy += sub_label(p, pos2(kx, yy), "EXPOSURE") + 6.0;
        for (k, v) in exposure {
            if let Some(v) = v {
                yy += kv_row(p, kx, vx, yy, k, &v, vw);
            }
        }
    }
    yy += pad - 3.0;
    p.rect_stroke(
        Rect::from_min_max(pos2(bx0, y), pos2(bx1, yy)),
        CornerRadius::ZERO,
        hair(),
        StrokeKind::Inside,
    );
    yy - HAIR
}

fn box_histogram(p: &egui::Painter, bx0: f32, bx1: f32, y: f32, hist: &[u32]) -> f32 {
    let pad = 14.0;
    let mut yy = y + pad;
    section_title(p, pos2(bx0 + pad, yy), "HISTOGRAM");
    yy += 20.0;
    p.text(
        pos2(bx0 + pad, yy),
        Align2::LEFT_TOP,
        "LUMINANCE - 256 BINS - SQRT",
        mono(11.5),
        INK,
    );
    yy += 26.0;

    let chart = Rect::from_min_max(pos2(bx0 + pad, yy), pos2(bx1 - pad, yy + 112.0));
    // Robust maximum: ignore the clipped ends so a spike at 0 or 255 does not flatten the curve.
    let mut sorted: Vec<u32> = hist[1..255].to_vec();
    sorted.sort_unstable();
    let robust = sorted[(sorted.len() as f32 * 0.995) as usize - 1].max(1) as f32;
    let v = |i: usize| ((hist[i] as f32 / robust).min(1.0)).sqrt();
    let plot_top = chart.top() + 18.0;
    let plot_bottom = chart.bottom() - 16.0;
    let x_at = |i: usize| chart.left() + chart.width() * i as f32 / 255.0;
    let y_at = |t: f32| plot_bottom - (plot_bottom - plot_top) * t;

    for (t, label) in [(1.0, "MAX"), (0.5, "25%")] {
        let gy = y_at(t);
        dashed_rule(p, pos2(chart.left(), gy), pos2(chart.right(), gy), INK);
        p.text(
            pos2(chart.left() + 2.0, gy - 2.0),
            Align2::LEFT_BOTTOM,
            label,
            mono(10.0),
            INK,
        );
    }
    // Lightly smoothed curve.
    let pts: Vec<Pos2> = (0..256usize)
        .map(|i| {
            let a = v(i.saturating_sub(1));
            let b = v(i);
            let c = v((i + 1).min(255));
            pos2(x_at(i), y_at((a + 2.0 * b + c) / 4.0))
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(1.4, INK)));

    let peak = (1..255).max_by_key(|&i| hist[i]).unwrap_or(128);
    let px = x_at(peak);
    let py = y_at(v(peak));
    p.circle_filled(pos2(px, py), 3.2, INK);
    p.line_segment(
        [pos2(px, py - 4.0), pos2(px, plot_top - 4.0)],
        Stroke::new(1.0, INK),
    );
    let lbl = format!("{peak} PEAK");
    let align = if peak > 200 {
        Align2::RIGHT_BOTTOM
    } else if peak < 55 {
        Align2::LEFT_BOTTOM
    } else {
        Align2::CENTER_BOTTOM
    };
    p.text(pos2(px, plot_top - 5.0), align, lbl, mono(10.5), INK);

    // Axis.
    p.line_segment(
        [
            pos2(chart.left(), plot_bottom + 1.0),
            pos2(chart.right(), plot_bottom + 1.0),
        ],
        Stroke::new(1.0, INK),
    );
    for (i, label, align) in [
        (0usize, "0", Align2::LEFT_TOP),
        (128, "128", Align2::CENTER_TOP),
        (255, "255", Align2::RIGHT_TOP),
    ] {
        let x = x_at(i);
        p.line_segment(
            [pos2(x, plot_bottom + 1.0), pos2(x, plot_bottom + 5.0)],
            Stroke::new(1.0, INK),
        );
        p.text(pos2(x, plot_bottom + 5.0), align, label, mono(10.5), INK);
    }
    yy = chart.bottom() + pad;
    p.rect_stroke(
        Rect::from_min_max(pos2(bx0, y), pos2(bx1, yy)),
        CornerRadius::ZERO,
        hair(),
        StrokeKind::Inside,
    );
    yy - HAIR
}

fn box_location(
    p: &egui::Painter,
    bx0: f32,
    bx1: f32,
    y: f32,
    lat: f64,
    lon: f64,
    alt: Option<String>,
) -> f32 {
    let pad = 14.0;
    let mut yy = y + pad;
    section_title(p, pos2(bx0 + pad, yy), "LOCATION");
    yy += 30.0;
    // A small crosshair diagram, then coordinates beside a dotted divider.
    let d = Rect::from_min_size(pos2(bx0 + pad, yy), vec2(108.0, 96.0));
    let c = d.center();
    let s = Stroke::new(1.0, INK);
    p.add(Shape::ellipse_stroke(c, vec2(40.0, 26.0), s));
    p.arrow(c, vec2(0.0, -46.0), s);
    p.arrow(c, vec2(0.0, 46.0), s);
    p.arrow(c, vec2(-52.0, 0.0), s);
    p.arrow(c, vec2(52.0, 0.0), s);
    for o in [
        vec2(0.0, 0.0),
        vec2(0.0, -26.0),
        vec2(0.0, 26.0),
        vec2(-40.0, 0.0),
        vec2(40.0, 0.0),
    ] {
        p.circle_stroke(c + o, 5.0, s);
    }
    let div = d.right() + 14.0;
    p.extend(Shape::dotted_line(
        &[pos2(div, d.top()), pos2(div, d.bottom())],
        INK,
        4.0,
        0.7,
    ));
    let tx = div + 12.0;
    let ns = if lat >= 0.0 { "N" } else { "S" };
    let ew = if lon >= 0.0 { "E" } else { "W" };
    let mut ty = d.top() + 4.0;
    let r = p.text(
        pos2(tx, ty),
        Align2::LEFT_TOP,
        "GPS",
        grotesk_semi(13.0),
        INK,
    );
    ty = r.bottom() + 8.0;
    for line in [
        format!("{:.5}° {ns}", lat.abs()),
        format!("{:.5}° {ew}", lon.abs()),
        alt.map(|a| format!("ALT {a}")).unwrap_or_default(),
    ] {
        if !line.is_empty() {
            let r = p.text(pos2(tx, ty), Align2::LEFT_TOP, line, mono(12.0), INK);
            ty = r.bottom() + 4.0;
        }
    }
    let _ = bx1;
    yy = d.bottom() + pad;
    p.rect_stroke(
        Rect::from_min_max(pos2(bx0, y), pos2(bx1, yy)),
        CornerRadius::ZERO,
        hair(),
        StrokeKind::Inside,
    );
    yy - HAIR
}

fn message_card(p: &egui::Painter, center: Pos2, title: &str, name: &str, detail: &str) {
    let w = 420.0;
    let name = ellipsize(p, name, mono(12.0), w - 48.0);
    let detail_g = p.layout(detail.to_owned(), mono(12.0), INK, w - 48.0);
    let h = 132.0 + detail_g.size().y;
    let card = Rect::from_center_size(center, vec2(w, h));
    p.rect_filled(card, CornerRadius::ZERO, PAPER);
    p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
    let x0 = card.left() + 24.0;
    let mut y = card.top() + 22.0;
    let r = p.text(pos2(x0, y), Align2::LEFT_TOP, title, grotesk(24.0), INK);
    y = r.bottom() + 10.0;
    p.text(pos2(x0, y), Align2::LEFT_TOP, name, mono(12.0), INK);
    y += 26.0;
    dotted_rule(p, pos2(x0, y), pos2(card.right() - 24.0, y), INK);
    y += 14.0;
    p.galley(pos2(x0, y), detail_g, INK);
}

fn help_sheet(p: &egui::Painter, screen: Rect) {
    let w = 760.0f32.min(screen.width() - 32.0);
    let col_w = (w - 24.0 * 2.0 - 24.0) / 2.0;
    let row_h = 19.0;
    let section_h = |rows: usize| 28.0 + rows as f32 * row_h + 18.0;
    let (left, right): (Vec<_>, Vec<_>) = HELP.iter().enumerate().partition(|(i, _)| i % 2 == 0);
    let col_h =
        |c: &[(usize, &HelpSection)]| c.iter().map(|(_, (_, r))| section_h(r.len())).sum::<f32>();
    let body_h = col_h(&left).max(col_h(&right));
    let h = (96.0 + body_h).min(screen.height() - 32.0);
    let card = Rect::from_center_size(screen.center(), vec2(w, h));
    p.rect_filled(card, CornerRadius::ZERO, PAPER);
    p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
    let p = p.with_clip_rect(card);
    let x0 = card.left() + 24.0;
    let mut y = card.top() + 20.0;
    let r = p.text(
        pos2(x0, y),
        Align2::LEFT_TOP,
        "SHORTCUTS",
        grotesk(32.0),
        INK,
    );
    let mut x = r.right() + 14.0;
    for t in [format!("v{VERSION}"), "ANY KEY TO CLOSE".into()] {
        x += pill(&p, pos2(x, r.center().y), &t, INK) + 6.0;
    }
    y = r.bottom() + 16.0;
    p.line_segment([pos2(card.left(), y), pos2(card.right(), y)], hair());
    let mid = card.left() + w / 2.0;
    p.line_segment([pos2(mid, y), pos2(mid, card.bottom())], hair());
    for (col, items) in [(0, left), (1, right)] {
        let cx = if col == 0 { x0 } else { mid + 24.0 };
        let mut cy = y + 16.0;
        for (_, (title, rows)) in items {
            sub_label(&p, pos2(cx, cy), title);
            cy += 26.0;
            for (keys, action) in rows.iter() {
                p.text(pos2(cx, cy), Align2::LEFT_TOP, *keys, mono(12.0), INK);
                p.text(
                    pos2(cx + col_w, cy),
                    Align2::RIGHT_TOP,
                    *action,
                    mono(12.0),
                    INK,
                );
                cy += row_h;
            }
            cy += 18.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_ratios() {
        assert_eq!(aspect(4032, 3024), "4:3");
        assert_eq!(aspect(1920, 1080), "16:9");
        assert_eq!(aspect(1000, 333), "3.00:1");
    }
}
