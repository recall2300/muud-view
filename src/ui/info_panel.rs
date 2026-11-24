//! The Tab info panel, drawn as a spec sheet.

use std::path::Path;

use eframe::egui::{self, pos2, vec2, Align2, CornerRadius, Pos2, Rect, Shape, Stroke, StrokeKind};

use crate::app::MuudApp;
use crate::core::decode::Decoded;
use crate::core::folder;
use crate::ui::theme::*;

impl MuudApp {
    pub(crate) fn info_panel(&self, ui: &mut egui::Ui) {
        let (Some(d), Some(path)) = (self.lib.current_decoded(), self.lib.current()) else {
            return;
        };
        let outer = ui.max_rect();
        ui.painter().line_segment(
            [
                outer.left_top() + vec2(0.5, 0.0),
                outer.left_bottom() + vec2(0.5, 0.0),
            ],
            hair(),
        );
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let top = ui.cursor().top();
                let left = ui.max_rect().left();
                let width = ui.available_width();
                let p = ui.painter().clone();
                let x0 = left + 16.0;
                let x1 = left + width - 16.0;
                let mut y = top + 18.0;

                // Header: name, pills, colour line.
                let name = p.layout(folder::file_name(path), grotesk(21.0), INK, x1 - x0);
                let h = name.size().y;
                p.galley(pos2(x0, y), name, INK);
                y += h + 12.0;
                let mut x = x0;
                let mp = d.width as f64 * d.height as f64 / 1e6;
                for t in [
                    d.format.to_string(),
                    human_size(d.file_size),
                    format!("{} MP", trim1(mp)),
                ] {
                    x += pill(&p, pos2(x, y + 11.0), &t, INK) + 6.0;
                }
                y += 22.0 + 14.0;
                let r = p.text(
                    pos2(x0, y),
                    Align2::LEFT_TOP,
                    "Color",
                    grotesk_semi(13.0),
                    INK,
                );
                let alpha = if d.has_alpha { "ALPHA" } else { "OPAQUE" };
                p.text(
                    pos2(r.right() + 8.0, y + 1.0),
                    Align2::LEFT_TOP,
                    format!("{} · {}", d.color, alpha),
                    mono(12.0),
                    INK,
                );
                y += 30.0;

                y = self.box_resolution(&p, x0 - 4.0, x1 + 4.0, y, d);
                if let Some(e) = &d.exif {
                    y = box_camera(&p, x0 - 4.0, x1 + 4.0, y, e);
                }
                y = self.box_file(&p, x0 - 4.0, x1 + 4.0, y, d, path);
                y = box_histogram(&p, x0 - 4.0, x1 + 4.0, y, &d.histogram);
                if let Some((lat, lon)) = d.exif.as_ref().and_then(|e| e.gps) {
                    let alt = d.exif.as_ref().and_then(|e| e.altitude.clone());
                    y = box_location(&p, x0 - 4.0, x1 + 4.0, y, lat, lon, alt);
                }
                ui.allocate_space(vec2(width, y - top + 24.0));
            });
    }

    fn box_resolution(&self, p: &egui::Painter, bx0: f32, bx1: f32, y: f32, d: &Decoded) -> f32 {
        let pad = 14.0;
        let mut yy = y + pad;
        section_title(p, pos2(bx0 + pad, yy), "RESOLUTION");
        yy += 30.0;
        let cx = (bx0 + bx1) / 2.0;
        let mp = d.width as f64 * d.height as f64 / 1e6;
        let big = p.layout_no_wrap(trim1(mp), grotesk(54.0), INK);
        let unit = p.layout_no_wrap("MP".into(), mono(24.0), INK);
        let total = big.size().x + 6.0 + unit.size().x;
        let bx = cx - total / 2.0;
        let big_h = big.size().y;
        p.galley(pos2(bx, yy), big.clone(), INK);
        p.galley(
            pos2(bx + big.size().x + 6.0, yy + big_h - unit.size().y - 6.0),
            unit,
            INK,
        );
        yy += big_h + 8.0;
        dotted_rule(p, pos2(bx0 + pad, yy), pos2(bx1 - pad, yy), INK);
        yy += 14.0;
        let mut lines = vec![
            format!("{} × {} PX", d.width, d.height),
            format!("ASPECT {}", aspect(d.width, d.height)),
        ];
        let tex = d.texture_size();
        if tex[0] < d.width as usize {
            lines.push(format!("TEXTURE {} × {}", tex[0], tex[1]));
        }
        lines.push("↓".into());
        lines.push(format!("SHOWN AT {:.0}%", self.last_scale * 100.0));
        for l in lines {
            let r = p.text(pos2(cx, yy), Align2::CENTER_TOP, l, mono(12.5), INK);
            yy = r.bottom() + 6.0;
        }
        yy += pad - 6.0;
        let r = Rect::from_min_max(pos2(bx0, y), pos2(bx1, yy));
        p.rect_stroke(r, CornerRadius::ZERO, hair(), StrokeKind::Inside);
        yy - HAIR
    }

    fn box_file(
        &self,
        p: &egui::Painter,
        bx0: f32,
        bx1: f32,
        y: f32,
        d: &Decoded,
        path: &Path,
    ) -> f32 {
        let pad = 14.0;
        let mut yy = y + pad;
        section_title(p, pos2(bx0 + pad, yy), "FILE");
        yy += 28.0;
        let kx = bx0 + pad;
        let vx = bx0 + pad + 96.0;
        let vw = bx1 - pad - vx;
        let modified = d.modified.map(|t| {
            chrono::DateTime::<chrono::Local>::from(t)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        });
        let folder = path
            .parent()
            .map(|f| f.display().to_string())
            .unwrap_or_default();
        let rows: Vec<(&str, String)> = vec![
            (
                "INDEX",
                format!("{} / {}", self.lib.index + 1, self.lib.len()),
            ),
            ("MODIFIED", modified.unwrap_or_else(|| "—".into())),
            ("DECODE", format!("{} MS", d.decode_time.as_millis())),
            ("FOLDER", folder),
        ];
        for (k, v) in rows {
            yy += kv_row(p, kx, vx, yy, k, &v, vw);
        }
        yy += pad - 3.0;
        p.rect_stroke(
            Rect::from_min_max(pos2(bx0, y), pos2(bx1, yy)),
            CornerRadius::ZERO,
            hair(),
            StrokeKind::Inside,
        );
        yy - HAIR
    }
}

