//! The image canvas: drawing, zoom, pan and wheel handling.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Event, Mesh, MouseWheelUnit,
    PointerButton, Pos2, Rect, Sense, Shape, StrokeKind, TextureOptions,
};

use crate::app::{MuudApp, VERSION};
use crate::core::folder;
use crate::core::keymap::Cmd;
use crate::core::library::Slot;
use crate::core::view::{clamp_offset, compose_uvs, corners, zoom_about, ZoomMode};
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
        let draw_path = self.shown.clone().filter(|s| self.textures.contains_key(s));
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
        let Some(Slot::Ready(d)) = self.lib.get(&draw_path) else {
            return;
        };
        let d = d.clone();

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

        if resp.dragged_by(PointerButton::Primary) {
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
        let can_pan = shown.x > rect.width() + 0.5 || shown.y > rect.height() + 0.5;
        if can_pan && resp.hovered() {
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

        let textures = &self.textures[&draw_path];
        let tex = &textures[self.frame_index.min(textures.len() - 1)];
        let mut mesh = Mesh::with_texture(tex.id());
        let uvs = compose_uvs(
            d.orientation,
            self.view.turns,
            self.view.flip_h,
            self.view.flip_v,
        );
        for (pos, uv) in corners(dest).into_iter().zip(uvs) {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv,
                color: Color32::WHITE,
            });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        p.add(Shape::mesh(mesh));
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
