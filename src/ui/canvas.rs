//! The image canvas: drawing, zoom, pan and wheel handling.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Event, Mesh, MouseWheelUnit,
    PointerButton, Pos2, Rect, Sense, Shape, StrokeKind, TextureOptions,
};

use std::path::PathBuf;

use crate::app::{MuudApp, VERSION};
use crate::core::compare::CompareMode;
use crate::core::folder;
use crate::core::keymap::Cmd;
use crate::core::library::Slot;
use crate::core::view::{
    clamp_offset, compose_uvs, corners, minimap_size, offset_to_center, place_part, visible_part,
    zoom_about, ZoomMode,
};
use crate::platform;
use crate::ui::theme::*;

impl MuudApp {
    fn checker_texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        self.checker
            .get_or_insert_with(|| {
                let a = Color32::from_gray(0xD9);
                let b = Color32::from_gray(0xF2);
                let img = egui::ColorImage::new([2, 2], vec![a, b, b, a]);
                ctx.load_texture("checker", img, TextureOptions::NEAREST_REPEAT)
            })
            .id()
    }

    pub(crate) fn canvas(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let p = ui.painter_at(rect);
        self.last_canvas = rect;

        if self.lib.is_empty() {
            self.empty_state(&p, rect);
            return;
        }
        let cur = self.lib.current().cloned();
        if let Some(Slot::Failed(err)) = cur.as_ref().and_then(|c| self.lib.get(c)) {
            let name = cur
                .as_ref()
                .map(|c| folder::file_name(c))
                .unwrap_or_default();
            crate::ui::overlays::message_card(
                &p,
                rect.center(),
                crate::ui::i18n::tr("CANNOT OPEN"),
                &name,
                err,
            );
            self.wheel_navigation(&ctx, &resp);
            return;
        }
        // Comparing: lay out and draw image A; B follows it (see
        // `draw_compare`).
        let cmp = self
            .compare
            .as_ref()
            .map(|c| (c.a.clone(), c.mode, c.split));
        let full = rect;
        let side = matches!(cmp, Some((_, CompareMode::SideBySide, _)));
        let rect = if side {
            Rect::from_min_max(full.min, pos2(full.center().x - 1.0, full.max.y))
        } else {
            full
        };
        let draw_path = match &cmp {
            Some((a, _, _)) => Some(a.clone()),
            None => self.shown.clone(),
        }
        .filter(|s| self.textures.contains_key(s));
        let Some(draw_path) = draw_path else {
            let name = cur
                .as_ref()
                .map(|c| folder::file_name(c))
                .unwrap_or_default();
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                crate::ui::i18n::tr("DECODING …"),
                mono(12.0),
                self.on_canvas_color(),
            );
            p.text(
                rect.center() + vec2(0.0, 20.0),
                Align2::CENTER_CENTER,
                name,
                mono(11.0),
                self.on_canvas_color().gamma_multiply(0.6),
            );
            return;
        };
        // Draw what the textures were made from (the cache may already hold
        // a newer decode that is uploaded next frame).
        let d = self.textures[&draw_path].src.clone();

        let ppp = ctx.pixels_per_point();
        let img = self.view.rotated(d.width as f32, d.height as f32);
        let mut scale = self
            .view
            .scale(img, rect.size(), ppp, self.settings.stretch_small);
        let mut shown = img * scale / ppp;
        if self.view.align_top {
            self.view.offset = vec2(0.0, (shown.y - rect.height()) / 2.0);
            self.view.align_top = false;
        }

        // Ctrl+wheel zooms at the cursor; the wheel alone scrolls a tall image
        // or moves to the next/previous image.
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let wheel: Vec<(f32, bool)> = if resp.hovered() {
            ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        Event::MouseWheel {
                            unit,
                            delta,
                            modifiers,
                            ..
                        } => {
                            let notches = match unit {
                                MouseWheelUnit::Line => delta.y,
                                MouseWheelUnit::Point => delta.y / 50.0,
                                MouseWheelUnit::Page => delta.y * 3.0,
                            };
                            Some((notches, modifiers.ctrl || modifiers.command))
                        }
                        _ => None,
                    })
                    .collect()
            })
        } else {
            Vec::new()
        };
        let overflow_y = shown.y > rect.height() + 0.5;
        let zoom_wheel = self.settings.wheel == crate::core::settings::WheelMode::Zoom;
        let max_offset = ((shown.y - rect.height()) / 2.0).max(0.0);
        for (notches, ctrl) in wheel {
            if ctrl != zoom_wheel {
                let new = (scale * 1.15f32.powf(notches))
                    .clamp(crate::core::view::MIN_SCALE, crate::core::view::MAX_SCALE);
                let anchor = pointer.unwrap_or(rect.center());
                self.view.offset = zoom_about(self.view.offset, rect.center(), anchor, scale, new);
                self.view.mode = ZoomMode::Free(new);
                scale = new;
                shown = img * scale / ppp;
            } else if overflow_y {
                // At the bottom (or top) edge, keep scrolling to move on.
                let at_bottom = self.view.offset.y <= -max_offset + 0.5 && notches < 0.0;
                let at_top = self.view.offset.y >= max_offset - 0.5 && notches > 0.0;
                if self.settings.wheel_edge_next && (at_bottom || at_top) {
                    self.edge_accum -= notches;
                    if self.edge_accum >= 2.0 {
                        self.edge_accum = 0.0;
                        self.run(&ctx, Cmd::Next);
                    } else if self.edge_accum <= -2.0 {
                        self.edge_accum = 0.0;
                        self.run(&ctx, Cmd::Prev);
                    }
                } else {
                    self.edge_accum = 0.0;
                    self.view.offset.y += notches * 90.0;
                }
            } else {
                self.wheel_accum -= notches;
                while self.wheel_accum >= 1.0 {
                    self.wheel_accum -= 1.0;
                    self.run(&ctx, Cmd::Next);
                }
                while self.wheel_accum <= -1.0 {
                    self.wheel_accum += 1.0;
                    self.run(&ctx, Cmd::Prev);
                }
            }
        }

        // Slider: dragging near the divider moves it instead of the image.
        let mut moving_split = false;
        if let (Some((_, CompareMode::Slider, split)), Some(c)) = (&cmp, self.compare.as_mut()) {
            let x = full.left() + split * full.width();
            if resp.drag_started_by(PointerButton::Primary) {
                c.dragging_split = pointer.is_some_and(|p| (p.x - x).abs() < 14.0);
            }
            if !resp.dragged() {
                c.dragging_split = false;
            }
            if c.dragging_split {
                if let Some(p) = pointer {
                    c.split = ((p.x - full.left()) / full.width()).clamp(0.0, 1.0);
                }
                moving_split = true;
            }
            if pointer.is_some_and(|p| (p.x - x).abs() < 14.0) && resp.hovered() {
                ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
        }
        if resp.dragged_by(PointerButton::Primary) && !moving_split {
            self.view.offset += resp.drag_delta();
        }
        if resp.double_clicked() {
            self.mouse_action(&ctx, self.settings.double_click);
        }
        if resp.clicked_by(PointerButton::Middle) {
            if ctx.input(|i| i.modifiers.ctrl) {
                self.run(&ctx, Cmd::Original);
            } else {
                self.mouse_action(&ctx, self.settings.middle_click);
            }
        }
        if self.settings.side_buttons {
            if resp.clicked_by(PointerButton::Extra1) {
                self.run(&ctx, Cmd::Prev);
            }
            if resp.clicked_by(PointerButton::Extra2) {
                self.run(&ctx, Cmd::Next);
            }
        }

        self.view.offset = clamp_offset(self.view.offset, shown, rect.size());
        self.last_scale = scale;
        // Zoomed past the screen-size pixels: load every pixel.
        if cur.as_ref() == Some(&draw_path) && needs_more_pixels(&d, scale) {
            self.lib.request_full();
        }
        let can_pan = shown.x > rect.width() + 0.5 || shown.y > rect.height() + 0.5;
        if can_pan && resp.hovered() && !moving_split {
            ctx.set_cursor_icon(if resp.dragged() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }

        // Snap to physical pixels so 100% stays crisp.
        let center = rect.center() + self.view.offset;
        let min = center - shown / 2.0;
        let min = pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp);
        let dest = Rect::from_min_size(min, shown);

        // Side by side: A stays in its half.
        let p = if side { p.with_clip_rect(rect) } else { p };
        if d.has_alpha {
            let checker = self.checker_texture(&ctx);
            let tiles = dest.size() / 16.0;
            p.image(
                checker,
                dest,
                Rect::from_min_max(Pos2::ZERO, pos2(tiles.x / 2.0, tiles.y / 2.0)),
                Color32::WHITE,
            );
        }

        let uvs = compose_uvs(
            d.orientation,
            self.view.turns,
            self.view.flip_h,
            self.view.flip_v,
        );
        let minimap = (can_pan && self.settings.minimap && cmp.is_none()).then(|| {
            let size = minimap_size(shown, 168.0);
            // Lifted above the floating bottom bar while it shows.
            let lift = crate::app::BOTTOM_BAR * self.bar_visible_bottom;
            Rect::from_min_size(rect.right_bottom() - size - vec2(16.0, 16.0 + lift), size)
        });
        // Clicking or dragging in the minimap pans there.
        if let Some(mini) = minimap {
            let r = ui.interact(mini, ui.scope_id().with("minimap"), Sense::click_and_drag());
            if let Some(pos) = r
                .interact_pointer_pos()
                .filter(|_| r.is_pointer_button_down_on())
            {
                let at = pos2(
                    ((pos.x - mini.min.x) / mini.width()).clamp(0.0, 1.0),
                    ((pos.y - mini.min.y) / mini.height()).clamp(0.0, 1.0),
                );
                self.view.offset = clamp_offset(offset_to_center(at, shown), shown, rect.size());
                ctx.request_repaint();
            }
            if r.hovered() {
                ctx.set_cursor_icon(CursorIcon::Crosshair);
            }
        }

        let tex = &self.textures[&draw_path];
        if tex.tiles.is_empty() {
            let frame = &tex.handles[self.frame_index.min(tex.handles.len() - 1)];
            p.add(quad(frame.id(), corners(dest), uvs));
        } else {
            // A huge image: every piece at full resolution, skipping the
            // ones that are off screen.
            let unit = [
                pos2(0.0, 0.0),
                pos2(1.0, 0.0),
                pos2(1.0, 1.0),
                pos2(0.0, 1.0),
            ];
            for (part, handle) in &tex.tiles {
                let at = place_part(corners(dest), uvs, *part);
                let bounds = Rect::from_points(&at);
                if bounds.intersects(rect) {
                    p.add(quad(handle.id(), at, unit));
                }
            }
        }

        if let Some(mini) = minimap {
            // Small copy of the image (the screen-size texture or the tiled
            // image's preview), the visible part outlined, the rest dimmed.
            let frame = &tex.handles[self.frame_index.min(tex.handles.len() - 1)];
            p.rect_filled(mini.expand(4.0), CornerRadius::ZERO, paper());
            p.add(quad(frame.id(), corners(mini), uvs));
            let v = visible_part(dest, rect);
            let view = Rect::from_min_max(
                mini.min + v.min.to_vec2() * mini.size(),
                mini.min + v.max.to_vec2() * mini.size(),
            );
            let dim = Color32::from_black_alpha(110);
            for r in [
                Rect::from_min_max(mini.min, pos2(mini.max.x, view.min.y)),
                Rect::from_min_max(pos2(mini.min.x, view.max.y), mini.max),
                Rect::from_min_max(pos2(mini.min.x, view.min.y), pos2(view.min.x, view.max.y)),
                Rect::from_min_max(pos2(view.max.x, view.min.y), pos2(mini.max.x, view.max.y)),
            ] {
                if r.is_positive() {
                    p.rect_filled(r, CornerRadius::ZERO, dim);
                }
            }
            p.rect_stroke(
                view,
                CornerRadius::ZERO,
                egui::Stroke::new(1.5, Color32::WHITE),
                StrokeKind::Inside,
            );
            p.rect_stroke(
                mini.expand(4.0),
                CornerRadius::ZERO,
                hair(),
                StrokeKind::Inside,
            );
        }

        if let Some((_, mode, split)) = cmp {
            let p = ui.painter_at(full);
            self.draw_compare(&ctx, &p, full, dest, (mode, split), cur.as_ref());
        }
    }

    /// Draws image B over/next to A (already drawn at `dest`) and the labels.
    fn draw_compare(
        &mut self,
        ctx: &egui::Context,
        p: &egui::Painter,
        full: Rect,
        dest: Rect,
        (mode, split): (CompareMode, f32),
        b_path: Option<&PathBuf>,
    ) {
        let label_bg = |p: &egui::Painter, at: Pos2, align: Align2, text: String| {
            let galley = p.layout_no_wrap(text, mono(11.0), ink());
            let r = align.anchor_size(at, galley.size() + vec2(12.0, 6.0));
            p.rect_filled(r, CornerRadius::ZERO, paper());
            p.galley(r.min + vec2(6.0, 3.0), galley, ink());
        };
        let a_name = self
            .compare
            .as_ref()
            .map(|c| folder::file_name(&c.a))
            .unwrap_or_default();
        let b_tex = b_path.and_then(|b| self.textures.get(b));
        let Some(b_tex) = b_tex else {
            let center = match mode {
                CompareMode::SideBySide => {
                    pos2(full.center().x + full.width() / 4.0, full.center().y)
                }
                _ => full.center(),
            };
            p.text(
                center,
                Align2::CENTER_CENTER,
                crate::ui::i18n::tr("DECODING …"),
                mono(12.0),
                self.on_canvas_color(),
            );
            return;
        };
        let b = b_tex.src.clone();
        let b_name = b_path.map(|p| folder::file_name(p)).unwrap_or_default();
        let b_handle = b_tex.handles[0].id();
        let b_img = self.view.rotated(b.width as f32, b.height as f32);
        let shown_b = crate::core::compare::match_width(dest.size(), b_img);
        let uvs_b = compose_uvs(
            b.orientation,
            self.view.turns,
            self.view.flip_h,
            self.view.flip_v,
        );
        match mode {
            CompareMode::SideBySide => {
                let half = full.width() / 2.0;
                let right = Rect::from_min_max(pos2(full.center().x + 1.0, full.min.y), full.max);
                let at = Rect::from_min_size(dest.min + vec2(half, 0.0), shown_b);
                p.with_clip_rect(right)
                    .add(quad(b_handle, corners(at), uvs_b));
                p.line_segment(
                    [
                        pos2(full.center().x, full.min.y),
                        pos2(full.center().x, full.max.y),
                    ],
                    hair(),
                );
                label_bg(
                    p,
                    full.left_top() + vec2(12.0, 12.0),
                    Align2::LEFT_TOP,
                    format!("A · {a_name}"),
                );
                label_bg(
                    p,
                    pos2(full.center().x + 12.0, full.top() + 12.0),
                    Align2::LEFT_TOP,
                    format!("B · {b_name}"),
                );
            }
            CompareMode::Slider => {
                let x = full.left() + split * full.width();
                let clip = Rect::from_min_max(pos2(x, full.min.y), full.max);
                let at = Rect::from_center_size(dest.center(), shown_b);
                p.with_clip_rect(clip)
                    .add(quad(b_handle, corners(at), uvs_b));
                p.line_segment(
                    [pos2(x, full.min.y), pos2(x, full.max.y)],
                    egui::Stroke::new(2.0, Color32::WHITE),
                );
                p.circle_filled(pos2(x, full.center().y), 9.0, Color32::WHITE);
                p.circle_stroke(
                    pos2(x, full.center().y),
                    9.0,
                    egui::Stroke::new(1.0, Color32::BLACK),
                );
                label_bg(
                    p,
                    pos2(x - 8.0, full.top() + 12.0),
                    Align2::RIGHT_TOP,
                    format!("A · {a_name}"),
                );
                label_bg(
                    p,
                    pos2(x + 8.0, full.top() + 12.0),
                    Align2::LEFT_TOP,
                    format!("B · {b_name}"),
                );
            }
            CompareMode::Difference => {
                let a_dec = self
                    .compare
                    .as_ref()
                    .and_then(|c| self.textures.get(&c.a))
                    .map(|t| t.src.clone());
                let Some(a_dec) = a_dec else { return };
                let fresh = self
                    .compare
                    .as_ref()
                    .and_then(|c| c.diff.as_ref())
                    .is_some_and(|d| {
                        std::sync::Arc::ptr_eq(&d.a, &a_dec) && std::sync::Arc::ptr_eq(&d.b, &b)
                    });
                if !fresh {
                    let ua =
                        crate::core::decode::upright(&a_dec.frames[0].image, a_dec.orientation);
                    let ub = crate::core::decode::upright(&b.frames[0].image, b.orientation);
                    let (img, stats) = crate::core::compare::diff(&ua, &ub);
                    let texture = ctx.load_texture("compare-diff", img, self.texture_options());
                    if let Some(c) = self.compare.as_mut() {
                        c.diff = Some(crate::app::DiffCache {
                            a: a_dec,
                            b: b.clone(),
                            texture,
                            stats,
                        });
                    }
                }
                let Some(diff) = self.compare.as_ref().and_then(|c| c.diff.as_ref()) else {
                    return;
                };
                let uvs = compose_uvs(
                    crate::core::decode::Orientation::default(),
                    self.view.turns,
                    self.view.flip_h,
                    self.view.flip_v,
                );
                p.rect_filled(full, CornerRadius::ZERO, Color32::from_gray(10));
                p.add(quad(diff.texture.id(), corners(dest), uvs));
                let s = diff.stats;
                let summary = if s.changed == 0.0 {
                    crate::ui::i18n::tr("IDENTICAL").to_string()
                } else {
                    format!(
                        "{:.2}% {} · MAX {}",
                        s.changed * 100.0,
                        crate::ui::i18n::tr("PIXELS DIFFER"),
                        s.max
                    )
                };
                label_bg(
                    p,
                    full.left_top() + vec2(12.0, 12.0),
                    Align2::LEFT_TOP,
                    format!("A · {a_name}   B · {b_name}"),
                );
                label_bg(
                    p,
                    full.left_top() + vec2(12.0, 40.0),
                    Align2::LEFT_TOP,
                    summary,
                );
            }
        }
    }

    fn wheel_navigation(&mut self, ctx: &egui::Context, resp: &egui::Response) {
        if !resp.hovered() {
            return;
        }
        let notches: f32 = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } if !modifiers.ctrl => Some(match unit {
                        MouseWheelUnit::Line => delta.y,
                        MouseWheelUnit::Point => delta.y / 50.0,
                        MouseWheelUnit::Page => delta.y * 3.0,
                    }),
                    _ => None,
                })
                .sum()
        });
        self.wheel_accum -= notches;
        if self.wheel_accum >= 1.0 {
            self.wheel_accum = 0.0;
            self.run(ctx, Cmd::Next);
        } else if self.wheel_accum <= -1.0 {
            self.wheel_accum = 0.0;
            self.run(ctx, Cmd::Prev);
        }
    }

    fn empty_state(&self, p: &egui::Painter, rect: Rect) {
        let w = 440.0f32.min(rect.width() - 32.0);
        let h = 372.0;
        let card = Rect::from_center_size(rect.center(), vec2(w, h));
        p.rect_filled(card, CornerRadius::ZERO, paper());
        p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
        let x0 = card.left() + 24.0;
        let x1 = card.right() - 24.0;
        let mut y = card.top() + 22.0;
        let r = p.text(
            pos2(x0, y),
            Align2::LEFT_TOP,
            "MUUD VIEW",
            grotesk(40.0),
            ink(),
        );
        y = r.bottom() + 12.0;
        let mut x = x0;
        for t in [
            format!("v{VERSION}"),
            "NO ADS".into(),
            "OFFLINE".into(),
            "RUST".into(),
        ] {
            x += pill(p, pos2(x, y + 11.0), &t, ink()) + 6.0;
        }
        y += 22.0 + 22.0;

        // Boxed section, like a spec sheet cell.
        let boxr = Rect::from_min_max(
            pos2(card.left() + 12.0, y),
            pos2(card.right() - 12.0, card.bottom() - 12.0),
        );
        p.rect_stroke(boxr, CornerRadius::ZERO, hair(), StrokeKind::Inside);
        let bx = boxr.left() + 16.0;
        section_title(p, pos2(bx, boxr.top() + 14.0), "OPEN");
        let cx = boxr.center().x;
        let mut yy = boxr.top() + 50.0;
        p.text(
            pos2(cx, yy),
            Align2::CENTER_TOP,
            crate::ui::i18n::tr("DROP AN IMAGE"),
            grotesk(26.0),
            ink(),
        );
        yy += 42.0;
        dotted_rule(p, pos2(bx, yy), pos2(boxr.right() - 16.0, yy), ink());
        yy += 16.0;
        use crate::core::keymap::Cmd as C;
        use crate::ui::i18n::tr;
        let k = |c: C| self.keymap.short_keys(&[c]);
        let lines: Vec<String> = if platform::dialogs_available() {
            vec![
                format!("{}  {}", k(C::OpenFile), tr("OPEN FILE")),
                format!("{}  {}", k(C::OpenFolder), tr("OPEN FOLDER")),
                "↓".into(),
                format!("{}  {}", k(C::Help), tr("ALL SHORTCUTS")),
            ]
        } else {
            vec![
                "muud-view <FILE>".into(),
                "↓".into(),
                format!("{}  {}", k(C::Help), tr("ALL SHORTCUTS")),
            ]
        };
        for line in lines {
            let r = p.text(pos2(cx, yy), Align2::CENTER_TOP, line, mono(12.5), ink());
            yy = r.bottom() + 8.0;
        }
        let _ = x1;
    }
}

