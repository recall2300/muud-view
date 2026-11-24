//! The top bar: position, file name and status pills.

use eframe::egui::{self, pos2, vec2, Align2, Rect};

use crate::app::MuudApp;
use crate::core::folder;
use crate::core::view::ZoomMode;
use crate::ui::theme::*;

impl MuudApp {
    fn status_pills(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Some(d) = self.lib.current_decoded() {
            v.push(format!("{} × {}", d.width, d.height));
            v.push(human_size(d.file_size));
            v.push(d.format.to_string());
            if d.is_animated() || !d.complete {
                let more = if d.complete { "" } else { "+" };
                v.push(format!("{}{more} FRAMES", d.frames.len()));
            }
            v.push(format!("{:.0}%", self.last_scale * 100.0));
            v.push(
                match self.view.mode {
                    ZoomMode::Fit => "FIT",
                    ZoomMode::FitWidth => "WIDTH",
                    ZoomMode::Original => "1:1",
                    ZoomMode::Free(_) => "ZOOM",
                }
                .to_string(),
            );
            if self.view.turns != 0 {
                v.push(format!("R{}°", self.view.turns as u32 * 90));
            }
            if self.view.flip_h || self.view.flip_v {
                v.push("FLIP".into());
            }
        }
        if self.lib.current().is_some_and(|p| self.stars.contains(p)) {
            v.push("STARRED".into());
        }
        if self.all_files.is_some() {
            v.push(format!("STARRED ONLY {}", self.lib.len()));
        }
        if let Some((every, _)) = self.slideshow {
            v.push(format!("▶ {}S", every.as_secs()));
        }
        if self.on_top {
            v.push("PIN".into());
        }
        v
    }

    pub(crate) fn top_bar(&self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter();
        p.line_segment(
            [
                rect.left_bottom() - vec2(0.0, 0.5),
                rect.right_bottom() - vec2(0.0, 0.5),
            ],
            hair(),
        );
        let cy = rect.center().y;

        let brand = p.layout_no_wrap("MUUD VIEW".into(), grotesk(14.0), ink());
        let brand_x = rect.right() - 16.0 - brand.size().x;
        p.galley(pos2(brand_x, cy - brand.size().y / 2.0), brand, ink());
        let hint_text = if self.settings.pin_top {
            "F1 KEYS"
        } else {
            "F1 KEYS · ` PIN"
        };
        let hint = p.layout_no_wrap(hint_text.into(), mono(11.0), muted());
        let hint_x = brand_x - 14.0 - hint.size().x;
        p.galley(pos2(hint_x, cy - hint.size().y / 2.0), hint, muted());
        let right_limit = hint_x - 18.0;

        let mut x = rect.left() + 16.0;
        let Some(path) = self.lib.current() else {
            p.text(
                pos2(x, cy),
                Align2::LEFT_CENTER,
                "NO FILE",
                mono(12.0),
                muted(),
            );
            return;
        };
        let counter = format!("{:03} / {:03}", self.lib.index + 1, self.lib.len());
        let r = p.text(
            pos2(x, cy),
            Align2::LEFT_CENTER,
            counter,
            mono_medium(12.0),
            ink(),
        );
        x = r.right() + 14.0;

        let pills = self.status_pills();
        let pills_w: f32 = pills.iter().map(|t| pill_width(p, t) + 6.0).sum();
        let name_max = (right_limit - x - pills_w - 14.0).max(90.0);
        let name = ellipsize(p, &folder::file_name(path), grotesk_semi(14.5), name_max);
        let r = p.text(
            pos2(x, cy),
            Align2::LEFT_CENTER,
            name,
            grotesk_semi(14.5),
            ink(),
        );
        x = r.right() + 14.0;
        for t in pills {
            let w = pill_width(p, &t);
            if x + w > right_limit {
                break;
            }
            pill(p, pos2(x, cy), &t, ink());
            x += w + 6.0;
        }
    }
}
