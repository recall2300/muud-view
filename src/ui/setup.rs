//! F5: Windows integration and diagnostics. (Becomes one tab of the full
//! settings window in M8.)

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, Id, Order, Rect, Sense, StrokeKind,
};

use crate::app::{MuudApp, VERSION};
use crate::core::log;
use crate::platform::integration;
use crate::ui::theme::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetupAction {
    Register,
    StartMenu,
    Unregister,
    CopyDiagnostics,
    OpenLogFolder,
}

pub const ACTIONS: &[(SetupAction, &str, &str)] = &[
    (SetupAction::Register, "1", "SET AS IMAGE VIEWER"),
    (SetupAction::StartMenu, "2", "ADD START MENU SHORTCUT"),
    (SetupAction::Unregister, "3", "REMOVE FROM WINDOWS"),
    (SetupAction::CopyDiagnostics, "4", "COPY DIAGNOSTICS"),
    (SetupAction::OpenLogFolder, "5", "OPEN LOG FOLDER"),
];

impl MuudApp {
    pub(crate) fn run_setup(&mut self, ctx: &egui::Context, action: SetupAction) {
        let result = match action {
            SetupAction::Register => integration::register()
                .and_then(|_| integration::open_default_apps())
                .map(|_| "REGISTERED · PICK MUUD VIEW IN DEFAULT APPS".to_string()),
            SetupAction::StartMenu => {
                integration::add_start_menu_shortcut().map(|_| "START MENU SHORTCUT ADDED".into())
            }
            SetupAction::Unregister => {
                integration::unregister().map(|_| "REMOVED FROM WINDOWS".into())
            }
            SetupAction::CopyDiagnostics => {
                ctx.copy_text(self.diagnostics(ctx));
                Ok("DIAGNOSTICS COPIED".into())
            }
            SetupAction::OpenLogFolder => {
                let dir = log::data_dir();
                let target = log::path().unwrap_or(dir);
                crate::platform::show_in_explorer(&target)
                    .map(|_| "OPENED".into())
                    .map_err(|e| e.to_string())
            }
        };
        match result {
            Ok(msg) => {
                log::info(&format!("setup {action:?}: ok"));
                self.notify(msg);
            }
            Err(e) => {
                log::error(&format!("setup {action:?}: {e}"));
                self.notify(format!("FAILED: {e}"));
            }
        }
    }

    pub(crate) fn diagnostics(&self, ctx: &egui::Context) -> String {
        let max_tex = ctx.input(|i| i.max_texture_side);
        let ppp = ctx.pixels_per_point();
        let current = match (self.lib.current(), self.lib.current_decoded()) {
            (Some(p), Some(d)) => format!(
                "{} — {} {}x{} ({} texture {}x{}, decode {} ms)",
                p.display(),
                d.format,
                d.width,
                d.height,
                if d.is_downscaled() {
                    "downscaled"
                } else {
                    "full"
                },
                d.texture_size()[0],
                d.texture_size()[1],
                d.decode_time.as_millis()
            ),
            (Some(p), None) => format!("{} — not decoded", p.display()),
            _ => "none".into(),
        };
        format!(
            "muud view {VERSION}\nOS: {} {}\nCPU threads: {}\nGPU max texture: {max_tex}\nScale: {ppp:.2}\nData folder: {}\nPortable: {}\nRegistered: {}\nCurrent: {current}\n\n--- log (last 30 lines) ---\n{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
            log::data_dir().display(),
            integration::portable_data_dir().is_some(),
            integration::is_registered(),
            log::tail(30)
        )
    }

    pub(crate) fn setup_sheet(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let w = 520.0f32.min(screen.width() - 32.0);
        let h = 404.0;
        let card = Rect::from_center_size(screen.center(), vec2(w, h));
        let mut chosen = None;
        egui::Area::new(Id::unique("setup-sheet"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (_, backdrop) = ui.allocate_exact_size(screen.size(), Sense::click());
                let p = ui.painter();
                p.rect_filled(screen, CornerRadius::ZERO, INK.gamma_multiply(0.75));
                p.rect_filled(card, CornerRadius::ZERO, PAPER);
                p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
                let x0 = card.left() + 24.0;
                let r = p.text(
                    pos2(x0, card.top() + 20.0),
                    Align2::LEFT_TOP,
                    "SETUP",
                    grotesk(32.0),
                    INK,
                );
                let mut x = r.right() + 14.0;
                let registered = integration::is_registered();
                let portable = integration::portable_data_dir().is_some();
                for t in [
                    format!("v{VERSION}"),
                    if registered {
                        "REGISTERED".into()
                    } else {
                        "NOT REGISTERED".into()
                    },
                    if portable {
                        "PORTABLE".into()
                    } else {
                        "INSTALLED".into()
                    },
                ] {
                    x += pill(p, pos2(x, r.center().y), &t, INK) + 6.0;
                }
                let mut y = r.bottom() + 16.0;
                p.line_segment([pos2(card.left(), y), pos2(card.right(), y)], hair());
                y += 16.0;
                sub_label(p, pos2(x0, y), "WINDOWS");
                y += 26.0;
                for (i, (action, key, label)) in ACTIONS.iter().enumerate() {
                    if i == 3 {
                        y += 10.0;
                        sub_label(ui.painter(), pos2(x0, y), "SUPPORT");
                        y += 26.0;
                    }
                    let row = Rect::from_min_size(
                        pos2(card.left() + 12.0, y - 4.0),
                        vec2(w - 24.0, 28.0),
                    );
                    let resp = ui.interact(row, Id::unique(("setup-row", *key)), Sense::click());
                    let p = ui.painter();
                    let (bg, fg) = if resp.hovered() {
                        (INK, PAPER)
                    } else {
                        (Color32::TRANSPARENT, INK)
                    };
                    p.rect_filled(row, CornerRadius::ZERO, bg);
                    p.text(
                        pos2(x0, y + 10.0),
                        Align2::LEFT_CENTER,
                        *key,
                        mono_medium(12.5),
                        fg,
                    );
                    p.text(
                        pos2(x0 + 34.0, y + 10.0),
                        Align2::LEFT_CENTER,
                        *label,
                        mono(12.5),
                        fg,
                    );
                    if resp.clicked() {
                        chosen = Some(*action);
                    }
                    y += 30.0;
                }
                let p = ui.painter();
                dotted_rule(
                    p,
                    pos2(x0, card.bottom() - 52.0),
                    pos2(card.right() - 24.0, card.bottom() - 52.0),
                    INK,
                );
                let note = "WINDOWS ASKS YOU TO CONFIRM THE DEFAULT APP · ESC CLOSE";
                p.text(
                    pos2(x0, card.bottom() - 30.0),
                    Align2::LEFT_CENTER,
                    note,
                    mono(11.0),
                    MUTED,
                );
                if backdrop.clicked()
                    && !ui
                        .ctx()
                        .input(|i| i.pointer.hover_pos().is_some_and(|p| card.contains(p)))
                {
                    self.show_setup = false;
                }
            });
        if let Some(a) = chosen {
            self.run_setup(ctx, a);
        }
    }
}