fn trim1(v: f64) -> String {
    crate::core::exif_info::trim_float(v, 1)
}

fn aspect(w: u32, h: u32) -> String {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }
    let g = gcd(w, h).max(1);
    let (a, b) = (w / g, h / g);
    if a <= 32 && b <= 32 {
        format!("{a}:{b}")
    } else {
        format!("{:.2}:1", w as f64 / h.max(1) as f64)
    }
}

fn box_camera(
    p: &egui::Painter,
    bx0: f32,
    bx1: f32,
    y: f32,
    e: &crate::core::exif_info::ExifInfo,
) -> f32 {
    let pad = 14.0;
    let mut yy = y + pad;
    section_title(p, pos2(bx0 + pad, yy), "CAMERA");
    yy += 30.0;
    let kx = bx0 + pad;
    let vx = bx0 + pad + 96.0;
    let vw = bx1 - pad - vx;
    let body: Vec<(&str, Option<String>)> = vec![
        ("MODEL", e.camera()),
        ("LENS", e.lens.clone()),
        ("TAKEN", e.taken.clone()),
    ];
    if body.iter().any(|(_, v)| v.is_some()) {
        yy += sub_label(p, pos2(kx, yy), "BODY") + 6.0;
        for (k, v) in body {
            if let Some(v) = v {
                yy += kv_row(p, kx, vx, yy, k, &v, vw);
            }
        }
        yy += 10.0;
    }
    let focal = match (&e.focal, &e.focal_35) {
        (Some(f), Some(f35)) if f != f35 => Some(format!("{f} ({f35} EQ)")),
        (Some(f), _) => Some(f.clone()),
        (None, f35) => f35.clone(),
    };
    let exposure: Vec<(&str, Option<String>)> = vec![
        ("APERTURE", e.aperture.clone()),
        ("SHUTTER", e.shutter.clone()),
        ("ISO", e.iso.clone()),
        ("FOCAL", focal),
        ("BIAS", e.bias.clone()),
        ("FLASH", e.flash.clone()),
    ];
    if exposure.iter().any(|(_, v)| v.is_some()) {
        yy += sub_label(p, pos2(kx, yy), "EXPOSURE") + 6.0;
        for (k, v) in exposure {
            if let Some(v) = v {
                yy += kv_row(p, kx, vx, yy, k, &v, vw);
            }
        }
    }
    yy += pad - 3.0;
    p.rect_stroke(
        Rect::from_min_max(pos2(bx0, y), pos2(bx1, yy)),
        CornerRadius::ZERO,
        hair(),
        StrokeKind::Inside,
    );
    yy - HAIR
}

