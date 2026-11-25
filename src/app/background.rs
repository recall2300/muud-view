//! Work that arrives from outside the frame loop: background jobs, folder
//! changes, files opened from Explorer, pasted pictures, screenshots.

use std::path::PathBuf;
use std::time::Duration;

use eframe::egui::{self, ViewportCommand};

use super::*;

impl MuudApp {
    /// Runs `job` on a thread; its returned message becomes a toast.
    pub(crate) fn spawn_job(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce() -> String + Send + 'static,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let msg = crate::core::guard::catch("job", || Ok(job()))
                .unwrap_or_else(|e| format!("FAILED · {e}"));
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
        self.jobs.push(rx);
    }

    pub(super) fn poll_jobs(&mut self) {
        let mut done = Vec::new();
        self.jobs.retain(|rx| match rx.try_recv() {
            Ok(msg) => {
                done.push(msg);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        // An empty message means "done, nothing to say" (e.g. printing,
        // where Windows shows its own dialog).
        for msg in done.into_iter().filter(|m| !m.is_empty()) {
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

    /// Folder changes made by other programs.
    pub(super) fn poll_watcher(&mut self) {
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
    pub(super) fn poll_incoming(&mut self, ctx: &egui::Context) {
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

    pub(super) fn screenshot(&mut self, ctx: &egui::Context) {
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
            || self.grid
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
