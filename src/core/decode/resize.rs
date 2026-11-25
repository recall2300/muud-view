//! Shrinking for the screen and the GPU, tiles for huge images, turning
//! upright.

use std::sync::Arc;

use eframe::egui::{Color32, ColorImage};

use super::*;
/// The stored-pixel size to shrink to so the image fits `fit` (upright
/// screen box) once `o` is applied, or `None` if it already fits.
pub fn screen_target(size: [usize; 2], fit: [usize; 2], o: Orientation) -> Option<[usize; 2]> {
    let [w, h] = size;
    let [fw, fh] = if o.swaps_axes() {
        [fit[1], fit[0]]
    } else {
        fit
    };
    if fw == 0 || fh == 0 || (w <= fw && h <= fh) {
        return None;
    }
    let s = (fw as f64 / w as f64).min(fh as f64 / h as f64);
    Some([
        ((w as f64 * s).round() as usize).max(1),
        ((h as f64 * s).round() as usize).max(1),
    ])
}

/// Size after shrinking so neither side exceeds `max_side`.
pub(super) fn gpu_target(size: [usize; 2], max_side: usize) -> Option<[usize; 2]> {
    let [w, h] = size;
    if w <= max_side && h <= max_side {
        return None;
    }
    let s = max_side as f64 / w.max(h) as f64;
    Some([
        ((w as f64 * s).round() as usize).clamp(1, max_side),
        ((h as f64 * s).round() as usize).clamp(1, max_side),
    ])
}

/// The last step for a still image: tiles for a huge image at full quality,
/// otherwise [`fit_for_display`]. Returns (image, tiles, reduced); with
/// tiles, the image is a screen-size preview.
pub(crate) fn finish(
    image: ColorImage,
    opts: DecodeOpts,
    o: Orientation,
) -> (ColorImage, Vec<Tile>, bool) {
    let (image, tiles, reduced, _) = finish_keep(image, opts, o);
    (image, tiles, reduced)
}

/// [`finish`], also returning the full-resolution pixels when they were
/// shrunk for the screen and [`DecodeOpts::keep_full`] asks for them (and
/// they fit in one texture).
pub(crate) fn finish_keep(
    image: ColorImage,
    opts: DecodeOpts,
    o: Orientation,
) -> (ColorImage, Vec<Tile>, bool, Option<ColorImage>) {
    let [w, h] = image.size;
    if opts.keep_full && w <= opts.max_side && h <= opts.max_side {
        if let Some(t) = opts.fit.and_then(|f| screen_target(image.size, f, o)) {
            let small = downscale(&image, t);
            return (small, Vec::new(), true, Some(image));
        }
    }
    let (image, tiles, reduced) = finish_inner(image, opts, o);
    (image, tiles, reduced, None)
}

pub(super) fn finish_inner(
    image: ColorImage,
    opts: DecodeOpts,
    o: Orientation,
) -> (ColorImage, Vec<Tile>, bool) {
    let [w, h] = image.size;
    if opts.tile && opts.fit.is_none() && (w > opts.max_side || h > opts.max_side) {
        // Absurdly large: shrink to the memory budget first.
        let image = match budget_target(image.size, MAX_TILED_BYTES) {
            Some(t) => downscale(&image, t),
            None => image,
        };
        let preview_target = gpu_target(image.size, 2048.min(opts.max_side)).unwrap_or(image.size);
        let preview = downscale(&image, preview_target);
        let tiles = split_tiles(&image, TILE_SIDE.min(opts.max_side));
        return (preview, tiles, false);
    }
    let (image, reduced) = fit_for_display(image, opts, o);
    (image, Vec::new(), reduced)
}

pub(crate) fn tiled_size(tiles: &[Tile]) -> [usize; 2] {
    tiles.iter().fold([0, 0], |[w, h], t| {
        [w.max(t.x + t.image.size[0]), h.max(t.y + t.image.size[1])]
    })
}

/// Size to shrink to so the RGBA pixels fit in `budget` bytes.
pub(super) fn budget_target(size: [usize; 2], budget: usize) -> Option<[usize; 2]> {
    let [w, h] = size;
    if w * h * 4 <= budget {
        return None;
    }
    let s = (budget as f64 / (w * h * 4) as f64).sqrt();
    Some([
        ((w as f64 * s).floor() as usize).max(1),
        ((h as f64 * s).floor() as usize).max(1),
    ])
}

