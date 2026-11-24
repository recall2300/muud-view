//! Application state and the per-frame flow. Drawing lives in `crate::ui`,
//! command handling in `commands`.

mod commands;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, vec2, CentralPanel, Color32, Panel, Pos2, Rect, TextureFilter, TextureHandle,
    TextureOptions, ViewportCommand,
};

use crate::core::folder;
use crate::core::library::Library;
use crate::core::loader::Loader;
use crate::core::settings::{Background, Lang, Settings};
use crate::core::view::{View, ZoomMode};
use crate::ui::bottom_bar::{BarState, Edge};
use crate::ui::theme::{apply_style, ink, install_fonts, paper, set_dark, CANVAS_GRAY, CANVAS_INK};

pub const CACHE_BUDGET: usize = 1024 * 1024 * 1024;
pub const TOP_BAR: f32 = 40.0;
pub const BOTTOM_BAR: f32 = 48.0;
pub const INFO_W: f32 = 340.0;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How long after the last key repeat "key held down" mode lasts.
pub const RAPID_GRACE: Duration = Duration::from_millis(200);

/// Two-image comparison: image A is pinned, the current image is B.
pub(crate) struct Compare {
    pub a: PathBuf,
    pub mode: crate::core::compare::CompareMode,
    /// Slider position, 0..1 of the canvas width.
    pub split: f32,
    pub dragging_split: bool,
    /// Difference image for (A decode, B decode).
    pub diff: Option<DiffCache>,
}

pub(crate) struct DiffCache {
    pub a: std::sync::Arc<crate::core::decode::Decoded>,
    pub b: std::sync::Arc<crate::core::decode::Decoded>,
    pub texture: TextureHandle,
    pub stats: crate::core::compare::DiffStats,
}

/// GPU textures for one image, and the decode they came from (so a newer
/// decode of the same file, e.g. full resolution, replaces them).
pub(crate) struct Tex {
    pub src: std::sync::Arc<crate::core::decode::Decoded>,
    pub handles: Vec<TextureHandle>,
    /// For a huge image: each piece and where it sits (texture coordinates).
    pub tiles: Vec<(Rect, TextureHandle)>,
}

/// Debug/README helper: `MUUD_SCREENSHOT=out.png` saves the window and exits.
struct ScreenshotJob {
    path: PathBuf,
    frames: u32,
    requested: bool,
    started: Instant,
}

pub struct MuudApp {
    pub(crate) settings: Settings,
    pub(crate) lib: Library,
    pub(crate) textures: HashMap<PathBuf, Tex>,
    /// The image currently drawn (may lag behind `lib.current()` while the
    /// next one decodes).
    pub(crate) shown: Option<PathBuf>,
    pub(crate) view: View,
    pub(crate) last_scale: f32,
    pub(crate) last_canvas: Rect,
    pub(crate) frame_index: usize,
    pub(crate) next_frame_at: Option<Instant>,
    pub(crate) slideshow: Option<(Duration, Instant)>,
    pub(crate) toast: Option<(String, Instant)>,
    pub(crate) show_help: bool,
    pub(crate) confirm_delete: bool,
    pub(crate) on_top: bool,
    pub(crate) wheel_accum: f32,
    pub(crate) checker: Option<TextureHandle>,
    pub(crate) last_dir: Option<PathBuf>,
    pub(crate) open_error: Option<String>,
    pub(crate) bars: BarState,
    pub(crate) history: crate::core::history::History,
    /// Messages from background jobs (e.g. clipboard writes), shown as toasts.
    pub(crate) jobs: Vec<std::sync::mpsc::Receiver<String>>,
    pub(crate) paste_count: u32,
    pub(crate) show_settings: bool,
    pub(crate) settings_tab: crate::ui::settings_window::Tab,
    pub(crate) keymap: crate::core::keymap::Keymap,
    /// Waiting for a key to assign to this command (Settings → Keys).
    pub(crate) capture: Option<crate::core::keymap::Cmd>,
    pub(crate) show_folders: bool,
    /// Text being edited in the rename sheet.
    pub(crate) rename: Option<String>,
    pub(crate) zoom_locked: bool,
    pub(crate) edge_accum: f32,
    /// A navigation key is held down until this moment.
    pub(crate) rapid_until: Option<Instant>,
    /// Watches the open folder for changes made by other programs.
    watcher: Option<crate::core::watch::Watcher>,
    pub(crate) compare: Option<Compare>,
    start_compare: Option<crate::core::compare::CompareMode>,
    ctx: egui::Context,
    pointer_moved_at: Instant,
    bar_visible_top: f32,
    /// How visible the floating bottom bar was last frame (0..1).
    pub(crate) bar_visible_bottom: f32,
    remembered: Option<PathBuf>,
    rng: u64,
    incoming: Option<std::sync::mpsc::Receiver<Option<PathBuf>>>,
    title: String,
    started: bool,
    pending_open: Option<PathBuf>,
    screenshot: Option<ScreenshotJob>,
}

