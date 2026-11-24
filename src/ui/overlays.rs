//! Things drawn above everything: drop hint, help sheet, confirmations, toasts.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, Id, LayerId, Order, Pos2, Rect, Sense, Stroke,
    StrokeKind,
};

use crate::app::{MuudApp, VERSION};
use crate::core::folder;
use crate::core::keymap::{HelpSection, Keymap, HELP};
use crate::core::settings::Osd;
use crate::ui::i18n::{tr, tr_msg};
use crate::ui::theme::*;

impl MuudApp {
    pub(crate) fn overlays(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let p = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("muud-overlay")));
        let dim = Color32::BLACK.gamma_multiply(0.6);

        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if hovering {
            p.rect_filled(screen, CornerRadius::ZERO, dim);
            let card = Rect::from_center_size(screen.center(), vec2(320.0, 120.0));
            p.rect_filled(card, CornerRadius::ZERO, paper());
            p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
            p.text(
                card.center() - vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                tr("DROP TO OPEN"),
                grotesk(24.0),
                ink(),
            );
            p.text(
                card.center() + vec2(0.0, 22.0),
                Align2::CENTER_CENTER,
                "↓",
                mono(14.0),
                ink(),
            );
        }

        if self.show_help {
            p.rect_filled(screen, CornerRadius::ZERO, dim);
            help_sheet(&p, screen, &self.keymap);
            if ctx.input(|i| i.pointer.any_pressed()) {
                self.show_help = false;
            }
        }

        if self.confirm_delete {
            p.rect_filled(screen, CornerRadius::ZERO, dim);
            let name = self
                .lib
                .current()
                .map(|c| folder::file_name(c))
                .unwrap_or_default();
            message_card(
                &p,
                screen.center(),
                tr("DELETE PERMANENTLY?"),
                &name,
                tr("ENTER  DELETE      ESC  CANCEL"),
            );
        }

        if self.show_folders {
            self.folders_sheet(ctx, screen);
        }
        if self.rename.is_some() {
            self.rename_sheet(ctx, screen);
        }

        if let Some((text, since)) = &self.toast {
            let age = since.elapsed().as_secs_f32();
            let alpha = if age > 1.4 {
                ((1.8 - age) / 0.4).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let galley =
                p.layout_no_wrap(tr_msg(text), mono_medium(12.0), ink().gamma_multiply(alpha));
            let size = galley.size() + vec2(28.0, 14.0);
            let r = Rect::from_center_size(pos2(screen.center().x, screen.bottom() - 48.0), size);
            p.rect_filled(r, CornerRadius::same(255), paper().gamma_multiply(alpha));
            p.rect_stroke(
                r,
                CornerRadius::same(255),
                Stroke::new(1.0, ink().gamma_multiply(alpha)),
                StrokeKind::Inside,
            );
            p.galley(r.min + vec2(14.0, 7.0), galley, ink());
        }
    }

    /// Small file name / details over the image when the top bar is hidden.
    pub(crate) fn osd(&self, p: &egui::Painter, canvas: Rect) {
        let (Some(path), true) = (self.lib.current(), self.settings.osd != Osd::Off) else {
            return;
        };
        let mut text = format!(
            "{:03} / {:03}   {}",
            self.lib.index + 1,
            self.lib.len(),
            folder::file_name(path)
        );
        if self.settings.osd == Osd::Full {
            if let Some(d) = self.lib.current_decoded() {
                text.push_str(&format!(
                    "   {} × {}   {:.0}%",
                    d.width,
                    d.height,
                    self.last_scale * 100.0
                ));
            }
        }
        let fg = self.on_canvas_color();
        let galley = p.layout_no_wrap(text, mono(11.5), fg);
        let pos = canvas.left_top() + vec2(14.0, 12.0);
        let bg = Rect::from_min_size(pos - vec2(6.0, 3.0), galley.size() + vec2(12.0, 6.0));
        p.rect_filled(
            bg,
            CornerRadius::same(4),
            self.canvas_color().gamma_multiply(0.7),
        );
        p.galley(pos, galley, fg);
    }

    fn folders_sheet(&mut self, ctx: &egui::Context, screen: Rect) {
        let w = 600.0f32.min(screen.width() - 32.0);
        let favs = self.settings.favorite_folders.clone();
        let recent: Vec<String> = self
            .settings
            .recent_folders
            .iter()
            .filter(|r| !favs.contains(r))
            .cloned()
            .collect();
        let rows = favs.len() + recent.len();
        let h = (150.0 + rows as f32 * 28.0 + if favs.is_empty() { 0.0 } else { 30.0 })
            .min(screen.height() - 32.0);
        let card = Rect::from_center_size(screen.center(), vec2(w, h));
        let mut open: Option<String> = None;
        egui::Area::new(Id::unique("folders-sheet"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (_, backdrop) = ui.allocate_exact_size(screen.size(), Sense::click());
                let p = ui.painter().clone();
                p.rect_filled(
                    screen,
                    CornerRadius::ZERO,
                    Color32::BLACK.gamma_multiply(0.6),
                );
                p.rect_filled(card, CornerRadius::ZERO, paper());
                p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
                let x0 = card.left() + 24.0;
                let r = p.text(
                    pos2(x0, card.top() + 18.0),
                    Align2::LEFT_TOP,
                    tr("RECENT FOLDERS"),
                    grotesk(26.0),
                    ink(),
                );
                p.text(
                    pos2(card.right() - 24.0, r.center().y),
                    Align2::RIGHT_CENTER,
                    tr("S  STAR THIS FOLDER"),
                    mono(11.0),
                    muted(),
                );
                let mut y = r.bottom() + 14.0;
                p.line_segment([pos2(card.left(), y), pos2(card.right(), y)], hair());
                y += 10.0;
                let mut n = 0usize;
                for (title, list) in [("FAVORITES", &favs), ("RECENT", &recent)] {
                    if list.is_empty() {
                        continue;
                    }
                    sub_label(&p, pos2(x0, y + 4.0), tr(title));
                    y += 28.0;
                    for f in list.iter() {
                        n += 1;
                        let row =
                            Rect::from_min_size(pos2(card.left() + 12.0, y), vec2(w - 24.0, 26.0));
                        let resp = ui.interact(
                            row,
                            Id::unique(("folder-row", f.as_str())),
                            Sense::click(),
                        );
                        let (bg, fg) = if resp.hovered() {
                            (ink(), paper())
                        } else {
                            (Color32::TRANSPARENT, ink())
                        };
                        let p = ui.painter();
                        p.rect_filled(row, CornerRadius::ZERO, bg);
                        let key = if n <= 9 { n.to_string() } else { " ".into() };
                        p.text(
                            pos2(x0, row.center().y),
                            Align2::LEFT_CENTER,
                            key,
                            mono_medium(12.0),
                            fg,
                        );
                        let label = ellipsize(p, f, mono(12.0), w - 90.0);
                        p.text(
                            pos2(x0 + 28.0, row.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            mono(12.0),
                            fg,
                        );
                        if resp.clicked() {
                            open = Some(f.clone());
                        }
                        y += 28.0;
                    }
                }
                if n == 0 {
                    p.text(
                        pos2(x0, y + 10.0),
                        Align2::LEFT_TOP,
                        tr("NO FOLDERS YET"),
                        mono(12.0),
                        muted(),
                    );
                }
                let outside = ui
                    .ctx()
                    .input(|i| i.pointer.hover_pos().is_some_and(|p| !card.contains(p)));
                if backdrop.clicked() && outside {
                    self.show_folders = false;
                }
            });
        if let Some(f) = open {
            self.show_folders = false;
            self.open(std::path::Path::new(&f));
        }
    }

    /// Ordered list the folder sheet shows (favorites first), for number keys.
    pub(crate) fn folder_list(&self) -> Vec<String> {
        let favs = &self.settings.favorite_folders;
        favs.iter()
            .cloned()
            .chain(
                self.settings
                    .recent_folders
                    .iter()
                    .filter(|r| !favs.contains(r))
                    .cloned(),
            )
            .collect()
    }

    fn rename_sheet(&mut self, ctx: &egui::Context, screen: Rect) {
        let w = 480.0f32.min(screen.width() - 32.0);
        let card = Rect::from_center_size(screen.center(), vec2(w, 170.0));
        egui::Area::new(Id::unique("rename-sheet"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.allocate_exact_size(screen.size(), Sense::click());
                let p = ui.painter().clone();
                p.rect_filled(
                    screen,
                    CornerRadius::ZERO,
                    Color32::BLACK.gamma_multiply(0.6),
                );
                p.rect_filled(card, CornerRadius::ZERO, paper());
                p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
                let x0 = card.left() + 24.0;
                p.text(
                    pos2(x0, card.top() + 20.0),
                    Align2::LEFT_TOP,
                    tr("RENAME"),
                    grotesk(24.0),
                    ink(),
                );
                let field = Rect::from_min_size(pos2(x0, card.top() + 66.0), vec2(w - 48.0, 30.0));
                p.rect_stroke(field, CornerRadius::ZERO, hair(), StrokeKind::Inside);
                if let Some(text) = &mut self.rename {
                    let edit = egui::TextEdit::singleline(text)
                        .font(mono(13.0))
                        .text_color(ink())
                        .frame(egui::Frame::NONE)
                        .desired_width(field.width() - 16.0);
                    let resp = ui.put(field.shrink2(vec2(8.0, 5.0)), edit);
                    resp.request_focus();
                }
                p.text(
                    pos2(x0, card.bottom() - 28.0),
                    Align2::LEFT_CENTER,
                    tr("ENTER  RENAME      ESC  CANCEL"),
                    mono(11.0),
                    muted(),
                );
            });
    }
}

