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
use crate::core::settings::{Background, Settings};
use crate::core::view::{View, ZoomMode};
use crate::ui::theme::{apply_style, install_fonts, CANVAS_GRAY, CANVAS_INK, INK, PAPER};

pub const CACHE_BUDGET: usize = 1024 * 1024 * 1024;
pub const TOP_BAR: f32 = 40.0;
pub const INFO_W: f32 = 340.0;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Debug/README helper: `MUUD_SCREENSHOT=out.png` saves the window and exits.
struct ScreenshotJob {
    path: PathBuf,
    frames: u32,
    requested: bool,
}

pub struct MuudApp {
    pub(crate) settings: Settings,
    pub(crate) lib: Library,
    pub(crate) textures: HashMap<PathBuf, Vec<TextureHandle>>,
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
    title: String,
    started: bool,
    pending_open: Option<PathBuf>,
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
        let show = std::env::var("MUUD_SHOW").ok();
        if let Some(show) = &show {
            settings.show_info = show.contains("info");
            settings.pin_bottom = show.contains("bottom");
        }
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(3, 8)
            - 1;
        let screenshot = std::env::var_os("MUUD_SCREENSHOT").map(|p| ScreenshotJob {
            path: PathBuf::from(p),
            frames: 0,
            requested: false,
        });
        Self {
            settings,
            lib: Library::new(Loader::new(cc.egui_ctx.clone(), threads), CACHE_BUDGET),
            textures: HashMap::new(),
            shown: None,
            view: View::default(),
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
            title: String::new(),
            started: false,
            pending_open: initial,
            screenshot,
        }
    }

    // -----------------------------------------------------------------------
    // Files

    pub(crate) fn open(&mut self, path: &Path) {
        match folder::list_for(path) {
            Ok(l) if !l.files.is_empty() => {
                self.last_dir = l.files[l.index].parent().map(Path::to_path_buf);
                self.open_error = None;
                self.lib.set_files(l.files, l.index);
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
        self.lib.clear();
        self.shown = None;
        self.textures.clear();
        self.slideshow = None;
    }

    pub(crate) fn reset_view(&mut self) {
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

    pub(crate) fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    // -----------------------------------------------------------------------
    // Textures

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
        let Some(crate::core::library::Slot::Ready(d)) = self.lib.get(path) else {
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
        let wanted = self.lib.wanted();
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
        // Free GPU memory for images that are no longer near.
        let shown = self.shown.clone();
        self.textures
            .retain(|p, _| wanted.contains(p) || Some(p) == shown.as_ref());
    }

    // -----------------------------------------------------------------------
    // Misc

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
            Background::Paper => INK,
            _ => PAPER,
        }
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.screenshot else {
            return;
        };
        job.frames += 1;
        // Wait until the first image is on screen (or a few seconds pass).
        let ready = self.lib.is_empty() || self.shown.is_some();
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
            self.lib.loader().set_max_texture_side(side);
            self.started = true;
            if let Some(p) = self.pending_open.take() {
                self.open(&p);
            }
        }

        self.lib.poll();
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
        if self.settings.show_info && self.lib.current_decoded().is_some() {
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
