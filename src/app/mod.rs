//! Application state and the per-frame flow. Drawing lives in `crate::ui`,
//! command handling in `commands`.

mod background;
mod commands;
mod navigation;
mod textures;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, vec2, CentralPanel, Color32, Panel, Pos2, Rect, TextureHandle, ViewportCommand,
};

use crate::core::folder;
use crate::core::library::Library;
use crate::core::loader::Loader;
use crate::core::settings::{Background, Lang, Settings};
use crate::core::view::{View, ZoomMode};
use crate::ui::bottom_bar::{BarState, Edge};
use crate::ui::theme::{apply_style, ink, install_fonts, paper, set_dark, CANVAS_GRAY, CANVAS_INK};

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
    /// Where the image was drawn last frame (for copying the visible area).
    pub(crate) last_dest: Option<Rect>,
    /// A drag out of the window just ended: tell egui the button is up
    /// (the shell's drag loop swallowed the release).
    pub(crate) release_after_drag: Option<Pos2>,
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
    /// In-app update: where it stands (F5 → Windows → 6).
    pub(crate) update: crate::ui::setup::UpdateState,
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
    /// Show the pixel under the cursor (`P`).
    pub(crate) pixel_info: bool,
    /// Two pages / webtoon (`D`, `W`).
    pub(crate) reading: crate::core::reading::ReadingMode,
    /// Webtoon: how far the current image's top is above the canvas top.
    pub(crate) webtoon_offset: f32,
    /// Thumbnail grid (`G`).
    pub(crate) grid: bool,
    pub(crate) grid_cols: usize,
    pub(crate) grid_scroll_to: bool,
    pub(crate) grid_wanted: (usize, usize),
    pub(crate) thumbs: Option<crate::core::thumbs::ThumbLoader>,
    pub(crate) thumb_tex: HashMap<PathBuf, TextureHandle>,
    pub(crate) thumb_failed: std::collections::HashSet<PathBuf>,
    pub(crate) stars: crate::core::stars::Stars,
    /// While showing starred images only: the whole folder.
    pub(crate) all_files: Option<Vec<PathBuf>>,
    /// Screenshots only: pretend the pointer is here (`MUUD_POINTER=x,y`).
    pub(crate) fake_pointer: Option<egui::Pos2>,
    start_compare: Option<crate::core::compare::CompareMode>,
    pub(crate) ctx: egui::Context,
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
        let budgets = crate::core::memory::budgets();
        crate::core::log::info(&format!("memory budgets: {budgets:?}"));
        let screenshot = std::env::var_os("MUUD_SCREENSHOT").map(|p| ScreenshotJob {
            path: PathBuf::from(p),
            frames: 0,
            requested: false,
            started: Instant::now(),
        });
        Self {
            settings,
            lib: Library::new(
                Loader::with_read_budget(cc.egui_ctx.clone(), threads, budgets.read),
                budgets.screen,
            )
            .with_full_budget(budgets.full),
            textures: HashMap::new(),
            shown: None,
            view: start_view,
            last_scale: 1.0,
            last_canvas: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
            last_dest: None,
            release_after_drag: None,
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
            update: crate::ui::setup::UpdateState::Idle,
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
            pixel_info: has("pixel"),
            reading: if has("webtoon") {
                crate::core::reading::ReadingMode::Webtoon
            } else if has("two-page") {
                crate::core::reading::ReadingMode::TwoLtr
            } else {
                Default::default()
            },
            webtoon_offset: 0.0,
            grid: has("grid"),
            grid_cols: 1,
            grid_scroll_to: true,
            grid_wanted: (0, 0),
            thumbs: None,
            thumb_tex: HashMap::new(),
            thumb_failed: Default::default(),
            stars: crate::core::stars::Stars::load(crate::core::log::data_dir().join("stars.txt")),
            all_files: None,
            fake_pointer: std::env::var("MUUD_POINTER").ok().and_then(|v| {
                let (x, y) = v.split_once(',')?;
                Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
            }),
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

    // -----------------------------------------------------------------------
    // Textures

    // -----------------------------------------------------------------------
    // Misc

    fn remember_position(&mut self) {
        let Some(cur) = self.lib.current() else {
            return;
        };
        let real = cur.exists() || crate::core::archive::is_virtual(cur);
        if self.remembered.as_ref() == Some(cur) || !real {
            return;
        }
        let cur = cur.clone();
        // Keyed by the folder or archive, so reopening either resumes.
        if let Some(container) = self.container() {
            self.settings
                .remember_position(&container.display().to_string(), &folder::file_name(&cur));
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
}

impl eframe::App for MuudApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(pos) = self.release_after_drag.take() {
            raw.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }

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
        self.poll_update(&ctx);
        self.poll_watcher();
        self.check_compare();
        // The grid shows thumbnails; the big images wait until it closes.
        self.lib.set_idle(self.grid);
        if let Some(w) = &self.watcher {
            use crate::core::watch::Activity;
            let (focused, minimized) = ctx.input(|i| {
                (
                    i.viewport().focused.unwrap_or(true),
                    i.viewport().minimized.unwrap_or(false),
                )
            });
            w.set_activity(if minimized {
                Activity::Hidden
            } else if focused {
                Activity::Focused
            } else {
                Activity::Background
            });
        }
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
        if self.grid && !self.lib.is_empty() {
            CentralPanel::no_frame()
                .frame(egui::Frame::new().fill(paper()))
                .show(ui, |ui| self.grid_view(ui));
        } else {
            CentralPanel::no_frame()
                .frame(egui::Frame::new().fill(bg))
                .show(ui, |ui| self.canvas(ui));
        }

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
