//! The bottom bar: seekable progress through the folder and view buttons.
//! Both bars can be pinned or left to fade in when the mouse comes near.

use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, Id, Order, Pos2, Rect, Sense, Stroke,
    StrokeKind,
};

use crate::app::{MuudApp, BOTTOM_BAR, TOP_BAR};
use crate::core::folder;
use crate::core::keymap::Cmd;
use crate::ui::theme::*;

/// How close to an edge (points) the pointer must be to reveal a hidden bar.
const REVEAL_ZONE: f32 = 96.0;
/// How long a revealed bar stays after the pointer leaves.
const LINGER: Duration = Duration::from_millis(1400);

/// Fraction (0..=1) of the track for item `index` of `len`.
pub fn track_fraction(index: usize, len: usize) -> f32 {
    if len <= 1 {
        1.0
    } else {
        index as f32 / (len - 1) as f32
    }
}

/// Item under horizontal position `x` on a track spanning `left..left+width`.
pub fn index_at(x: f32, left: f32, width: f32, len: usize) -> usize {
    if len == 0 || width <= 0.0 {
        return 0;
    }
    let t = ((x - left) / width).clamp(0.0, 1.0);
    ((t * (len - 1) as f32).round() as usize).min(len - 1)
}

#[derive(Default)]
pub struct BarState {
    top_seen: Option<Instant>,
    bottom_seen: Option<Instant>,
}

pub enum Edge {
    Top,
    Bottom,
}

impl MuudApp {
    /// Opacity (0..=1) of an auto-hiding bar this frame.
    pub(crate) fn bar_opacity(&mut self, ctx: &egui::Context, edge: Edge) -> f32 {
        let screen = ctx.content_rect();
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let near = pointer.is_some_and(|p| match edge {
            Edge::Top => p.y < screen.top() + REVEAL_ZONE,
            Edge::Bottom => p.y > screen.bottom() - REVEAL_ZONE,
        }) && ctx.input(|i| i.pointer.has_pointer());
        let now = Instant::now();
        let seen = match edge {
            Edge::Top => &mut self.bars.top_seen,
            Edge::Bottom => &mut self.bars.bottom_seen,
        };
        if near {
            *seen = Some(now);
        }
        let visible = seen.is_some_and(|t| now - t < LINGER);
        if visible {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let id = Id::unique(match edge {
            Edge::Top => "top-bar-fade",
            Edge::Bottom => "bottom-bar-fade",
        });
        ctx.animate_bool_with_time(id, visible, 0.18)
    }

    /// Draws a bar as a floating layer over the canvas.
    pub(crate) fn floating_bar(&mut self, ctx: &egui::Context, edge: Edge, opacity: f32) {
        let screen = ctx.content_rect();
        let (h, pos) = match edge {
            Edge::Top => (TOP_BAR, screen.left_top()),
            Edge::Bottom => (
                BOTTOM_BAR,
                pos2(screen.left(), screen.bottom() - BOTTOM_BAR),
            ),
        };
        let id = match edge {
            Edge::Top => "top-bar-floating",
            Edge::Bottom => "bottom-bar-floating",
        };
        egui::Area::new(Id::unique(id))
            .order(Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                ui.multiply_opacity(opacity);
                let (rect, _) = ui.allocate_exact_size(vec2(screen.width(), h), Sense::click());
                ui.painter().rect_filled(rect, CornerRadius::ZERO, paper());
                match edge {
                    Edge::Top => self.top_bar(ui, rect),
                    Edge::Bottom => self.bottom_bar(ui, rect),
                }
            });
    }

