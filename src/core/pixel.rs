//! Pixel readout: which image pixel is under the cursor and its colour.

use eframe::egui::Pos2;

use super::decode::Decoded;

/// Texture coordinates (0..1) under screen point `pos`, for an image drawn
/// with corners `dest` (TL, TR, BR, BL) and texture coordinates `uvs` there.
/// The inverse of [`super::view::place_part`].
pub fn uv_at(dest: [Pos2; 4], uvs: [Pos2; 4], pos: Pos2) -> Pos2 {
    // Screen = dest0 + a * (dest1 - dest0) + b * (dest3 - dest0), with the
    // same a, b giving uv = uv0 + a * (uv1 - uv0) + b * (uv3 - uv0).
    let e1 = dest[1] - dest[0];
    let e3 = dest[3] - dest[0];
    let d = pos - dest[0];
    let det = e1.x * e3.y - e3.x * e1.y;
    if det.abs() < f32::EPSILON {
        return uvs[0];
    }
    let a = (d.x * e3.y - e3.x * d.y) / det;
    let b = (e1.x * d.y - d.x * e1.y) / det;
    uvs[0] + (uvs[1] - uvs[0]) * a + (uvs[3] - uvs[0]) * b
}

/// The pixel at `uv` in an image of `size`, or `None` outside it.
pub fn pixel_at(uv: Pos2, size: [u32; 2]) -> Option<[u32; 2]> {
    if !(0.0..1.0).contains(&uv.x) || !(0.0..1.0).contains(&uv.y) {
        return None;
    }
    Some([
        ((uv.x * size[0] as f32) as u32).min(size[0].saturating_sub(1)),
        ((uv.y * size[1] as f32) as u32).min(size[1].saturating_sub(1)),
    ])
}

/// Colour of the drawn pixels at stored texture coordinates `uv` as
/// straight (not premultiplied) RGBA. Screen-size textures give the
/// averaged colour.
pub fn sample(d: &Decoded, frame: usize, uv: Pos2) -> Option<[u8; 4]> {
    let [tw, th] = d.texture_size();
    let [x, y] = pixel_at(uv, [tw as u32, th as u32])?;
    let (x, y) = (x as usize, y as usize);
    let c = if d.is_tiled() {
        let t = d.tiles.iter().find(|t| {
            let [w, h] = t.image.size;
            (t.x..t.x + w).contains(&x) && (t.y..t.y + h).contains(&y)
        })?;
        t.image.pixels[(y - t.y) * t.image.size[0] + (x - t.x)]
    } else {
        let img = &d.frames.get(frame).or(d.frames.first())?.image;
        img.pixels[y * img.size[0] + x]
    };
    Some(c.to_srgba_unmultiplied())
}

pub fn hex([r, g, b, a]: [u8; 4]) -> String {
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

pub fn rgb([r, g, b, a]: [u8; 4]) -> String {
    if a == 255 {
        format!("RGB {r} {g} {b}")
    } else {
        format!("RGBA {r} {g} {b} {a}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::view::{corner_uvs, corners, place_part};
    use eframe::egui::{pos2, Color32, ColorImage, Rect, Vec2};

    const UNIT: [Pos2; 4] = [
        pos2(0.0, 0.0),
        pos2(1.0, 0.0),
        pos2(1.0, 1.0),
        pos2(0.0, 1.0),
    ];

    #[test]
    fn screen_points_map_back_to_pixels() {
        let dest = corners(Rect::from_min_size(
            pos2(100.0, 50.0),
            Vec2::new(400.0, 200.0),
        ));
        // Unrotated 400x200 image at 100%: pixel under (150, 60) is (50, 10).
        let uv = uv_at(dest, UNIT, pos2(150.5, 60.5));
        assert_eq!(pixel_at(uv, [400, 200]), Some([50, 10]));
        assert_eq!(
            pixel_at(uv_at(dest, UNIT, pos2(99.0, 60.0)), [400, 200]),
            None
        );
        // Any rotation/flip: uv_at undoes place_part.
        for turns in 0..4 {
            for flip in [false, true] {
                let uvs = corner_uvs(turns, flip, false);
                let part = Rect::from_min_max(pos2(0.25, 0.5), pos2(0.75, 1.0));
                let at = place_part(dest, uvs, part);
                let back = uv_at(dest, uvs, at[0]);
                assert!((back - part.min).length() < 1e-4, "{turns} {flip} {back:?}");
            }
        }
    }

    #[test]
    fn colours_and_formats() {
        let mut img = ColorImage::new([2, 1], vec![Color32::from_rgb(58, 123, 213); 2]);
        img.pixels[1] = Color32::from_rgba_unmultiplied(255, 0, 0, 128);
        let d = Decoded::from_color_image(img, "PNG");
        let c = sample(&d, 0, pos2(0.1, 0.5)).unwrap();
        assert_eq!(hex(c), "#3A7BD5");
        assert_eq!(rgb(c), "RGB 58 123 213");
        let c = sample(&d, 0, pos2(0.9, 0.5)).unwrap();
        assert_eq!(c[3], 128);
        assert_eq!(hex(c), "#FF000080");
        assert!(rgb(c).starts_with("RGBA "));
        assert_eq!(sample(&d, 0, pos2(1.5, 0.5)), None);
    }
}
