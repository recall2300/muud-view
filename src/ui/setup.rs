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
    CheckUpdates,
}

use crate::core::update::{self, Event, Version};

/// The in-app update, step by step (see `core::update`).
pub enum UpdateState {
    Idle,
    Checking(std::sync::mpsc::Receiver<Event>),
    UpToDate,
    Available(Version),
    /// Downloading and swapping `Version`; percent so far.
    Installing(std::sync::mpsc::Receiver<Event>, Version, Option<u8>),
    /// In place; the next press restarts into it.
    Installed(Version),
}

pub const ACTIONS: &[(SetupAction, &str, &str)] = &[
    (SetupAction::Register, "1", "SET AS IMAGE VIEWER"),
    (SetupAction::StartMenu, "2", "ADD START MENU SHORTCUT"),
    (SetupAction::Unregister, "3", "REMOVE FROM WINDOWS"),
    (SetupAction::CopyDiagnostics, "4", "COPY DIAGNOSTICS"),
    (SetupAction::OpenLogFolder, "5", "OPEN LOG FOLDER"),
    (SetupAction::CheckUpdates, "6", "CHECK FOR UPDATES"),
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
            SetupAction::CheckUpdates => {
                self.update_step(ctx);
                return;
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

    /// One press of "check for updates": check, then install, then restart.
    fn update_step(&mut self, ctx: &egui::Context) {
        let exe = std::env::current_exe();
        match &self.update {
            UpdateState::Idle | UpdateState::UpToDate => {
                log::info("update: checking");
                self.update = UpdateState::Checking(update::check());
            }
            UpdateState::Available(v) => {
                let v = *v;
                match exe {
                    Ok(exe) => {
                        log::info(&format!("update: installing {}", v.tag()));
                        self.update = UpdateState::Installing(update::install(v, exe), v, None);
                    }
                    Err(e) => self.notify(format!("UPDATE FAILED · {e}")),
                }
            }
            UpdateState::Installed(_) => {
                let current = self.lib.current().map(|p| p.to_path_buf());
                match exe
                    .map_err(|e| e.to_string())
                    .and_then(|exe| update::restart(&exe, current.as_deref()))
                {
                    Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(e) => self.notify(format!("RESTART FAILED · {e}")),
                }
            }
            UpdateState::Checking(_) | UpdateState::Installing(..) => {}
        }
        ctx.request_repaint();
    }

    /// Picks up what the update thread reports.
    pub(crate) fn poll_update(&mut self, ctx: &egui::Context) {
        let rx = match &self.update {
            UpdateState::Checking(rx) | UpdateState::Installing(rx, ..) => rx,
            _ => return,
        };
        let mut last = None;
        let mut percent = None;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                Event::Progress(n, Some(total)) if total > 0 => {
                    percent = Some((n * 100 / total).min(100) as u8);
                }
                Event::Progress(..) => {}
                other => last = Some(other),
            }
        }
        if let (Some(p), UpdateState::Installing(_, _, shown)) = (percent, &mut self.update) {
            *shown = Some(p);
        }
        // Keep the label moving while the thread works.
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        let Some(ev) = last else { return };
        log::info(&format!("update: {ev:?}"));
        match ev {
            Event::UpToDate(_) => {
                self.notify(format!("UP TO DATE · v{VERSION}"));
                self.update = UpdateState::UpToDate;
            }
            Event::Available(v) => {
                self.notify(format!("UPDATE AVAILABLE · {}", v.tag()));
                self.update = UpdateState::Available(v);
            }
            Event::Installed(v) => {
                self.notify(format!("UPDATED · {} · RESTART TO FINISH", v.tag()));
                self.update = UpdateState::Installed(v);
            }
            Event::Failed(e) => {
                self.notify(format!("UPDATE FAILED · {e}"));
                self.update = match &self.update {
                    // Keep the offer, so it can be tried again.
                    UpdateState::Installing(_, v, _) => UpdateState::Available(*v),
                    _ => UpdateState::Idle,
                };
            }
            Event::Progress(..) => {}
        }
    }

    /// The label of the update row for where the update stands.
    pub(crate) fn update_label(&self) -> String {
        use crate::ui::i18n::tr;
        match &self.update {
            UpdateState::Idle => tr("CHECK FOR UPDATES").to_string(),
            UpdateState::Checking(_) => tr("CHECKING FOR UPDATES…").to_string(),
            UpdateState::UpToDate => format!("{} · v{VERSION}", tr("UP TO DATE")),
            UpdateState::Available(v) => format!("{} · {}", tr("INSTALL UPDATE"), v.tag()),
            UpdateState::Installing(_, _, p) => match p {
                Some(p) => format!("{} · {p}%", tr("DOWNLOADING")),
                None => tr("DOWNLOADING").to_string(),
            },
            UpdateState::Installed(v) => format!("{} · {}", tr("RESTART TO FINISH"), v.tag()),
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
                if d.reduced {
                    "screen-size"
                } else if d.is_downscaled() {
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
        let loader = self.lib.loader();
        let (screen_budget, full_budget) = self.lib.budgets();
        let ram = crate::core::memory::installed_ram()
            .map_or("unknown".into(), |b| crate::core::memory::human(b as usize));
        let pipeline = format!(
            "{} decoders, texture limit {}, {} files read ahead, full-res current: {}, key held: {}\nMemory: {ram} RAM; caches {} screen-size, {} full-res, {} read-ahead",
            loader.decoders(),
            loader.max_texture_side(),
            loader.read_cache_len(),
            self.lib.current().is_some_and(|p| self.lib.has_full(p)),
            self.lib.is_rapid(),
            crate::core::memory::human(screen_budget),
            crate::core::memory::human(full_budget),
            crate::core::memory::human(loader.read_budget()),
        );
        format!(
            "muud view {VERSION}\nOS: {} {}\nCPU threads: {}\nGPU max texture: {max_tex}\nScale: {ppp:.2}\nPipeline: {pipeline}\nData folder: {}\nPortable: {}\nRegistered: {}\nCurrent: {current}\n\n--- log (last 30 lines) ---\n{}",
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
