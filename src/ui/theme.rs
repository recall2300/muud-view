//! The "spec sheet" look: monochrome, bold grotesk headings, monospaced data,
//! hairline borders, dotted rules and outlined pill tags.

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect,
    Shape, Stroke, StrokeKind, Vec2,
};

pub const PAPER: Color32 = Color32::from_rgb(0xF4, 0xF3, 0xEF);
pub const INK: Color32 = Color32::from_rgb(0x0D, 0x0D, 0x0D);
pub const MUTED: Color32 = Color32::from_rgb(0x6E, 0x6D, 0x68);
pub const CANVAS_INK: Color32 = Color32::from_rgb(0x12, 0x12, 0x12);
pub const CANVAS_GRAY: Color32 = Color32::from_rgb(0x5A, 0x5A, 0x57);

pub const HAIR: f32 = 1.0;

pub fn hair() -> Stroke {
    Stroke::new(HAIR, INK)
}

const GROTESK: &str = "grotesk";
const GROTESK_SEMI: &str = "grotesk-semi";
const MONO_MEDIUM: &str = "mono-medium";

pub fn grotesk(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(GROTESK.into()))
}
pub fn grotesk_semi(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(GROTESK_SEMI.into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
pub fn mono_medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MONO_MEDIUM.into()))
}

/// Loads the bundled OFL fonts, with a system Hangul font as fallback so
/// Korean file names render.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, data: FontData| {
        fonts.font_data.insert(name.to_owned(), Arc::new(data));
    };
    add(
        &mut fonts,
        "inter-tight-800",
        FontData::from_static(include_bytes!(
            "../../assets/fonts/InterTight-ExtraBold.ttf"
        )),
    );
    add(
        &mut fonts,
        "inter-tight-600",
        FontData::from_static(include_bytes!("../../assets/fonts/InterTight-SemiBold.ttf")),
    );
    add(
        &mut fonts,
        "plex-mono-400",
        FontData::from_static(include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf")),
    );
    add(
        &mut fonts,
        "plex-mono-500",
        FontData::from_static(include_bytes!("../../assets/fonts/IBMPlexMono-Medium.ttf")),
    );

    let (hangul, hangul_bold) = load_hangul_fonts();
    let mut fallback = Vec::new();
    let mut fallback_bold = Vec::new();
    if let Some(f) = hangul {
        add(&mut fonts, "hangul", f);
        fallback.push("hangul".to_owned());
    }
    if let Some(f) = hangul_bold {
        add(&mut fonts, "hangul-bold", f);
        fallback_bold.push("hangul-bold".to_owned());
    } else {
        fallback_bold.extend(fallback.iter().cloned());
    }
    // egui's own fonts cover arrows and symbols.
    let defaults: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let chain = |first: &str, hangul: &[String]| -> Vec<String> {
        let mut v = vec![first.to_owned()];
        v.extend(hangul.iter().cloned());
        v.extend(defaults.iter().cloned());
        v
    };
    fonts.families.insert(
        FontFamily::Name(GROTESK.into()),
        chain("inter-tight-800", &fallback_bold),
    );
    fonts.families.insert(
        FontFamily::Name(GROTESK_SEMI.into()),
        chain("inter-tight-600", &fallback_bold),
    );
    fonts.families.insert(
        FontFamily::Name(MONO_MEDIUM.into()),
        chain("plex-mono-500", &fallback),
    );
    fonts
        .families
        .insert(FontFamily::Monospace, chain("plex-mono-400", &fallback));
    fonts
        .families
        .insert(FontFamily::Proportional, chain("plex-mono-400", &fallback));
    ctx.set_fonts(fonts);
}

fn load_hangul_fonts() -> (Option<FontData>, Option<FontData>) {
    let load = |path: &str, index: u32| -> Option<FontData> {
        let bytes = std::fs::read(path).ok()?;
        let mut f = FontData::from_owned(bytes);
        f.index = index;
        Some(f)
    };
    let mut regular = None;
    let mut bold = None;
    if cfg!(windows) {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
        regular = load(&format!(r"{windir}\Fonts\malgun.ttf"), 0);
        bold = load(&format!(r"{windir}\Fonts\malgunbd.ttf"), 0);
    } else {
        for (r, b, i) in [
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
                1,
            ),
            (
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc",
                1,
            ),
            (
                "/System/Library/Fonts/AppleSDGothicNeo.ttc",
                "/System/Library/Fonts/AppleSDGothicNeo.ttc",
                0,
            ),
        ] {
            if regular.is_none() {
                regular = load(r, i);
                bold = load(b, i);
            }
        }
    }
    (regular, bold)
}