/// Cuts an image into `side`×`side` pieces (smaller at the right and
/// bottom edges).
pub fn split_tiles(img: &ColorImage, side: usize) -> Vec<Tile> {
    let [w, h] = img.size;
    let side = side.max(1);
    let mut tiles = Vec::new();
    for y in (0..h).step_by(side) {
        let th = side.min(h - y);
        for x in (0..w).step_by(side) {
            let tw = side.min(w - x);
            let mut pixels = Vec::with_capacity(tw * th);
            for row in y..y + th {
                pixels.extend_from_slice(&img.pixels[row * w + x..row * w + x + tw]);
            }
            tiles.push(Tile {
                x,
                y,
                image: Arc::new(ColorImage::new([tw, th], pixels)),
            });
        }
    }
    tiles
}

/// Shrinks a freshly decoded image to screen size (if asked) or to what the
/// GPU can hold. Returns whether it was shrunk for the screen.
pub(crate) fn fit_for_display(
    image: ColorImage,
    opts: DecodeOpts,
    o: Orientation,
) -> (ColorImage, bool) {
    if let Some(t) = opts.fit.and_then(|f| screen_target(image.size, f, o)) {
        // Never larger than the GPU limit either.
        let t = gpu_target(t, opts.max_side).unwrap_or(t);
        return (downscale(&image, t), true);
    }
    match gpu_target(image.size, opts.max_side) {
        Some(t) => (downscale(&image, t), false),
        None => (image, false),
    }
}

/// High-quality, fast shrink: average whole `k`×`k` blocks first (one pass
/// over memory), then an exact area-average to the target size. Area
/// averaging never aliases, and fixed-point maths keeps it quick. Works on
/// premultiplied pixels, which average correctly.
pub fn downscale(img: &ColorImage, target: [usize; 2]) -> ColorImage {
    let [w, h] = img.size;
    let [tw, th] = [target[0].max(1), target[1].max(1)];
    if [w, h] == [tw, th] {
        return img.clone();
    }
    let k = (w / tw).min(h / th).max(1);
    if k >= 2 {
        let boxed = box_shrink(&img.pixels, w, h, k);
        area_resample(&boxed, [w / k, h / k], [tw, th])
    } else {
        area_resample(&img.pixels, [w, h], [tw, th])
    }
}

/// For each output index, the first source index and the 16-bit weights of
/// the source pixels it covers (summing to 65536).
pub(super) fn area_taps(src: usize, dst: usize) -> Vec<(usize, Vec<u32>)> {
    let r = src as f64 / dst as f64;
    (0..dst)
        .map(|o| {
            let a = o as f64 * r;
            let b = (a + r).min(src as f64);
            let s0 = a.floor() as usize;
            let s1 = (b.ceil() as usize).clamp(s0 + 1, src);
            let mut w: Vec<u32> = (s0..s1)
                .map(|s| {
                    let cover = b.min(s as f64 + 1.0) - a.max(s as f64);
                    (cover.max(0.0) / (b - a) * 65536.0).round() as u32
                })
                .collect();
            // Make the weights sum to exactly 1.0 so flat areas stay flat.
            let sum: u32 = w.iter().sum();
            let big = (0..w.len()).max_by_key(|&i| w[i]).unwrap_or(0);
            w[big] = (w[big] + 65536).saturating_sub(sum);
            (s0, w)
        })
        .collect()
}