fn box_histogram(p: &egui::Painter, bx0: f32, bx1: f32, y: f32, hist: &[u32]) -> f32 {
    let pad = 14.0;
    let mut yy = y + pad;
    section_title(p, pos2(bx0 + pad, yy), "HISTOGRAM");
    yy += 20.0;
    p.text(
        pos2(bx0 + pad, yy),
        Align2::LEFT_TOP,
        "LUMINANCE - 256 BINS - SQRT",
        mono(11.5),
        INK,
    );
    yy += 26.0;

    let chart = Rect::from_min_max(pos2(bx0 + pad, yy), pos2(bx1 - pad, yy + 112.0));
    // Robust maximum: ignore the clipped ends so a spike at 0 or 255 does not flatten the curve.
    let mut sorted: Vec<u32> = hist[1..255].to_vec();
    sorted.sort_unstable();
    let robust = sorted[(sorted.len() as f32 * 0.995) as usize - 1].max(1) as f32;
    let v = |i: usize| ((hist[i] as f32 / robust).min(1.0)).sqrt();
    let plot_top = chart.top() + 18.0;
    let plot_bottom = chart.bottom() - 16.0;
    let x_at = |i: usize| chart.left() + chart.width() * i as f32 / 255.0;
    let y_at = |t: f32| plot_bottom - (plot_bottom - plot_top) * t;

    for (t, label) in [(1.0, "MAX"), (0.5, "25%")] {
        let gy = y_at(t);
        dashed_rule(p, pos2(chart.left(), gy), pos2(chart.right(), gy), INK);
        p.text(
            pos2(chart.left() + 2.0, gy - 2.0),
            Align2::LEFT_BOTTOM,
            label,
            mono(10.0),
            INK,
        );
    }
    // Lightly smoothed curve.
    let pts: Vec<Pos2> = (0..256usize)
        .map(|i| {
            let a = v(i.saturating_sub(1));
            let b = v(i);
            let c = v((i + 1).min(255));
            pos2(x_at(i), y_at((a + 2.0 * b + c) / 4.0))
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(1.4, INK)));

    let peak = (1..255).max_by_key(|&i| hist[i]).unwrap_or(128);
    let px = x_at(peak);
    let py = y_at(v(peak));
    p.circle_filled(pos2(px, py), 3.2, INK);
    p.line_segment(
        [pos2(px, py - 4.0), pos2(px, plot_top - 4.0)],
        Stroke::new(1.0, INK),
    );
    let lbl = format!("{peak} PEAK");
    let align = if peak > 200 {
        Align2::RIGHT_BOTTOM
    } else if peak < 55 {
        Align2::LEFT_BOTTOM
    } else {
        Align2::CENTER_BOTTOM
    };
    p.text(pos2(px, plot_top - 5.0), align, lbl, mono(10.5), INK);

    // Axis.
    p.line_segment(
        [
            pos2(chart.left(), plot_bottom + 1.0),
            pos2(chart.right(), plot_bottom + 1.0),
        ],
        Stroke::new(1.0, INK),
    );
    for (i, label, align) in [
        (0usize, "0", Align2::LEFT_TOP),
        (128, "128", Align2::CENTER_TOP),
        (255, "255", Align2::RIGHT_TOP),
    ] {
        let x = x_at(i);
        p.line_segment(
            [pos2(x, plot_bottom + 1.0), pos2(x, plot_bottom + 5.0)],
            Stroke::new(1.0, INK),
        );
        p.text(pos2(x, plot_bottom + 5.0), align, label, mono(10.5), INK);
    }
    yy = chart.bottom() + pad;
    p.rect_stroke(
        Rect::from_min_max(pos2(bx0, y), pos2(bx1, yy)),
        CornerRadius::ZERO,
        hair(),
        StrokeKind::Inside,
    );
    yy - HAIR
}

