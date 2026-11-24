// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod core;
mod platform;
mod ui;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    let initial = std::env::args_os().nth(1).map(PathBuf::from);

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
        // The screenshot helper must not restore a saved window size.
        persist_window: std::env::var_os("MUUD_SCREENSHOT").is_none(),
        ..Default::default()
    };
    eframe::run_native(
        "muud-view",
        options,
        Box::new(move |cc| Ok(Box::new(app::MuudApp::new(cc, initial)))),
    )
}
