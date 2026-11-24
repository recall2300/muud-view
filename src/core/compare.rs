//! Comparing two images (QA, design reviews): side by side, a slider over
//! both, or the pixels that differ. The UI lives in `ui/canvas.rs`; the
//! maths is here so it is tested without a window.

use eframe::egui::{Color32, ColorImage, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareMode {
    SideBySide,
    Slider,
    Difference,
}

impl CompareMode {
    pub fn next(self) -> Self {
        match self {
            CompareMode::SideBySide => CompareMode::Slider,
            CompareMode::Slider => CompareMode::Difference,
            CompareMode::Difference => CompareMode::SideBySide,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CompareMode::SideBySide => "SIDE BY SIDE",
            CompareMode::Slider => "SLIDER",
            CompareMode::Difference => "DIFFERENCE",
        }
    }
}

/// Size to draw image B at so it lines up with A (drawn at `shown_a`): same
/// width, B's own shape. Two screenshots of one screen then match exactly,
/// and a 2x export lines up with its 1x original.
pub fn match_width(shown_a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(shown_a.x, shown_a.x * b.y / b.x.max(1.0))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffStats {
    /// Share of pixels that differ at all (0..1).
    pub changed: f32,
    /// Largest difference in any channel (0..255).
    pub max: u8,
}

/// Pixel difference of two upright images, at A's size (B is sampled to
/// match). Unchanged pixels show A dimmed to grey for orientation; changed
/// ones are red, brighter for bigger differences.
pub fn diff(a: &ColorImage, b: &ColorImage) -> (ColorImage, DiffStats) {
    let [aw, ah] = a.size;
    let [bw, bh] = b.size;
    let mut out = Vec::with_capacity(aw * ah);
    let mut changed = 0usize;
    let mut max = 0u8;
    let bx: Vec<usize> = (0..aw).map(|x| (x * bw / aw.max(1)).min(bw - 1)).collect();
    for y in 0..ah {
        let by = (y * bh / ah.max(1)).min(bh - 1);
        let arow = &a.pixels[y * aw..(y + 1) * aw];
        let brow = &b.pixels[by * bw..(by + 1) * bw];
        for (x, pa) in arow.iter().enumerate() {
            let pb = brow[bx[x]];
            let d = [
                pa.r().abs_diff(pb.r()),
                pa.g().abs_diff(pb.g()),
                pa.b().abs_diff(pb.b()),
                pa.a().abs_diff(pb.a()),
            ]
            .into_iter()
            .max()
            .unwrap_or(0);
            if d == 0 {
                let l = ((pa.r() as u32 * 3 + pa.g() as u32 * 6 + pa.b() as u32) / 10 / 4) as u8;
                out.push(Color32::from_gray(l + 16));
            } else {
                changed += 1;
                max = max.max(d);
                let v = 96 + (d as u32 * 159 / 255) as u8;
                out.push(Color32::from_rgb(v, (v / 6).saturating_sub(4), 0));
            }
        }
    }
    let stats = DiffStats {
        changed: changed as f32 / (aw * ah).max(1) as f32,
        max,
    };
    (ColorImage::new([aw, ah], out), stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_cycle() {
        let m = CompareMode::SideBySide;
        assert_eq!(m.next().next().next(), m);
        assert_eq!(m.next(), CompareMode::Slider);
    }

    #[test]
    fn same_images_have_no_difference() {
        let a = ColorImage::new([4, 3], vec![Color32::from_rgb(10, 20, 30); 12]);
        let (img, s) = diff(&a, &a);
        assert_eq!(
            s,
            DiffStats {
                changed: 0.0,
                max: 0
            }
        );
        assert!(img.pixels.iter().all(|p| p.r() == p.g() && p.g() == p.b()));
    }

    #[test]
    fn one_changed_pixel_is_found_and_marked() {
        let a = ColorImage::new([4, 4], vec![Color32::WHITE; 16]);
        let mut b = a.clone();
        b.pixels[5] = Color32::from_rgb(255, 200, 255);
        let (img, s) = diff(&a, &b);
        assert_eq!(s.max, 55);
        assert!((s.changed - 1.0 / 16.0).abs() < 1e-6);
        assert!(img.pixels[5].r() > img.pixels[5].g());
        assert_ne!(img.pixels[5], img.pixels[4]);
    }

    #[test]
    fn different_sizes_are_sampled_to_match() {
        // B is A at 2x: identical once sampled.
        let px = |x: usize| Color32::from_gray((x * 60) as u8);
        let a = ColorImage::new([2, 1], vec![px(0), px(1)]);
        let b = ColorImage::new(
            [4, 2],
            vec![px(0), px(0), px(1), px(1), px(0), px(0), px(1), px(1)],
        );
        let (_, s) = diff(&a, &b);
        assert_eq!(s.changed, 0.0);
        assert_eq!(
            match_width(Vec2::new(200.0, 100.0), Vec2::new(400.0, 400.0)),
            Vec2::new(200.0, 200.0)
        );
    }
}
