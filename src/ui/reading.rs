//! Drawing the reading modes: two pages side by side, and the webtoon strip.

use eframe::egui::{self, pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Vec2};

use crate::app::MuudApp;
use crate::core::reading::{scroll_strip, spread, ReadingMode};
use crate::core::view::{compose_uvs, corners};
use crate::ui::theme::*;

impl MuudApp {
    /// Draws one image's first frame (or the current animation frame for
    /// the current image) into `at`, EXIF orientation applied.
    fn draw_page(&self, p: &egui::Painter, path: &std::path::PathBuf, at: Rect) -> bool {
        let Some(tex) = self.textures.get(path) else {
            return false;
        };
        let current = self.lib.current() == Some(path);
        let frame = if current { self.frame_index } else { 0 };
        let handle = &tex.handles[frame.min(tex.handles.len() - 1)];
        let uvs = compose_uvs(tex.src.orientation, 0, false, false);
        let mut mesh = egui::Mesh::with_texture(handle.id());
        for (pos, uv) in corners(at).into_iter().zip(uvs) {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv,
                color: Color32::WHITE,
            });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        p.add(egui::Shape::mesh(mesh));
        true
    }

    fn page_size(&self, path: &std::path::PathBuf) -> Option<Vec2> {
        self.textures
            .get(path)
            .map(|t| vec2(t.src.width as f32, t.src.height as f32))
    }

    /// Two pages: the current image and the next one.
    pub(crate) fn two_page_view(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter_at(rect);
        let i = self.lib.index;
        let first = self.lib.files[i].clone();
        let second = self.lib.files.get(i + 1).cloned();
        let rtl = self.reading == ReadingMode::TwoRtl;
        let Some(a) = self.page_size(&first) else {
            self.reading_placeholder(&p, rect.center());
            return;
        };
        match second {
            Some(second) => {
                let b = self.page_size(&second).unwrap_or(a);
                let (ra, rb) = spread(rect, a, b, rtl);
                self.draw_page(&p, &first, ra);
                if !self.draw_page(&p, &second, rb) {
                    self.reading_placeholder(&p, rb.center());
                }
            }
            None => {
                // The last page alone, centred.
                let s = (rect.width() / a.x).min(rect.height() / a.y);
                self.draw_page(&p, &first, Rect::from_center_size(rect.center(), a * s));
            }
        }
    }

    fn reading_placeholder(&self, p: &egui::Painter, at: Pos2) {
        p.text(
            at,
            Align2::CENTER_CENTER,
            crate::ui::i18n::tr("DECODING …"),
            mono(12.0),
            self.on_canvas_color(),
        );
    }

    /// Width every image of the strip is drawn at: the window width, or the
    /// current image's own width if narrower (no blurry upscaling).
    fn strip_width(&self, rect: Rect, ppp: f32) -> f32 {
        let natural = self
            .lib
            .current()
            .and_then(|c| self.page_size(c))
            .map(|s| s.x / ppp)
            .unwrap_or(rect.width());
        if self.settings.stretch_small {
            rect.width()
        } else {
            rect.width().min(natural)
        }
    }

    fn strip_height(&self, i: usize, width: f32, fallback: f32) -> f32 {
        self.lib
            .files
            .get(i)
            .and_then(|p| self.page_size(p))
            .map(|s| width * s.y / s.x.max(1.0))
            .unwrap_or(fallback)
    }

    /// Scrolls the webtoon strip by `dy` points (positive = down).
    pub(crate) fn webtoon_scroll(&mut self, dy: f32) {
        let rect = self.last_canvas;
        let ppp = self.ctx.pixels_per_point();
        let width = self.strip_width(rect, ppp);
        let len = self.lib.len();
        let (i, off) = scroll_strip(
            self.lib.index,
            self.webtoon_offset,
            dy,
            len,
            rect.height(),
            |k| self.strip_height(k, width, rect.height()),
        );
        self.webtoon_offset = off;
        if i != self.lib.index {
            self.lib.go_to(i);
        }
    }

    /// The webtoon strip: images one under another, scrolled with the
    /// wheel, by dragging or with the navigation keys.
    pub(crate) fn webtoon_view(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let resp = ui.interact(rect, ui.scope_id().with("webtoon"), Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let mut dy = -ctx.input(|i| i.smooth_scroll_delta.y);
        if resp.dragged() {
            dy -= resp.drag_delta().y;
        }
        if resp.hovered() && dy != 0.0 {
            self.webtoon_scroll(dy);
        }
        let p = ui.painter_at(rect);
        let ppp = ctx.pixels_per_point();
        let width = self.strip_width(rect, ppp);
        let x = rect.center().x - width / 2.0;
        // The image above the current one, if it shows.
        let mut y = rect.top() - self.webtoon_offset;
        let i = self.lib.index;
        if i > 0 && y > rect.top() {
            let h = self.strip_height(i - 1, width, rect.height());
            let path = self.lib.files[i - 1].clone();
            self.draw_page(
                &p,
                &path,
                Rect::from_min_size(pos2(x, y - h), vec2(width, h)),
            );
        }
        let mut k = i;
        while k < self.lib.len() && y < rect.bottom() {
            let h = self.strip_height(k, width, rect.height());
            let at = Rect::from_min_size(pos2(x, y), vec2(width, h));
            let path = self.lib.files[k].clone();
            if !self.draw_page(&p, &path, at) {
                self.reading_placeholder(&p, at.center());
            }
            y += h;
            k += 1;
        }
    }
}
