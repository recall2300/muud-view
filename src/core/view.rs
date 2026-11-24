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

/// Like [`corner_uvs`], but first applies the image's own EXIF orientation.
pub fn compose_uvs(
    base: crate::core::decode::Orientation,
    turns: u8,
    flip_h: bool,
    flip_v: bool,
) -> [Pos2; 4] {
    let base_uvs = corner_uvs(base.turns, base.flip, false);
    let user = corner_indices(turns, flip_h, flip_v);
    [
        base_uvs[user[0]],
        base_uvs[user[1]],
        base_uvs[user[2]],
        base_uvs[user[3]],
    ]
}

/// For each destination corner (TL, TR, BR, BL), which source corner shows there.
fn corner_indices(turns: u8, flip_h: bool, flip_v: bool) -> [usize; 4] {
    let k = (turns % 4) as usize;
    let mut idx = [0usize; 4];
    for (i, slot) in idx.iter_mut().enumerate() {
        *slot = (i + 4 - k) % 4;
    }
    if flip_h {
        idx.swap(0, 1);
        idx.swap(2, 3);
    }
    if flip_v {
        idx.swap(0, 3);
        idx.swap(1, 2);
    }
    idx
}

/// Where a piece of the texture lands on screen. `dest` are the screen
/// corners (TL, TR, BR, BL) of the whole image and `uvs` the texture
/// coordinates shown there (from [`compose_uvs`]); `part` is the piece in
/// texture coordinates (0..1). Returns the screen positions of the piece's
/// corners at texture (min.x, min.y), (max.x, min.y), (max.x, max.y),
/// (min.x, max.y) — draw them with UVs (0,0), (1,0), (1,1), (0,1).
pub fn place_part(dest: [Pos2; 4], uvs: [Pos2; 4], part: Rect) -> [Pos2; 4] {
    // Texture -> screen is affine: solve it from three corners.
    let du1 = uvs[1] - uvs[0];
    let du3 = uvs[3] - uvs[0];
    let dp1 = dest[1] - dest[0];
    let dp3 = dest[3] - dest[0];
    let det = du1.x * du3.y - du3.x * du1.y;
    let map = |uv: Pos2| {
        let d = uv - uvs[0];
        // Coordinates of d in the (du1, du3) basis.
        let a = (d.x * du3.y - du3.x * d.y) / det;
        let b = (du1.x * d.y - d.x * du1.y) / det;
        dest[0] + dp1 * a + dp3 * b
    };
    [
        map(part.min),
        map(pos2(part.max.x, part.min.y)),
        map(part.max),
        map(pos2(part.min.x, part.max.y)),
    ]
}

/// The part of the drawn image that is on screen, in 0..1 of the image as
/// drawn (upright, after rotation), for the minimap.
pub fn visible_part(dest: Rect, canvas: Rect) -> Rect {
    let v = dest.intersect(canvas);
    let n = |p: Pos2| {
        pos2(
            ((p.x - dest.min.x) / dest.width()).clamp(0.0, 1.0),
            ((p.y - dest.min.y) / dest.height()).clamp(0.0, 1.0),
        )
    };
    Rect::from_min_max(n(v.min), n(v.max))
}

/// The pan offset that puts the point at `at` (0..1 of the drawn image) in
/// the middle of the canvas.
pub fn offset_to_center(at: Pos2, shown: Vec2) -> Vec2 {
    Vec2::new((0.5 - at.x) * shown.x, (0.5 - at.y) * shown.y)
}

/// Minimap size: the image's shape within a `max`×`max` box.
pub fn minimap_size(shown: Vec2, max: f32) -> Vec2 {
    let s = max / shown.x.max(shown.y).max(1.0);
    Vec2::new((shown.x * s).max(8.0), (shown.y * s).max(8.0))
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

    #[test]
    fn parts_land_where_the_whole_image_puts_them() {
        let dest = corners(Rect::from_min_size(
            pos2(10.0, 20.0),
            Vec2::new(200.0, 100.0),
        ));
        let right_half = Rect::from_min_max(pos2(0.5, 0.0), pos2(1.0, 1.0));
        // Unrotated: the right half of the texture is the right half on screen.
        let q = place_part(dest, corner_uvs(0, false, false), right_half);
        assert_eq!(q[0], pos2(110.0, 20.0));
        assert_eq!(q[2], pos2(210.0, 120.0));
        // Mirrored: it moves to the left half, drawn right-to-left.
        let q = place_part(dest, corner_uvs(0, true, false), right_half);
        assert_eq!(q[0], pos2(110.0, 20.0));
        assert_eq!(q[1], pos2(10.0, 20.0));
        // Rotated clockwise (screen is 200x100, texture is portrait): the
        // texture's right half becomes the bottom half on screen.
        let q = place_part(dest, corner_uvs(1, false, false), right_half);
        let ys: Vec<f32> = q.iter().map(|p| p.y).collect();
        assert!(
            ys.iter().all(|y| (70.0 - 0.01..=120.01).contains(y)),
            "{q:?}"
        );
        // The whole texture maps back onto the destination corners.
        let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let uvs = corner_uvs(3, true, false);
        let q = place_part(dest, uvs, whole);
        for (corner, uv) in [
            pos2(0.0, 0.0),
            pos2(1.0, 0.0),
            pos2(1.0, 1.0),
            pos2(0.0, 1.0),
        ]
        .iter()
        .enumerate()
        {
            let i = uvs.iter().position(|u| u == uv).unwrap();
            assert_eq!(q[corner], dest[i]);
        }
    }

    #[test]
    fn minimap_shows_the_visible_part() {
        let canvas = Rect::from_min_size(pos2(0.0, 0.0), Vec2::new(100.0, 100.0));
        // Image drawn 400x200, centred: the middle quarter (x) and half (y).
        let shown = Vec2::new(400.0, 200.0);
        let dest = Rect::from_center_size(canvas.center(), shown);
        let v = visible_part(dest, canvas);
        assert_eq!(v, Rect::from_min_max(pos2(0.375, 0.25), pos2(0.625, 0.75)));
        // Clicking the right edge of the minimap pans there.
        let off = offset_to_center(pos2(0.875, 0.5), shown);
        let dest = Rect::from_center_size(canvas.center() + off, shown);
        let v = visible_part(dest, canvas);
        assert!((v.center().x - 0.875).abs() < 1e-6);
        assert_eq!(minimap_size(shown, 160.0), Vec2::new(160.0, 80.0));
    }

    #[test]
    fn exif_orientation_composes_with_user_rotation() {
        use crate::core::decode::Orientation;
        let base = Orientation {
            turns: 1,
            flip: false,
        };
        // EXIF says rotate CW; user rotates CCW: back to the stored pixels.
        assert_eq!(
            compose_uvs(base, 3, false, false),
            corner_uvs(0, false, false)
        );
        // No user change: same as the EXIF rotation alone.
        assert_eq!(
            compose_uvs(base, 0, false, false),
            corner_uvs(1, false, false)
        );
    }
}
