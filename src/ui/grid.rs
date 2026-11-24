//! Thumbnail grid (`G`): the whole folder at a glance. Arrow keys move,
//! Enter or a click opens, `S` stars, Esc goes back.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
};

use crate::app::MuudApp;
use crate::core::folder;
use crate::ui::theme::*;

/// Cell size (points) including the name below the thumbnail.
pub const CELL: f32 = 208.0;
const NAME_H: f32 = 20.0;
const GAP: f32 = 8.0;
/// Thumbnails kept around the visible rows (each is at most 256 KB).
const KEEP: usize = 300;

/// Columns that fit in `width`.
pub fn columns(width: f32) -> usize {
    ((width - GAP) / (CELL + GAP)).floor().max(1.0) as usize
}

/// Index after moving `dx` columns / `dy` rows from `index`, clamped.
pub fn moved(index: usize, len: usize, cols: usize, dx: i64, dy: i64) -> usize {
    if len == 0 {
        return 0;
    }
    let target = index as i64 + dx + dy * cols as i64;
    target.clamp(0, len as i64 - 1) as usize
}

/// A five-pointed star polygon centred at `c`.
pub fn star_points(c: Pos2, r: f32) -> Vec<Pos2> {
    (0..10)
        .map(|i| {
            let a = std::f32::consts::PI * (i as f32 / 5.0) - std::f32::consts::FRAC_PI_2;
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            pos2(c.x + rr * a.cos(), c.y + rr * a.sin())
        })
        .collect()
}