/// A textured quad: corners TL, TR, BR, BL and their texture coordinates.
fn quad(texture: egui::TextureId, at: [Pos2; 4], uvs: [Pos2; 4]) -> Shape {
    let mut mesh = Mesh::with_texture(texture);
    for (pos, uv) in at.into_iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv,
            color: Color32::WHITE,
        });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    Shape::mesh(mesh)
}

/// True if the image is drawn with more screen pixels than its texture has
/// and a full-resolution decode would add detail.
pub fn needs_more_pixels(d: &crate::core::decode::Decoded, scale: f32) -> bool {
    if !d.reduced {
        return false;
    }
    let [tw, th] = d.texture_size();
    let drawn = d.width.max(d.height) as f32 * scale;
    drawn > tw.max(th) as f32 * 1.02
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::ColorImage;

    #[test]
    fn full_resolution_when_zoomed_past_screen_pixels() {
        // 6960x4640 shown from a 2160x1440 screen texture.
        let mut d = crate::core::decode::Decoded::from_color_image(
            ColorImage::new([2160, 1440], vec![Color32::BLACK; 2160 * 1440]),
            "JPEG",
        );
        d.width = 6960;
        d.height = 4640;
        d.reduced = true;
        let fit = 1440.0 / 4640.0;
        assert!(!needs_more_pixels(&d, fit));
        assert!(needs_more_pixels(&d, fit * 1.5));
        assert!(needs_more_pixels(&d, 1.0));
        d.reduced = false;
        assert!(!needs_more_pixels(&d, 1.0));
    }
}