pub(crate) fn message_card(p: &egui::Painter, center: Pos2, title: &str, name: &str, detail: &str) {
    let w = 420.0;
    let name = ellipsize(p, name, mono(12.0), w - 48.0);
    let detail_g = p.layout(detail.to_owned(), mono(12.0), ink(), w - 48.0);
    let h = 132.0 + detail_g.size().y;
    let card = Rect::from_center_size(center, vec2(w, h));
    p.rect_filled(card, CornerRadius::ZERO, paper());
    p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
    let x0 = card.left() + 24.0;
    let mut y = card.top() + 22.0;
    let r = p.text(pos2(x0, y), Align2::LEFT_TOP, title, grotesk(24.0), ink());
    y = r.bottom() + 10.0;
    p.text(pos2(x0, y), Align2::LEFT_TOP, name, mono(12.0), ink());
    y += 26.0;
    dotted_rule(p, pos2(x0, y), pos2(card.right() - 24.0, y), ink());
    y += 14.0;
    p.galley(pos2(x0, y), detail_g, ink());
}

fn help_sheet(p: &egui::Painter, screen: Rect, keymap: &Keymap) {
    let w = 820.0f32.min(screen.width() - 32.0);
    let col_w = (w - 24.0 * 2.0 - 24.0) / 2.0;
    let row_h = 19.0;
    let section_h = |rows: usize| 28.0 + rows as f32 * row_h + 18.0;
    // Fill the shorter column with each next section.
    let mut cols: [Vec<&HelpSection>; 2] = [Vec::new(), Vec::new()];
    let mut heights = [0.0f32; 2];
    for s in HELP {
        let c = if heights[0] <= heights[1] { 0 } else { 1 };
        heights[c] += section_h(s.1.len());
        cols[c].push(s);
    }
    let h = (96.0 + heights[0].max(heights[1])).min(screen.height() - 32.0);
    let card = Rect::from_center_size(screen.center(), vec2(w, h));
    p.rect_filled(card, CornerRadius::ZERO, paper());
    p.rect_stroke(card, CornerRadius::ZERO, hair(), StrokeKind::Inside);
    let p = p.with_clip_rect(card);
    let x0 = card.left() + 24.0;
    let r = p.text(
        pos2(x0, card.top() + 20.0),
        Align2::LEFT_TOP,
        tr("SHORTCUTS"),
        grotesk(32.0),
        ink(),
    );
    let mut x = r.right() + 14.0;
    for t in [format!("v{VERSION}"), tr("ANY KEY TO CLOSE").to_string()] {
        x += pill(&p, pos2(x, r.center().y), &t, ink()) + 6.0;
    }
    let y = r.bottom() + 16.0;
    p.line_segment([pos2(card.left(), y), pos2(card.right(), y)], hair());
    let mid = card.left() + w / 2.0;
    p.line_segment([pos2(mid, y), pos2(mid, card.bottom())], hair());
    for (col, items) in cols.iter().enumerate() {
        let cx = if col == 0 { x0 } else { mid + 24.0 };
        let mut cy = y + 16.0;
        for (title, rows) in items.iter() {
            sub_label(&p, pos2(cx, cy), tr(title));
            cy += 26.0;
            for (cmds, action) in rows.iter() {
                let keys = ellipsize(&p, &keymap.short_keys(cmds), mono(11.5), col_w * 0.58);
                p.text(pos2(cx, cy), Align2::LEFT_TOP, keys, mono(11.5), ink());
                p.text(
                    pos2(cx + col_w, cy),
                    Align2::RIGHT_TOP,
                    tr(action),
                    mono(11.5),
                    ink(),
                );
                cy += row_h;
            }
            cy += 18.0;
        }
    }
}