impl MuudApp {
    pub(crate) fn grid_view(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();
        let cols = columns(rect.width());
        self.grid_cols = cols;
        let len = self.lib.len();
        let rows = len.div_ceil(cols);
        let current = self.lib.index;
        let scroll_to = std::mem::take(&mut self.grid_scroll_to);

        self.poll_thumbs(&ctx);
        let mut open: Option<usize> = None;
        let mut visible = (0usize, 0usize);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let total = vec2(rect.width(), rows as f32 * (CELL + GAP) + GAP);
                let (area, resp) = ui.allocate_exact_size(total, Sense::click());
                let p = ui.painter();
                let origin = area.min
                    + vec2(
                        ((rect.width() - cols as f32 * (CELL + GAP) + GAP) / 2.0).max(GAP),
                        GAP,
                    );
                let first_row = ((viewport.min.y - GAP) / (CELL + GAP)).floor().max(0.0) as usize;
                let last_row = (((viewport.max.y) / (CELL + GAP)).ceil() as usize).min(rows);
                visible = (first_row * cols, (last_row * cols).min(len));
                let cell_rect = |i: usize| {
                    let (r, c) = (i / cols, i % cols);
                    Rect::from_min_size(
                        origin + vec2(c as f32 * (CELL + GAP), r as f32 * (CELL + GAP)),
                        vec2(CELL, CELL),
                    )
                };
                if scroll_to && current < len {
                    ui.scroll_to_rect(cell_rect(current), None);
                }
                let hover = resp.hover_pos();
                for i in visible.0..visible.1 {
                    let cell = cell_rect(i);
                    let path = &self.lib.files[i];
                    let hovered = hover.is_some_and(|h| cell.contains(h));
                    p.rect_filled(
                        cell,
                        CornerRadius::ZERO,
                        if hovered {
                            muted().gamma_multiply(0.25)
                        } else {
                            paper()
                        },
                    );
                    let pic = Rect::from_min_max(
                        cell.min + vec2(8.0, 8.0),
                        cell.max - vec2(8.0, NAME_H + 4.0),
                    );
                    match self.thumb_tex.get(path) {
                        Some(t) => {
                            let [w, h] = t.size();
                            let s = (pic.width() / w as f32).min(pic.height() / h as f32);
                            let r =
                                Rect::from_center_size(pic.center(), vec2(w as f32, h as f32) * s);
                            p.image(
                                t.id(),
                                r,
                                Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }
                        None => {
                            let label = if self.thumb_failed.contains(path) {
                                "—"
                            } else {
                                "…"
                            };
                            p.text(
                                pic.center(),
                                Align2::CENTER_CENTER,
                                label,
                                mono(12.0),
                                ink().gamma_multiply(0.5),
                            );
                        }
                    }
                    let name = ellipsize(p, &folder::file_name(path), mono(10.0), CELL - 12.0);
                    p.text(
                        pos2(cell.center().x, cell.max.y - NAME_H / 2.0 - 2.0),
                        Align2::CENTER_CENTER,
                        name,
                        mono(10.0),
                        ink(),
                    );
                    if self.stars.contains(path) {
                        let c = pos2(cell.max.x - 18.0, cell.min.y + 18.0);
                        p.add(Shape::convex_polygon(
                            star_points(c, 10.0),
                            Color32::from_rgb(255, 196, 0),
                            Stroke::new(1.0, Color32::BLACK),
                        ));
                    }
                    if i == current {
                        p.rect_stroke(
                            cell,
                            CornerRadius::ZERO,
                            Stroke::new(2.0, ink()),
                            StrokeKind::Inside,
                        );
                    }
                }
                if resp.clicked() {
                    if let Some(h) = resp.interact_pointer_pos() {
                        open = (visible.0..visible.1).find(|&i| cell_rect(i).contains(h));
                    }
                }
            });
        if let Some(i) = open {
            self.go_to(i);
            self.grid = false;
        }
        self.want_thumbs(visible);
    }

    /// Asks for the visible thumbnails (then one screen around them) and
    /// forgets far-away ones.
    fn want_thumbs(&mut self, (first, last): (usize, usize)) {
        if self.grid_wanted == (first, last) {
            return;
        }
        self.grid_wanted = (first, last);
        let len = self.lib.len();
        let span = last.saturating_sub(first);
        let around = (first.saturating_sub(span)..first)
            .rev()
            .chain(last..(last + span).min(len));
        let paths: Vec<_> = (first..last)
            .chain(around)
            .map(|i| self.lib.files[i].clone())
            .filter(|p| !self.thumb_tex.contains_key(p) && !self.thumb_failed.contains(p))
            .collect();
        let threads = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .clamp(1, 4);
        let ctx = self.ctx.clone();
        self.thumbs
            .get_or_insert_with(|| crate::core::thumbs::ThumbLoader::new(ctx, threads))
            .want(paths);
        // Keep memory bounded.
        if self.thumb_tex.len() > KEEP * 2 {
            let keep: std::collections::HashSet<_> = (first.saturating_sub(KEEP)
                ..(last + KEEP).min(len))
                .map(|i| self.lib.files[i].clone())
                .collect();
            self.thumb_tex.retain(|p, _| keep.contains(p));
        }
    }

    fn poll_thumbs(&mut self, ctx: &egui::Context) {
        let Some(t) = &self.thumbs else { return };
        for (path, result) in t.poll() {
            match result {
                Ok(img) => {
                    let tex = ctx.load_texture(
                        format!("thumb:{}", path.display()),
                        img,
                        egui::TextureOptions::LINEAR,
                    );
                    self.thumb_tex.insert(path, tex);
                }
                Err(_) => {
                    self.thumb_failed.insert(path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_navigation() {
        assert_eq!(columns(CELL + 2.0 * GAP), 1);
        assert_eq!(columns(5.0 * (CELL + GAP) + GAP), 5);
        // 5 columns, 23 items.
        assert_eq!(moved(7, 23, 5, 0, 1), 12);
        assert_eq!(moved(7, 23, 5, 0, -2), 0);
        assert_eq!(moved(20, 23, 5, 0, 1), 22);
        assert_eq!(moved(0, 23, 5, -1, 0), 0);
        assert_eq!(moved(4, 23, 5, 1, 0), 5);
        assert_eq!(star_points(Pos2::ZERO, 10.0).len(), 10);
    }
}
