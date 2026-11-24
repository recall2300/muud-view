//! Zoom, pan and orientation math, kept free of UI code so it can be tested.

use eframe::egui::{pos2, Pos2, Rect, Vec2};

pub const MIN_SCALE: f32 = 0.02;
pub const MAX_SCALE: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoomMode {
    Fit,
    FitWidth,
    Original,
    Free(f32),
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub mode: ZoomMode,
    /// Image centre relative to the canvas centre, in points.
    pub offset: Vec2,
    /// Clockwise quarter turns, 0..=3.
    pub turns: u8,
    pub flip_h: bool,
    pub flip_v: bool,
    /// Align the top edge on the next layout (fit-to-width starts at the top).
    pub align_top: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            mode: ZoomMode::Fit,
            offset: Vec2::ZERO,
            turns: 0,
            flip_h: false,
            flip_v: false,
            align_top: false,
        }
    }
}

impl View {
    /// Image size in image pixels after rotation.
    pub fn rotated(&self, w: f32, h: f32) -> Vec2 {
        if self.turns % 2 == 1 {
            Vec2::new(h, w)
        } else {
            Vec2::new(w, h)
        }
    }

    /// Scale where 1.0 shows one image pixel per screen pixel.
    pub fn scale(&self, img: Vec2, canvas: Vec2, ppp: f32, stretch_small: bool) -> f32 {
        let canvas_px = canvas * ppp;
        let cap = |s: f32| if stretch_small { s } else { s.min(1.0) };
        let s = match self.mode {
            ZoomMode::Fit => cap((canvas_px.x / img.x).min(canvas_px.y / img.y)),
            ZoomMode::FitWidth => cap(canvas_px.x / img.x),
            ZoomMode::Original => 1.0,
            ZoomMode::Free(s) => s,
        };
        if s.is_finite() {
            s.clamp(MIN_SCALE, MAX_SCALE)
        } else {
            1.0
        }
    }

    pub fn rotate(&mut self, quarter_turns_cw: i32) {
        self.turns = ((self.turns as i32 + quarter_turns_cw).rem_euclid(4)) as u8;
        self.offset = Vec2::ZERO;
    }
}

/// Keeps a too-large image from leaving gaps at the canvas edges and centres
/// an image that is smaller than the canvas.
pub fn clamp_offset(offset: Vec2, shown: Vec2, canvas: Vec2) -> Vec2 {
    let axis = |o: f32, s: f32, c: f32| {
        if s <= c {
            0.0
        } else {
            let lim = (s - c) / 2.0;
            o.clamp(-lim, lim)
        }
    };
    Vec2::new(
        axis(offset.x, shown.x, canvas.x),
        axis(offset.y, shown.y, canvas.y),
    )
}

/// New offset after changing scale so that the point under `anchor` stays put.
pub fn zoom_about(
    offset: Vec2,
    canvas_center: Pos2,
    anchor: Pos2,
    old_scale: f32,
    new_scale: f32,
) -> Vec2 {
    let center = canvas_center + offset;
    let rel = (anchor - center) / old_scale;
    let new_center = anchor - rel * new_scale;
    new_center - canvas_center
}

/// Next zoom step for `+`/`-`, snapping through 100%.
pub fn step_zoom(scale: f32, zoom_in: bool) -> f32 {
    const STEPS: &[f32] = &[
        0.02, 0.05, 0.1, 0.15, 0.2, 0.25, 0.33, 0.5, 0.67, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0,
        6.0, 8.0, 12.0, 16.0, 24.0, 32.0, 40.0,
    ];
    if zoom_in {
        STEPS
            .iter()
            .copied()
            .find(|s| *s > scale * 1.001)
            .unwrap_or(MAX_SCALE)
    } else {
        STEPS
            .iter()
            .rev()
            .copied()
            .find(|s| *s < scale * 0.999)
            .unwrap_or(MIN_SCALE)
    }
}

/// Texture coordinates for the four corners of the destination rectangle
/// (top-left, top-right, bottom-right, bottom-left), after rotating clockwise
/// and then flipping on screen.
pub fn corner_uvs(turns: u8, flip_h: bool, flip_v: bool) -> [Pos2; 4] {
    let src = [
        pos2(0.0, 0.0),
        pos2(1.0, 0.0),
        pos2(1.0, 1.0),
        pos2(0.0, 1.0),
    ];
    let k = (turns % 4) as usize;
    let mut uv = [src[0]; 4];
    for (i, slot) in uv.iter_mut().enumerate() {
        *slot = src[(i + 4 - k) % 4];
    }
    if flip_h {
        uv.swap(0, 1);
        uv.swap(2, 3);
    }
    if flip_v {
        uv.swap(0, 3);
        uv.swap(1, 2);
    }
    uv
}

pub fn corners(rect: Rect) -> [Pos2; 4] {
    [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_never_upscales_unless_asked() {
        let v = View::default();
        let s = v.scale(
            Vec2::new(100.0, 50.0),
            Vec2::new(1000.0, 1000.0),
            1.0,
            false,
        );
        assert_eq!(s, 1.0);
        let s = v.scale(Vec2::new(100.0, 50.0), Vec2::new(1000.0, 1000.0), 1.0, true);
        assert_eq!(s, 10.0);
    }

    #[test]
    fn fit_uses_physical_pixels() {
        let v = View::default();
        // A 4000 px wide image in a 1000 pt canvas at 200% DPI: 2000 px / 4000 px.
        let s = v.scale(
            Vec2::new(4000.0, 1000.0),
            Vec2::new(1000.0, 1000.0),
            2.0,
            false,
        );
        assert!((s - 0.5).abs() < 1e-6);
    }

    #[test]
    fn rotation_swaps_size() {
        let mut v = View::default();
        v.rotate(1);
        assert_eq!(v.rotated(30.0, 10.0), Vec2::new(10.0, 30.0));
        v.rotate(-2);
        assert_eq!(v.turns, 3);
    }

    #[test]
    fn clamp_centres_small_images() {
        let o = clamp_offset(
            Vec2::new(50.0, -50.0),
            Vec2::new(100.0, 400.0),
            Vec2::new(200.0, 200.0),
        );
        assert_eq!(o, Vec2::new(0.0, -50.0));
        let o = clamp_offset(
            Vec2::new(0.0, 500.0),
            Vec2::new(100.0, 400.0),
            Vec2::new(200.0, 200.0),
        );
        assert_eq!(o.y, 100.0);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let c = pos2(100.0, 100.0);
        let anchor = pos2(150.0, 120.0);
        let off = zoom_about(Vec2::ZERO, c, anchor, 1.0, 2.0);
        // Image point under the anchor was (50, 20) from the centre; now 100, 40.
        assert_eq!(c + off + Vec2::new(100.0, 40.0), anchor);
    }

    #[test]
    fn zoom_steps_through_100() {
        assert_eq!(step_zoom(0.8, true), 1.0);
        assert_eq!(step_zoom(1.0, true), 1.25);
        assert_eq!(step_zoom(1.0, false), 0.75);
    }

    #[test]
    fn uv_rotation() {
        let uv = corner_uvs(1, false, false);
        // Rotated clockwise: the source's bottom-left shows at the top-left.
        assert_eq!(uv[0], pos2(0.0, 1.0));
        assert_eq!(uv[1], pos2(0.0, 0.0));
        let uv = corner_uvs(0, true, false);
        assert_eq!(uv[0], pos2(1.0, 0.0));
        let uv = corner_uvs(2, false, false);
        assert_eq!(uv[0], pos2(1.0, 1.0));
    }
}