pub fn apply_style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    ctx.global_style_mut(|style| {
        style.visuals.panel_fill = PAPER;
        style.visuals.window_fill = PAPER;
        style.visuals.extreme_bg_color = PAPER;
        style.visuals.override_text_color = Some(INK);
        style.visuals.window_stroke = hair();
        style.visuals.window_corner_radius = CornerRadius::ZERO;
        style.visuals.selection.bg_fill = INK;
        style.visuals.selection.stroke = Stroke::new(1.0, PAPER);
        style.spacing.scroll.bar_width = 4.0;
        style.spacing.scroll.floating = true;
        style.interaction.selectable_labels = false;
    });
}

// ---------------------------------------------------------------------------
// Painting helpers. Everything is drawn with a `Painter`, not widgets, so the
// layout stays exact.

/// Outlined, fully rounded tag with monospaced text. Returns its width.
pub fn pill(painter: &egui::Painter, left_center: Pos2, text: &str, color: Color32) -> f32 {
    let galley = painter.layout_no_wrap(text.to_owned(), mono(11.5), color);
    let pad = Vec2::new(10.0, 4.0);
    let size = galley.size() + pad * 2.0;
    let rect = Rect::from_min_size(Pos2::new(left_center.x, left_center.y - size.y / 2.0), size);
    painter.rect_stroke(
        rect,
        CornerRadius::same(255),
        Stroke::new(1.0, color),
        StrokeKind::Inside,
    );
    painter.galley(rect.min + pad, galley, color);
    size.x
}

/// Width a pill would take.
pub fn pill_width(painter: &egui::Painter, text: &str) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), mono(11.5), INK)
        .size()
        .x
        + 20.0
}

pub fn dotted_rule(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32) {
    painter.extend(Shape::dotted_line(&[a, b], color, 4.0, 0.7));
}

pub fn dashed_rule(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32) {
    painter.extend(Shape::dashed_line(
        &[a, b],
        Stroke::new(1.0, color),
        3.0,
        3.0,
    ));
}

/// Section title in bold grotesk, e.g. "BATTERY LIFE".
pub fn section_title(painter: &egui::Painter, pos: Pos2, text: &str) -> f32 {
    let r = painter.text(pos, Align2::LEFT_TOP, text, grotesk(15.0), INK);
    r.height()
}

/// Small label with a down arrow, e.g. "MATERIALS ↓".
pub fn sub_label(painter: &egui::Painter, pos: Pos2, text: &str) -> f32 {
    let r = painter.text(
        pos,
        Align2::LEFT_TOP,
        format!("{text} ↓"),
        grotesk_semi(11.5),
        INK,
    );
    r.height()
}

/// Two-column monospaced key/value row. Returns the row height.
pub fn kv_row(
    painter: &egui::Painter,
    left: f32,
    value_x: f32,
    y: f32,
    key: &str,
    value: &str,
    max_w: f32,
) -> f32 {
    painter.text(Pos2::new(left, y), Align2::LEFT_TOP, key, mono(12.5), INK);
    let galley = painter.layout(value.to_owned(), mono(12.5), INK, max_w.max(40.0));
    let h = galley.size().y.max(17.0);
    painter.galley(Pos2::new(value_x, y), galley, INK);
    h + 3.0
}

/// Truncates `text` with an ellipsis so it fits `max_width` in `font`.
pub fn ellipsize(painter: &egui::Painter, text: &str, font: FontId, max_width: f32) -> String {
    let width = |s: &str| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), INK)
            .size()
            .x
    };
    if width(text) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars[..mid].iter().collect::<String>() + "…";
        if width(&s) <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[u])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2_500_000), "2.4 MB");
        assert_eq!(human_size(150 * 1024 * 1024), "150 MB");
    }
}