/// Exact area-average resize (shrinking only), separable, fixed point.
pub(super) fn area_resample(src: &[Color32], size: [usize; 2], target: [usize; 2]) -> ColorImage {
    let [w, h] = size;
    let [tw, th] = target;
    let xt = area_taps(w, tw);
    let yt = area_taps(h, th);
    // Horizontal pass: h rows of tw pixels.
    let mut mid = vec![Color32::TRANSPARENT; tw * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let out = &mut mid[y * tw..(y + 1) * tw];
        for (o, (s0, wts)) in out.iter_mut().zip(&xt) {
            let mut acc = [0u32; 4];
            for (p, &wt) in row[*s0..].iter().zip(wts) {
                acc[0] += p.r() as u32 * wt;
                acc[1] += p.g() as u32 * wt;
                acc[2] += p.b() as u32 * wt;
                acc[3] += p.a() as u32 * wt;
            }
            *o = to_color(acc);
        }
    }
    // Vertical pass, row by row so memory is read in order.
    let mut out = Vec::with_capacity(tw * th);
    let mut acc = vec![[0u32; 4]; tw];
    for (s0, wts) in &yt {
        acc.iter_mut().for_each(|a| *a = [0; 4]);
        for (dy, &wt) in wts.iter().enumerate() {
            let row = &mid[(s0 + dy) * tw..(s0 + dy + 1) * tw];
            for (a, p) in acc.iter_mut().zip(row) {
                a[0] += p.r() as u32 * wt;
                a[1] += p.g() as u32 * wt;
                a[2] += p.b() as u32 * wt;
                a[3] += p.a() as u32 * wt;
            }
        }
        out.extend(acc.iter().map(|a| to_color(*a)));
    }
    ColorImage::new([tw, th], out)
}

pub(super) fn to_color(acc: [u32; 4]) -> Color32 {
    let c = |v: u32| ((v + 32768) >> 16).min(255) as u8;
    Color32::from_rgba_premultiplied(c(acc[0]), c(acc[1]), c(acc[2]), c(acc[3]))
}

/// Averages `k`×`k` blocks (the right/bottom remainder is dropped).
pub(super) fn box_shrink(src: &[Color32], w: usize, h: usize, k: usize) -> Vec<Color32> {
    let (ow, oh) = (w / k, h / k);
    let area = (k * k) as u32;
    let bytes: &[u8] = bytemuck::cast_slice(src);
    let mut out = Vec::with_capacity(ow * oh);
    // Column sums first (k rows added channel by channel, which vectorises),
    // then each block of k columns.
    let mut cols = vec![0u32; ow * k * 4];
    for oy in 0..oh {
        cols.iter_mut().for_each(|c| *c = 0);
        for dy in 0..k {
            let start = (oy * k + dy) * w * 4;
            let row = &bytes[start..start + ow * k * 4];
            for (c, b) in cols.iter_mut().zip(row) {
                *c += *b as u32;
            }
        }
        out.extend(cols.chunks_exact(k * 4).map(|block| {
            let mut a = [0u32; 4];
            for px in block.chunks_exact(4) {
                a[0] += px[0];
                a[1] += px[1];
                a[2] += px[2];
                a[3] += px[3];
            }
            let avg = |v: u32| ((v + area / 2) / area) as u8;
            Color32::from_rgba_premultiplied(avg(a[0]), avg(a[1]), avg(a[2]), avg(a[3]))
        }));
    }
    out
}

/// Rotates/mirrors pixels so they look upright (only for copying out).
pub fn upright(img: &ColorImage, o: Orientation) -> ColorImage {
    if o == Orientation::default() {
        return img.clone();
    }
    let [w, h] = img.size;
    let (ow, oh) = if o.swaps_axes() { (h, w) } else { (w, h) };
    let mut out = vec![Color32::BLACK; ow * oh];
    for y in 0..oh {
        for x in 0..ow {
            // Undo the flip, then the rotation, to find the source pixel.
            let x0 = if o.flip { ow - 1 - x } else { x };
            let (sx, sy) = match o.turns % 4 {
                0 => (x0, y),
                1 => (y, h - 1 - x0),
                2 => (w - 1 - x0, h - 1 - y),
                _ => (w - 1 - y, x0),
            };
            out[y * ow + x] = img.pixels[sy * w + sx];
        }
    }
    ColorImage::new([ow, oh], out)
}

/// Downscales so the longer side is at most `max_side` (for "copy resized").
pub fn shrink(img: &ColorImage, max_side: usize) -> ColorImage {
    let [w, h] = img.size;
    if w.max(h) <= max_side {
        return img.clone();
    }
    let t = gpu_target([w, h], max_side).expect("larger than max_side");
    downscale(img, t)
}