    /// Two-pixel progress line along the bottom of the canvas when the bar is hidden.
    pub(crate) fn progress_line(&self, painter: &egui::Painter, canvas: Rect) {
        if self.lib.len() < 2 {
            return;
        }
        let t = track_fraction(self.lib.index, self.lib.len());
        let y = canvas.bottom() - 1.0;
        let c = self.on_canvas_color();
        let split = canvas.left() + canvas.width() * t;
        painter.line_segment(
            [pos2(canvas.left(), y), pos2(canvas.right(), y)],
            Stroke::new(2.0, c.gamma_multiply(0.22)),
        );
        painter.line_segment(
            [pos2(canvas.left(), y), pos2(split, y)],
            Stroke::new(2.0, c.gamma_multiply(0.9)),
        );
    }

    pub(crate) fn bottom_bar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        p.line_segment(
            [
                rect.left_top() + vec2(0.0, 0.5),
                rect.right_top() + vec2(0.0, 0.5),
            ],
            hair(),
        );
        let cy = rect.center().y;
        let mut clicked: Option<Cmd> = None;

        // Left: previous / next.
        let mut x = rect.left() + 12.0;
        for (label, cmd) in [("PREV", Cmd::Prev), ("NEXT", Cmd::Next)] {
            let (w, hit) = pill_button(ui, pos2(x, cy), label, false);
            if hit {
                clicked = Some(cmd);
            }
            x += w + 6.0;
        }

        // Right: view buttons, laid out from the right edge.
        let fit = matches!(self.view.mode, crate::core::view::ZoomMode::Fit);
        let one = matches!(self.view.mode, crate::core::view::ZoomMode::Original);
        let right: [(&str, Cmd, bool); 8] = [
            ("−", Cmd::ZoomOut, false),
            ("1:1", Cmd::Original, one),
            ("+", Cmd::ZoomIn, false),
            ("FIT", Cmd::FitWindow, fit),
            ("ROT", Cmd::RotateCw, false),
            ("FULL", Cmd::Fullscreen, false),
            ("INFO", Cmd::ToggleInfo, self.settings.show_info),
            (
                if self.settings.pin_bottom {
                    "PINNED"
                } else {
                    "PIN"
                },
                Cmd::PinBottom,
                self.settings.pin_bottom,
            ),
        ];
        let widths: Vec<f32> = right.iter().map(|(l, ..)| pill_width(&p, l)).collect();
        let total: f32 = widths.iter().sum::<f32>() + 6.0 * (right.len() as f32 - 1.0) + 10.0;
        let mut rx = rect.right() - 12.0 - total;
        let right_start = rx;
        for (i, (label, cmd, active)) in right.iter().enumerate() {
            if i == 4 || i == 6 {
                // Small gaps between button groups.
                rx += 5.0;
            }
            let (w, hit) = pill_button(ui, pos2(rx, cy), label, *active);
            if hit {
                clicked = Some(*cmd);
            }
            rx += w + 6.0;
        }

        // Middle: counter and seek track.
        let len = self.lib.len();
        let counter = if len == 0 {
            "000 / 000".to_string()
        } else {
            format!("{:03} / {:03}", self.lib.index + 1, len)
        };
        let r = p.text(
            pos2(x + 8.0, cy),
            Align2::LEFT_CENTER,
            counter,
            mono_medium(12.0),
            ink(),
        );
        let track = Rect::from_min_max(
            pos2(r.right() + 16.0, cy - 10.0),
            pos2(right_start - 20.0, cy + 10.0),
        );
        if track.width() > 40.0 && len > 0 {
            if let Some(target) = self.seek_track(ui, track) {
                self.go_to(target);
            }
        }

