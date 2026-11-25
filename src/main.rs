// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod core;
mod platform;
mod ui;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    let data_dir = core::log::data_dir();
    core::log::init(&data_dir);

    // Command-line setup for scripts and IT deployment.
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if let Some(flag) = args.first().and_then(|a| a.to_str()).map(str::to_owned) {
        let result = match flag.as_str() {
            "--register" => Some(platform::integration::register()),
            "--unregister" => Some(platform::integration::unregister()),
            _ => None,
        };
        if let Some(r) = result {
            if let Err(e) = &r {
                core::log::error(&format!("{flag}: {e}"));
            }
            std::process::exit(if r.is_ok() { 0 } else { 1 });
        }
    }
    // Started by an in-app update: let the old version close first (it
    // holds the single-instance pipe), then remove its file.
    if args.first().and_then(|a| a.to_str()) == Some("--after-update") {
        let pid = args
            .get(1)
            .and_then(|a| a.to_str())
            .and_then(|s| s.parse().ok());
        args.drain(..args.len().min(2));
        if let Some(pid) = pid {
            platform::wait_for_exit(pid, std::time::Duration::from_secs(15));
        }
        core::log::info("started after an update");
    }
    if let Ok(exe) = std::env::current_exe() {
        core::update::clean_up(&exe);
    }
    let initial = (!args.is_empty()).then(|| PathBuf::from(args.remove(0)));
    let initial = initial.map(|p| std::fs::canonicalize(&p).unwrap_or(p));

    // One window: hand the file to a running viewer if there is one.
    let ctx_slot: std::sync::Arc<std::sync::Mutex<Option<egui::Context>>> = Default::default();
    let incoming = if std::env::var_os("MUUD_SCREENSHOT").is_some()
        || std::env::var_os("MUUD_MULTI").is_some()
    {
        None
    } else {
        let slot = ctx_slot.clone();
        let wake = Box::new(move || {
            if let Some(ctx) = slot.lock().ok().and_then(|s| s.clone()) {
                ctx.request_repaint();
            }
        });
        match platform::single_instance::start(initial.as_deref(), wake) {
            platform::single_instance::Role::Handed => return Ok(()),
            platform::single_instance::Role::Primary(rx) => rx,
        }
    };

    // MUUD_SIZE=WxH sets the window size (used for README screenshots).
    let size = std::env::var("MUUD_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some([w.parse().ok()?, h.parse().ok()?])
        })
        .unwrap_or([1280.0, 820.0]);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("muud view")
        .with_app_id("muud-view")
        .with_inner_size(size)
        .with_min_inner_size([480.0, 320.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = image::load_from_memory(include_bytes!("../assets/icon-256.png")) {
        let icon = icon.into_rgba8();
        viewport = viewport.with_icon(egui::IconData {
            width: icon.width(),
            height: icon.height(),
            rgba: icon.into_raw(),
        });
    }

    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        // Ctrl+C / Ctrl+Insert / Shift+Insert / Shift+Delete are viewer
        // shortcuts here, not text-editing clipboard commands.
        clipboard_shortcuts: false,
        // The screenshot helper must not restore a saved window size.
        persist_window: std::env::var_os("MUUD_SCREENSHOT").is_none(),
        persistence_path: platform::integration::portable_data_dir()
            .map(|d| d.join("settings.ron")),
        ..Default::default()
    };
    eframe::run_native(
        "muud-view",
        options,
        Box::new(move |cc| Ok(Box::new(app::MuudApp::new(cc, initial, incoming, ctx_slot)))),
    )
}
