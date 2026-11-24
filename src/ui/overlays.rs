//! Things drawn above everything: drop hint, help sheet, confirmations, toasts.

use eframe::egui::{
    self, pos2, vec2, Align2, CornerRadius, Id, LayerId, Order, Pos2, Rect, Stroke, StrokeKind,
};

use crate::app::{MuudApp, VERSION};
use crate::core::folder;
use crate::core::keymap::{HelpSection, HELP};
use crate::ui::theme::*;

impl MuudApp {
    pub(crate) fn overlays(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let p = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("muud-overlay")));

        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if hovering {
            p.rect_filled(screen, CornerRadius::ZERO, INK.gamma_multiply(0.7));
            let card = Rect::from_center_size(screen.center(), vec2(320.0, 120.0));
            p.rect_filled(card, CornerRadius::ZERO, PAPER);
            p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
            p.text(
                card.center() - vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                "DROP TO OPEN",
                grotesk(24.0),
                INK,
            );
            p.text(
                card.center() + vec2(0.0, 22.0),
                Align2::CENTER_CENTER,
                "↓",
                mono(14.0),
                INK,
            );
        }

        if self.show_help {
            p.rect_filled(screen, CornerRadius::ZERO, INK.gamma_multiply(0.75));
            help_sheet(&p, screen);
            if ctx.input(|i| i.pointer.any_pressed()) {
                self.show_help = false;
            }
        }

        if self.confirm_delete {
            p.rect_filled(screen, CornerRadius::ZERO, INK.gamma_multiply(0.75));
            let name = self
                .lib
                .current()
                .map(|c| folder::file_name(c))
                .unwrap_or_default();
            message_card(
                &p,
                screen.center(),
                "DELETE PERMANENTLY?",
                &name,
                "ENTER  DELETE      ESC  CANCEL",
            );
        }

        if let Some((text, since)) = &self.toast {
            let age = since.elapsed().as_secs_f32();
            let alpha = if age > 1.4 {
                ((1.8 - age) / 0.4).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let galley =
                p.layout_no_wrap(text.clone(), mono_medium(12.0), INK.gamma_multiply(alpha));
            let size = galley.size() + vec2(28.0, 14.0);
            let r = Rect::from_center_size(pos2(screen.center().x, screen.bottom() - 48.0), size);
            p.rect_filled(r, CornerRadius::same(255), PAPER.gamma_multiply(alpha));
            p.rect_stroke(
                r,
                CornerRadius::same(255),
                Stroke::new(1.0, INK.gamma_multiply(alpha)),
                StrokeKind::Inside,
            );
            p.galley(r.min + vec2(14.0, 7.0), galley, INK);
        }
    }
}

pub(crate) fn message_card(p: &egui::Painter, center: Pos2, title: &str, name: &str, detail: &str) {
    let w = 420.0;
    let name = ellipsize(p, name, mono(12.0), w - 48.0);
    let detail_g = p.layout(detail.to_owned(), mono(12.0), INK, w - 48.0);
    let h = 132.0 + detail_g.size().y;
    let card = Rect::from_center_size(center, vec2(w, h));
    p.rect_filled(card, CornerRadius::ZERO, PAPER);
    p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
    let x0 = card.left() + 24.0;
    let mut y = card.top() + 22.0;
    let r = p.text(pos2(x0, y), Align2::LEFT_TOP, title, grotesk(24.0), INK);
    y = r.bottom() + 10.0;
    p.text(pos2(x0, y), Align2::LEFT_TOP, name, mono(12.0), INK);
    y += 26.0;
    dotted_rule(p, pos2(x0, y), pos2(card.right() - 24.0, y), INK);
    y += 14.0;
    p.galley(pos2(x0, y), detail_g, INK);
}

fn help_sheet(p: &egui::Painter, screen: Rect) {
    let w = 760.0f32.min(screen.width() - 32.0);
    let col_w = (w - 24.0 * 2.0 - 24.0) / 2.0;
    let row_h = 19.0;
    let section_h = |rows: usize| 28.0 + rows as f32 * row_h + 18.0;
    let (left, right): (Vec<_>, Vec<_>) = HELP.iter().enumerate().partition(|(i, _)| i % 2 == 0);
    let col_h =
        |c: &[(usize, &HelpSection)]| c.iter().map(|(_, (_, r))| section_h(r.len())).sum::<f32>();
    let body_h = col_h(&left).max(col_h(&right));
    let h = (96.0 + body_h).min(screen.height() - 32.0);
    let card = Rect::from_center_size(screen.center(), vec2(w, h));
    p.rect_filled(card, CornerRadius::ZERO, PAPER);
    p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
    let p = p.with_clip_rect(card);
    let x0 = card.left() + 24.0;
    let mut y = card.top() + 20.0;
    let r = p.text(
        pos2(x0, y),
        Align2::LEFT_TOP,
        "SHORTCUTS",
        grotesk(32.0),
        INK,
    );
    let mut x = r.right() + 14.0;
    for t in [format!("v{VERSION}"), "ANY KEY TO CLOSE".into()] {
        x += pill(&p, pos2(x, r.center().y), &t, INK) + 6.0;
    }
    y = r.bottom() + 16.0;
    p.line_segment([pos2(card.left(), y), pos2(card.right(), y)], hair());
    let mid = card.left() + w / 2.0;
    p.line_segment([pos2(mid, y), pos2(mid, card.bottom())], hair());
    for (col, items) in [(0, left), (1, right)] {
        let cx = if col == 0 { x0 } else { mid + 24.0 };
        let mut cy = y + 16.0;
        for (_, (title, rows)) in items {
            sub_label(&p, pos2(cx, cy), title);
            cy += 26.0;
            for (keys, action) in rows.iter() {
                p.text(pos2(cx, cy), Align2::LEFT_TOP, *keys, mono(12.0), INK);
                p.text(
                    pos2(cx + col_w, cy),
                    Align2::RIGHT_TOP,
                    *action,
                    mono(12.0),
                    INK,
                );
                cy += row_h;
            }
            cy += 18.0;
        }
    }
}
