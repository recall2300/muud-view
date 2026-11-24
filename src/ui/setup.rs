//! F5: Windows integration and diagnostics. (Becomes one tab of the full
//! settings window in M8.)

use eframe::egui;

use crate::app::{MuudApp, VERSION};
use crate::core::log;
use crate::platform::integration;

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
}