        if let Some(cmd) = clicked {
            self.run(&ctx, cmd);
        }
    }

    /// Dotted track with the travelled part solid. Returns an index to jump to.
    fn seek_track(&self, ui: &mut egui::Ui, track: Rect) -> Option<usize> {
        let len = self.lib.len();
        let resp = ui.interact(track, Id::unique("seek-track"), Sense::click_and_drag());
        let p = ui.painter();
        let y = track.center().y;
        let t = track_fraction(self.lib.index, len);
        let cur_x = track.left() + track.width() * t;
        dotted_rule(p, pos2(track.left(), y), pos2(track.right(), y), ink());
        p.line_segment(
            [pos2(track.left(), y), pos2(cur_x, y)],
            Stroke::new(1.6, ink()),
        );
        for x in [track.left(), track.right()] {
            p.line_segment(
                [pos2(x, y - 4.0), pos2(x, y + 4.0)],
                Stroke::new(1.0, ink()),
            );
        }
        p.circle_filled(pos2(cur_x, y), 4.0, ink());

        if let Some(hover) = resp.hover_pos() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            let i = index_at(hover.x, track.left(), track.width(), len);
            let hx = track.left() + track.width() * track_fraction(i, len);
            p.line_segment(
                [pos2(hx, y - 6.0), pos2(hx, y + 6.0)],
                Stroke::new(1.0, ink()),
            );
            let label = format!("{:03}  {}", i + 1, folder::file_name(&self.lib.files[i]));
            let label = ellipsize(p, &label, mono(11.0), 360.0);
            let galley = p.layout_no_wrap(label, mono(11.0), ink());
            let size = galley.size() + vec2(16.0, 8.0);
            let x = (hx - size.x / 2.0).clamp(track.left() - 40.0, track.right() + 40.0 - size.x);
            let tip = Rect::from_min_size(pos2(x, track.top() - size.y - 10.0), size);
            let fg = ui
                .ctx()
                .layer_painter(egui::LayerId::new(Order::Tooltip, Id::unique("seek-tip")));
            fg.rect_filled(tip, CornerRadius::same(255), paper());
            fg.rect_stroke(tip, CornerRadius::same(255), hair(), StrokeKind::Inside);
            fg.galley(tip.min + vec2(8.0, 4.0), galley, ink());
        }
        let pressed = resp.clicked() || resp.dragged();
        let pos = resp.interact_pointer_pos()?;
        pressed.then(|| index_at(pos.x, track.left(), track.width(), len))
    }
}

/// Outlined pill that inverts when hovered or active. Returns (width, clicked).
fn pill_button(ui: &mut egui::Ui, left_center: Pos2, label: &str, active: bool) -> (f32, bool) {
    let p = ui.painter();
    let w = pill_width(p, label);
    let rect = Rect::from_min_size(pos2(left_center.x, left_center.y - 12.0), vec2(w, 24.0));
    let resp = ui.interact(rect, Id::unique(("bar-button", label)), Sense::click());
    let inverted = active || resp.hovered();
    let (fill, fg) = if inverted {
        (ink(), paper())
    } else {
        (Color32::TRANSPARENT, ink())
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(255), fill);
    p.rect_stroke(
        rect,
        CornerRadius::same(255),
        Stroke::new(1.0, ink()),
        StrokeKind::Inside,
    );
    p.text(rect.center(), Align2::CENTER_CENTER, label, mono(11.5), fg);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (w, resp.clicked())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractions() {
        assert_eq!(track_fraction(0, 1), 1.0);
        assert_eq!(track_fraction(0, 5), 0.0);
        assert_eq!(track_fraction(4, 5), 1.0);
        assert_eq!(track_fraction(2, 5), 0.5);
    }

    #[test]
    fn seek_positions() {
        assert_eq!(index_at(0.0, 0.0, 100.0, 5), 0);
        assert_eq!(index_at(100.0, 0.0, 100.0, 5), 4);
        assert_eq!(index_at(49.0, 0.0, 100.0, 5), 2);
        assert_eq!(index_at(-50.0, 0.0, 100.0, 5), 0);
        assert_eq!(index_at(500.0, 0.0, 100.0, 5), 4);
        assert_eq!(index_at(10.0, 0.0, 100.0, 0), 0);
    }

    #[test]
    fn round_trip() {
        for len in [2usize, 7, 120, 5000] {
            for i in [0, len / 3, len - 1] {
                let x = 10.0 + 600.0 * track_fraction(i, len);
                assert_eq!(index_at(x, 10.0, 600.0, len), i);
            }
        }
    }
}
