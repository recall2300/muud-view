//! Getting decoded pictures onto the GPU, at the size the screen needs.

use std::path::Path;
use std::time::Instant;

use eframe::egui::{self, vec2, Pos2, Rect, TextureFilter, TextureOptions};

use super::*;

impl MuudApp {
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
    pub(super) fn upload(&mut self, ctx: &egui::Context, path: &Path) -> bool {
        let Some(d) = self.lib.best(path).cloned() else {
            return false;
        };
        // Full-resolution textures keep their mipmaps: between screen size
        // and 100% they are drawn smaller than they are.
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

    pub(super) fn ensure_textures(&mut self, ctx: &egui::Context) {
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
    pub(super) fn update_rapid(&mut self, ctx: &egui::Context) {
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
    pub(crate) fn update_screen_size(&self, ctx: &egui::Context) {
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
        // A webtoon strip is fitted by width only: its images are tall.
        let h = if self.reading == crate::core::reading::ReadingMode::Webtoon {
            1 << 15
        } else {
            (size.y * ppp).round() as usize
        };
        self.lib
            .loader()
            .set_screen_size([(size.x * ppp).round() as usize, h]);
    }
}

/// True if `new` is `old` with more frames added (a streaming animation).
pub(super) fn continues(
    old: &crate::core::decode::Decoded,
    new: &crate::core::decode::Decoded,
) -> bool {
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