fn box_location(
    p: &egui::Painter,
    bx0: f32,
    bx1: f32,
    y: f32,
    lat: f64,
    lon: f64,
    alt: Option<String>,
) -> f32 {
    let pad = 14.0;
    let mut yy = y + pad;
    section_title(p, pos2(bx0 + pad, yy), "LOCATION");
    yy += 30.0;
    // A small crosshair diagram, then coordinates beside a dotted divider.
    let d = Rect::from_min_size(pos2(bx0 + pad, yy), vec2(108.0, 96.0));
    let c = d.center();
    let s = Stroke::new(1.0, INK);
    p.add(Shape::ellipse_stroke(c, vec2(40.0, 26.0), s));
    p.arrow(c, vec2(0.0, -46.0), s);
    p.arrow(c, vec2(0.0, 46.0), s);
    p.arrow(c, vec2(-52.0, 0.0), s);
    p.arrow(c, vec2(52.0, 0.0), s);
    for o in [
        vec2(0.0, 0.0),
        vec2(0.0, -26.0),
        vec2(0.0, 26.0),
        vec2(-40.0, 0.0),
        vec2(40.0, 0.0),
    ] {
        p.circle_stroke(c + o, 5.0, s);
    }
    let div = d.right() + 14.0;
    p.extend(Shape::dotted_line(
        &[pos2(div, d.top()), pos2(div, d.bottom())],
        INK,
        4.0,
        0.7,
    ));
    let tx = div + 12.0;
    let ns = if lat >= 0.0 { "N" } else { "S" };
    let ew = if lon >= 0.0 { "E" } else { "W" };
    let mut ty = d.top() + 4.0;
    let r = p.text(
        pos2(tx, ty),
        Align2::LEFT_TOP,
        "GPS",
        grotesk_semi(13.0),
        INK,
    );
    ty = r.bottom() + 8.0;
    for line in [
        format!("{:.5}° {ns}", lat.abs()),
        format!("{:.5}° {ew}", lon.abs()),
        alt.map(|a| format!("ALT {a}")).unwrap_or_default(),
    ] {
        if !line.is_empty() {
            let r = p.text(pos2(tx, ty), Align2::LEFT_TOP, line, mono(12.0), INK);
            ty = r.bottom() + 4.0;
        }
    }
    let _ = bx1;
    yy = d.bottom() + pad;
    p.rect_stroke(
        Rect::from_min_max(pos2(bx0, y), pos2(bx1, yy)),
        CornerRadius::ZERO,
        hair(),
        StrokeKind::Inside,
    );
    yy - HAIR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_ratios() {
        assert_eq!(aspect(4032, 3024), "4:3");
        assert_eq!(aspect(1920, 1080), "16:9");
        assert_eq!(aspect(1000, 333), "3.00:1");
    }
}