impl MuudApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial: Option<PathBuf>,
        incoming: Option<std::sync::mpsc::Receiver<Option<PathBuf>>>,
        ctx_slot: std::sync::Arc<std::sync::Mutex<Option<egui::Context>>>,
    ) -> Self {
        if let Ok(mut slot) = ctx_slot.lock() {
            *slot = Some(cc.egui_ctx.clone());
        }
        let mut settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        let show = std::env::var("MUUD_SHOW").ok();
        let has = |w: &str| show.as_deref().is_some_and(|s| s.contains(w));
        if show.is_some() {
            settings.show_info = has("info");
            settings.pin_bottom = has("bottom");
            settings.dark = has("dark");
            settings.lang = if has("en") { Lang::En } else { Lang::Ko };
        }
        let show_settings = has("settings");
        // Screenshots of the compare views.
        let start_compare = if has("compare-diff") {
            Some(crate::core::compare::CompareMode::Difference)
        } else if has("compare-slider") {
            Some(crate::core::compare::CompareMode::Slider)
        } else if has("compare") {
            Some(crate::core::compare::CompareMode::SideBySide)
        } else {
            None
        };
        let start_view = View {
            mode: if has("zoom100") {
                ZoomMode::Original
            } else {
                ZoomMode::Fit
            },
            ..View::default()
        };
        let settings_tab = if has("keys") {
            crate::ui::settings_window::Tab::Keys
        } else {
            Default::default()
        };
        crate::ui::i18n::set_korean(settings.lang == Lang::Ko);
        set_dark(settings.dark);
        install_fonts(&cc.egui_ctx);
        apply_style(&cc.egui_ctx);
        let keymap = crate::core::keymap::Keymap::with_overrides(&settings.key_overrides);
        // One decoder per core; reading runs on its own thread.
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 16);
        let screenshot = std::env::var_os("MUUD_SCREENSHOT").map(|p| ScreenshotJob {
            path: PathBuf::from(p),
            frames: 0,
            requested: false,
            started: Instant::now(),
        });
        Self {
            settings,
            lib: Library::new(Loader::new(cc.egui_ctx.clone(), threads), CACHE_BUDGET),
            textures: HashMap::new(),
            shown: None,
            view: start_view,
            last_scale: 1.0,
            last_canvas: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
            frame_index: 0,
            next_frame_at: None,
            slideshow: None,
            toast: None,
            show_help: show.as_deref().is_some_and(|s| s.contains("help")),
            confirm_delete: false,
            on_top: false,
            wheel_accum: 0.0,
            checker: None,
            last_dir: None,
            open_error: None,
            bars: BarState::default(),
            history: Default::default(),
            jobs: Vec::new(),
            paste_count: 0,
            show_settings,
            settings_tab,
            keymap,
            capture: None,
            show_folders: has("folders"),
            rename: None,
            zoom_locked: false,
            edge_accum: 0.0,
            rapid_until: None,
            watcher: None,
            compare: None,
            start_compare,
            ctx: cc.egui_ctx.clone(),
            pointer_moved_at: Instant::now(),
            bar_visible_top: 0.0,
            bar_visible_bottom: 0.0,
            remembered: None,
            rng: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1)
                | 1,
            incoming,
            title: String::new(),
            started: false,
            pending_open: initial,
            screenshot,
        }
    }

    // -----------------------------------------------------------------------
    // Files

    pub(crate) fn open(&mut self, path: &Path) {
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

    pub(crate) fn close_file(&mut self) {
        self.watcher = None;
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

    /// Runs `job` on a thread; its returned message becomes a toast.
    pub(crate) fn spawn_job(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce() -> String + Send + 'static,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
        self.jobs.push(rx);
    }

    fn poll_jobs(&mut self) {
        let mut done = Vec::new();
        self.jobs.retain(|rx| match rx.try_recv() {
            Ok(msg) => {
                done.push(msg);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        for msg in done {
            self.notify(msg);
        }
    }

    /// Shows an image that came from the clipboard.
    pub(crate) fn show_pasted(&mut self, image: egui::ColorImage) {
        self.paste_count += 1;
        let label = PathBuf::from(format!(
            "clipboard-{}-{}.png",
            chrono::Local::now().format("%H%M%S"),
            self.paste_count
        ));
        let decoded = crate::core::decode::Decoded::from_color_image(image, "CLIPBOARD");
        // Cache first, so the loader never tries to read this non-file.
        self.lib
            .insert_decoded(label.clone(), std::sync::Arc::new(decoded));
        self.lib.set_files(vec![label], 0);
        self.watcher = None;
        self.open_error = None;
        self.reset_view();
    }

    /// Runs a configurable mouse action.
    pub(crate) fn mouse_action(
        &mut self,
        ctx: &egui::Context,
        a: crate::core::settings::MouseAction,
    ) {
        use crate::core::keymap::Cmd;
        use crate::core::settings::MouseAction as M;
        let cmd = match a {
            M::None => return,
            M::Fullscreen => Cmd::Fullscreen,
            M::Maximize => Cmd::Maximize,
            M::Original => Cmd::Original,
            M::FitWindow => Cmd::FitWindow,
        };
        self.run(ctx, cmd);
    }

    pub(crate) fn notify(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.contains("FAILED") || text.starts_with("CANNOT") {
            crate::core::log::error(&text);
        }
        self.toast = Some((text, Instant::now()));
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
    fn check_compare(&mut self) {
        let gone = self
            .compare
            .as_ref()
            .is_some_and(|c| !self.lib.files.contains(&c.a));
        if gone {
            self.end_compare();
            self.notify("COMPARE OFF");
        }
    }

    /// Folder changes made by other programs.
    fn poll_watcher(&mut self) {
        use crate::core::library::Applied;
        let Some(w) = &self.watcher else {
            return;
        };
        let changes = w.poll();
        for change in changes {
            let added = change.added.len();
            match self
                .lib
                .apply_change(&change, self.settings.sort, self.settings.sort_desc)
            {
                Applied::Nothing => {}
                Applied::Emptied => {
                    self.close_file();
                    self.notify("NO IMAGES IN THIS FOLDER");
                    return;
                }
                Applied::CurrentRemoved => {
                    self.reset_view();
                    self.notify("FILE REMOVED");
                }
                Applied::Kept | Applied::CurrentReloaded if added > 0 => {
                    self.notify(format!("NEW IMAGES · {added}"));
                }
                Applied::Kept | Applied::CurrentReloaded => {}
            }
        }
    }

    /// Files sent by a second launch ("Open with" while a window is open).
    fn poll_incoming(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.incoming else {
            return;
        };
        let requests: Vec<Option<PathBuf>> = rx.try_iter().collect();
        for req in requests {
            if let Some(p) = req {
                self.open(&p);
            }
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
    }

    // -----------------------------------------------------------------------
    // Textures

    pub(crate) fn texture_options(&self) -> TextureOptions {
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

    /// Uploads the best decode of `path`, replacing older textures of it.
    /// Returns true if anything was uploaded.
    fn upload(&mut self, ctx: &egui::Context, path: &Path) -> bool {
        let Some(d) = self.lib.best(path).cloned() else {
            return false;
        };
        let opts = self.texture_options();
        let name = path.display().to_string();
        if let Some(t) = self.textures.get_mut(path) {
            if std::sync::Arc::ptr_eq(&t.src, &d) {
                return false;
            }
            // More frames of an animation that is still decoding: upload
            // only the new ones.
            if continues(&t.src, &d) {
                for f in &d.frames[t.handles.len()..] {
                    t.handles
                        .push(ctx.load_texture(name.clone(), f.image.clone(), opts));
                }
                t.src = d;
                return true;
            }
        }
        let handles = d
            .frames
            .iter()
            .map(|f| ctx.load_texture(name.clone(), f.image.clone(), opts))
            .collect();
        let [fw, fh] = d.texture_size();
        let tiles = d
            .tiles
            .iter()
            .map(|t| {
                let [tw, th] = t.image.size;
                let part = Rect::from_min_size(
                    Pos2::new(t.x as f32 / fw as f32, t.y as f32 / fh as f32),
                    vec2(tw as f32 / fw as f32, th as f32 / fh as f32),
                );
                (part, ctx.load_texture(name.clone(), t.image.clone(), opts))
            })
            .collect();
        self.textures.insert(
            path.to_path_buf(),
            Tex {
                src: d,
                handles,
                tiles,
            },
        );
        true
    }

    fn ensure_textures(&mut self, ctx: &egui::Context) {
        let wanted = self.lib.wanted();
        // The current image, or while it decodes the closest one that is
        // ready (so holding a key shows the images it flies past).
        if let Some(target) = self.lib.nearest_ready(self.shown.as_deref()) {
            self.upload(ctx, &target);
            if self.shown.as_ref() != Some(&target) {
                self.shown = Some(target);
                self.frame_index = 0;
                self.next_frame_at = None;
            }
        }
        // Pre-upload one neighbour per frame so switching is instant.
        for p in wanted.iter().skip(1) {
            if !self.textures.contains_key(p) && self.upload(ctx, p) {
                ctx.request_repaint();
                break;
            }
        }
        // Free GPU memory for images that are no longer near.
        let shown = self.shown.clone();
        self.textures
            .retain(|p, _| wanted.contains(p) || Some(p) == shown.as_ref());
    }

    /// Ends "key held down" mode a moment after the last key repeat.
    fn update_rapid(&mut self, ctx: &egui::Context) {
        if let Some(until) = self.rapid_until {
            let now = Instant::now();
            if now >= until {
                self.rapid_until = None;
                self.lib.set_rapid(false);
            } else {
                ctx.request_repaint_after(until - now);
            }
        }
    }

    /// Tells the loader how big "screen size" is: the monitor, so resizing
    /// the window or going fullscreen does not need a new decode.
    fn update_screen_size(&self, ctx: &egui::Context) {
        let (monitor, content, ppp) = ctx.input(|i| {
            (
                i.viewport().monitor_size,
                i.content_rect().size(),
                i.pixels_per_point(),
            )
        });
        let size = monitor
            .filter(|m| m.x > 0.0 && m.y > 0.0)
            .unwrap_or(content)
            .max(content);
        self.lib.loader().set_screen_size([
            (size.x * ppp).round() as usize,
            (size.y * ppp).round() as usize,
        ]);
    }

    // -----------------------------------------------------------------------
    // Misc

    fn remember_position(&mut self) {
        let Some(cur) = self.lib.current() else {
            return;
        };
        if self.remembered.as_ref() == Some(cur) || !cur.exists() {
            return;
        }
        let cur = cur.clone();
        if let Some(folder) = cur.parent() {
            self.settings
                .remember_position(&folder.display().to_string(), &folder::file_name(&cur));
        }
        self.remembered = Some(cur);
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.lib.current() {
            Some(p) => format!("{} — muud view", folder::file_name(p)),
            None => "muud view".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    pub(crate) fn canvas_color(&self) -> Color32 {
        match self.settings.background {
            Background::Ink => CANVAS_INK,
            Background::Paper => Color32::from_rgb(0xE6, 0xE4, 0xDE),
            Background::Gray => CANVAS_GRAY,
        }
    }

    pub(crate) fn on_canvas_color(&self) -> Color32 {
        match self.settings.background {
            Background::Paper => ink(),
            _ => paper(),
        }
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.screenshot else {
            return;
        };
        job.frames += 1;
        // Wait until the first image is on screen (or a few seconds pass).
        let compare_ready = match &self.compare {
            Some(c) => {
                self.textures.contains_key(&c.a)
                    && self
                        .lib
                        .current()
                        .is_some_and(|b| self.textures.contains_key(b))
            }
            None => self.start_compare.is_none(),
        };
        let ready = self.lib.is_empty()
            || (self.shown.is_some() && !self.lib.full_pending() && compare_ready);
        let timed_out = job.started.elapsed() > Duration::from_secs(60);
        if !job.requested && ((ready && job.frames > 20) || timed_out) {
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

/// True if `new` is `old` with more frames added (a streaming animation).
fn continues(old: &crate::core::decode::Decoded, new: &crate::core::decode::Decoded) -> bool {
    !old.complete
        && old.tiles.is_empty()
        && new.tiles.is_empty()
        && !old.frames.is_empty()
        && old.frames.len() <= new.frames.len()
        && old
            .frames
            .iter()
            .zip(&new.frames)
            .all(|(a, b)| std::sync::Arc::ptr_eq(&a.image, &b.image))
}

impl eframe::App for MuudApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.started {
            // The real texture limit is only known once the renderer runs.
            let side = ctx.input(|i| i.max_texture_side).clamp(2048, 16384);
            self.lib.loader().set_max_texture_side(side);
            self.lib
                .loader()
                .set_color_management(self.settings.color_manage);
            self.update_screen_size(&ctx);
            self.started = true;
            if let Some(p) = self.pending_open.take() {
                self.open(&p);
            }
        }

        self.update_screen_size(&ctx);
        self.lib.poll();
        self.poll_jobs();
        self.poll_watcher();
        self.check_compare();
        if self.shown.is_some() && self.compare.is_none() {
            if let Some(mode) = self.start_compare.take() {
                self.start_compare();
                if let Some(c) = &mut self.compare {
                    c.mode = mode;
                }
            }
        }
        self.poll_incoming(&ctx);
        self.handle_drops(&ctx);
        self.handle_keys(&ctx);
        self.tick(&ctx);
        self.update_rapid(&ctx);
        self.ensure_textures(&ctx);
        self.update_title(&ctx);
        self.remember_position();

        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let empty = self.lib.is_empty();
        // In fullscreen both bars only fade in on demand.
        let pin_top = !fullscreen && (self.settings.pin_top || empty);
        let pin_bottom = !fullscreen && !empty && self.settings.pin_bottom;
        if pin_top {
            Panel::top("top")
                .exact_size(TOP_BAR)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(paper()))
                .show(ui, |ui| {
                    let r = ui.max_rect();
                    self.top_bar(ui, r)
                });
        }
        if pin_bottom {
            Panel::bottom("bottom")
                .exact_size(BOTTOM_BAR)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(paper()))
                .show(ui, |ui| {
                    let r = ui.max_rect();
                    self.bottom_bar(ui, r)
                });
        }
        if self.settings.show_info && self.lib.current_decoded().is_some() {
            Panel::right("info")
                .exact_size(INFO_W)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(paper()))
                .show(ui, |ui| self.info_panel(ui));
        }
        let bg = self.canvas_color();
        CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(bg))
            .show(ui, |ui| self.canvas(ui));

        self.bar_visible_top = 0.0;
        if !pin_top {
            let a = self.bar_opacity(&ctx, Edge::Top);
            self.bar_visible_top = a;
            if a > 0.01 {
                self.floating_bar(&ctx, Edge::Top, a);
            }
        }
        self.bar_visible_bottom = 0.0;
        if !pin_bottom && !empty {
            let a = self.bar_opacity(&ctx, Edge::Bottom);
            self.bar_visible_bottom = a;
            if a > 0.01 {
                self.floating_bar(&ctx, Edge::Bottom, a);
            } else {
                let p = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Middle,
                    egui::Id::unique("progress-line"),
                ));
                self.progress_line(&p, self.last_canvas);
            }
        }

        // File name over the image when the top bar is out of the way.
        let top_hidden = fullscreen || (!pin_top && self.bar_visible_top < 0.01);
        if top_hidden && !empty {
            let p = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::unique("osd"),
            ));
            self.osd(&p, self.last_canvas);
        }
        // Hide the cursor in fullscreen after a moment without movement.
        if ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO || i.pointer.any_down()) {
            self.pointer_moved_at = Instant::now();
        }
        if fullscreen && self.settings.cursor_autohide && !self.show_settings {
            let idle = self.pointer_moved_at.elapsed();
            if idle > Duration::from_millis(1500) {
                ctx.set_cursor_icon(egui::CursorIcon::None);
            } else {
                ctx.request_repaint_after(Duration::from_millis(1600) - idle);
            }
        }

        self.overlays(&ctx);
        if self.show_settings {
            self.settings_window(&ctx);
        }
        self.screenshot(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.canvas_color().to_normalized_gamma_f32()
    }
}
