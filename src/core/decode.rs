//! Decoding files into ready-to-upload pixel buffers.
//!
//! JPEG — by far the most common format, and the slowest for big camera
//! files — takes a fast path: zune-jpeg decodes straight into the RGBA
//! buffer that becomes the GPU texture, and EXIF orientation is applied at
//! draw time instead of by moving pixels. Everything else goes through the
//! `image` crate.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use eframe::egui::{Color32, ColorImage};
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat};

use super::exif_info::{self, ExifInfo};

/// Upper bound on decoded animation frames kept in memory (bytes).
const MAX_ANIMATION_BYTES: usize = 768 * 1024 * 1024;

/// How big the decoded pixels may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeOpts {
    /// Largest texture side the GPU accepts.
    pub max_side: usize,
    /// Screen quality: shrink to fit this box (upright width, height in
    /// physical pixels). `None` keeps every pixel the GPU can hold.
    pub fit: Option<[usize; 2]>,
    /// Images larger than `max_side` are split into tiles instead of being
    /// shrunk (full quality only).
    pub tile: bool,
    /// Convert colours from an embedded ICC profile to sRGB.
    pub color: bool,
}

impl DecodeOpts {
    /// Every pixel up to `max_side`, in one image.
    pub fn full(max_side: usize) -> Self {
        Self {
            max_side,
            fit: None,
            tile: false,
            color: true,
        }
    }

    /// Every pixel, split into tiles if the GPU cannot hold it in one.
    pub fn tiled(max_side: usize) -> Self {
        Self {
            tile: true,
            ..Self::full(max_side)
        }
    }
}

/// Side of one tile of a huge image (and never more than the GPU limit).
pub const TILE_SIDE: usize = 4096;
/// Most memory a tiled image may take; larger ones are shrunk to fit.
pub const MAX_TILED_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// One piece of an image too large for a single GPU texture.
pub struct Tile {
    /// Position and size in stored (not rotated) pixels.
    pub x: usize,
    pub y: usize,
    pub image: Arc<ColorImage>,
}

pub struct Frame {
    pub image: Arc<ColorImage>,
    pub delay: Duration,
}

/// How stored pixels must be turned to look upright: rotate clockwise by
/// `turns` quarter turns, then mirror horizontally if `flip`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Orientation {
    pub turns: u8,
    pub flip: bool,
}

impl Orientation {
    /// From the EXIF Orientation tag (1–8).
    pub fn from_exif(tag: u32) -> Self {
        let (turns, flip) = match tag {
            2 => (0, true),
            3 => (2, false),
            4 => (2, true),
            5 => (1, true),
            6 => (1, false),
            7 => (3, true),
            8 => (3, false),
            _ => (0, false),
        };
        Self { turns, flip }
    }

    pub fn swaps_axes(self) -> bool {
        self.turns % 2 == 1
    }
}

pub struct Decoded {
    pub frames: Vec<Frame>,
    /// Upright size in image pixels (after orientation, before any
    /// downscaling for the GPU).
    pub width: u32,
    pub height: u32,
    /// Applied when drawing; the stored frames are not rotated.
    pub orientation: Orientation,
    pub format: &'static str,
    pub color: String,
    pub has_alpha: bool,
    pub file_size: u64,
    pub modified: Option<SystemTime>,
    pub exif: Option<ExifInfo>,
    /// Luminance histogram, 256 bins.
    pub histogram: Vec<u32>,
    pub decode_time: Duration,
    /// Shrunk to screen size; a full decode would have more pixels.
    pub reduced: bool,
    /// Every pixel of a huge image, in pieces. `frames` then holds a
    /// screen-size preview of it.
    pub tiles: Vec<Tile>,
    /// Stored size covered by `tiles`.
    pub tiled_size: [usize; 2],
    /// Name of the embedded colour profile, if any.
    pub profile: Option<String>,
    /// Colours were converted from that profile to sRGB.
    pub color_converted: bool,
}

impl Decoded {
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// Approximate memory held by the decoded frames and tiles.
    pub fn bytes(&self) -> usize {
        self.frames
            .iter()
            .map(|f| f.image.pixels.len() * 4)
            .chain(self.tiles.iter().map(|t| t.image.pixels.len() * 4))
            .sum()
    }

    pub fn is_tiled(&self) -> bool {
        !self.tiles.is_empty()
    }

    /// Stored size of the drawable pixels (not rotated): the tiles if any,
    /// otherwise the single texture.
    pub fn texture_size(&self) -> [usize; 2] {
        if self.is_tiled() {
            return self.tiled_size;
        }
        self.frames.first().map(|f| f.image.size).unwrap_or([1, 1])
    }

    /// True if the texture holds fewer pixels than the image.
    pub fn is_downscaled(&self) -> bool {
        let [tw, th] = self.texture_size();
        tw.max(th) < self.width.max(self.height) as usize
    }

    /// A still image wrapped as `Decoded` (used for pasted images).
    pub fn from_color_image(image: ColorImage, format: &'static str) -> Self {
        let [w, h] = image.size;
        let histogram = histogram_of(&image);
        let has_alpha = has_alpha(&image);
        Self {
            frames: vec![Frame {
                image: Arc::new(image),
                delay: Duration::ZERO,
            }],
            width: w as u32,
            height: h as u32,
            orientation: Orientation::default(),
            format,
            color: "RGBA 8".into(),
            has_alpha,
            file_size: 0,
            modified: None,
            exif: None,
            histogram,
            decode_time: Duration::ZERO,
            reduced: false,
            tiles: Vec::new(),
            tiled_size: [0, 0],
            profile: None,
            color_converted: false,
        }
    }
}

pub fn decode_bytes(bytes: &[u8], path: &Path, opts: DecodeOpts) -> Result<Decoded, String> {
    let format = image::guess_format(bytes)
        .ok()
        .or_else(|| ImageFormat::from_path(path).ok())
        .ok_or_else(|| "unknown image format".to_string())?;

    let raw_exif = match format {
        ImageFormat::Jpeg | ImageFormat::Tiff | ImageFormat::WebP | ImageFormat::Png => {
            exif_info::read(bytes)
        }
        _ => None,
    };
    let exif = raw_exif.clone().filter(|e| !e.is_empty());

    if format == ImageFormat::Jpeg {
        let orientation =
            Orientation::from_exif(raw_exif.as_ref().map(|e| e.orientation).unwrap_or(1));
        // Fall back to the general path for the rare JPEG zune-jpeg rejects.
        if let Ok(d) = decode_jpeg(bytes, opts, orientation, exif.clone()) {
            return Ok(d);
        }
    }

    let animated = match format {
        ImageFormat::Gif => decode_animation(
            image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?,
            opts,
        )?,
        ImageFormat::WebP => {
            let dec = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
                .map_err(|e| e.to_string())?;
            if dec.has_animation() {
                decode_animation(dec, opts)?
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some((frames, w, h, reduced)) = animated {
        let histogram = histogram_of(&frames[0].image);
        return Ok(Decoded {
            width: w,
            height: h,
            orientation: Orientation::default(),
            format: format_name(format),
            color: "RGBA 8".into(),
            has_alpha: frames.iter().any(|f| has_alpha(&f.image)),
            histogram,
            frames,
            file_size: 0,
            modified: None,
            exif,
            decode_time: Duration::ZERO,
            reduced,
            tiles: Vec::new(),
            tiled_size: [0, 0],
            profile: None,
            color_converted: false,
        });
    }

    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(4 * 1024 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let color = decoder.color_type();
    let icc = if opts.color {
        decoder.icc_profile().ok().flatten()
    } else {
        None
    };
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);

    let (width, height) = (img.width(), img.height());
    let image = if color.has_alpha() {
        let rgba = img.into_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
    } else {
        // Opaque: premultiplied and unmultiplied are identical, skip the maths.
        opaque_color_image(img)
    };
    let (mut image, mut tiles, reduced) = finish(image, opts, Orientation::default());
    let tiled_size = tiled_size(&tiles);
    let (profile, color_converted) = manage_color(icc.as_deref(), &mut image, &mut tiles);

    let histogram = histogram_of(&image);
    let alpha = color.has_alpha() && has_alpha(&image);
    Ok(Decoded {
        frames: vec![Frame {
            image: Arc::new(image),
            delay: Duration::ZERO,
        }],
        width,
        height,
        orientation: Orientation::default(),
        format: format_name(format),
        color: color_name(color),
        has_alpha: alpha,
        file_size: 0,
        modified: None,
        exif,
        histogram,
        decode_time: Duration::ZERO,
        reduced,
        tiles,
        tiled_size,
        profile,
        color_converted,
    })
}

/// zune-jpeg straight into the texture buffer: no RGB→RGBA pass and no
/// premultiply pass (JPEG has no alpha).
fn decode_jpeg(
    bytes: &[u8],
    opts: DecodeOpts,
    orientation: Orientation,
    exif: Option<ExifInfo>,
) -> Result<Decoded, String> {
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;

    let zopts = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(1 << 16)
        .set_max_height(1 << 16);
    let mut dec = zune_jpeg::JpegDecoder::new_with_options(Cursor::new(bytes), zopts);
    dec.decode_headers().map_err(|e| format!("{e:?}"))?;
    let (w, h) = dec.dimensions().ok_or("no dimensions")?;
    let gray = matches!(dec.input_colorspace(), Some(ColorSpace::Luma));
    let icc = if opts.color { dec.icc_profile() } else { None };
    let mut pixels = vec![Color32::BLACK; w * h];
    dec.decode_into(bytemuck::cast_slice_mut(&mut pixels))
        .map_err(|e| format!("{e:?}"))?;
    let image = ColorImage::new([w, h], pixels);
    let (mut image, mut tiles, reduced) = finish(image, opts, orientation);
    let tiled_size = tiled_size(&tiles);
    let (profile, color_converted) = manage_color(icc.as_deref(), &mut image, &mut tiles);

    let (uw, uh) = if orientation.swaps_axes() {
        (h, w)
    } else {
        (w, h)
    };
    let histogram = histogram_of(&image);
    Ok(Decoded {
        frames: vec![Frame {
            image: Arc::new(image),
            delay: Duration::ZERO,
        }],
        width: uw as u32,
        height: uh as u32,
        orientation,
        format: "JPEG",
        color: if gray { "GRAY 8" } else { "YCbCr 8" }.into(),
        has_alpha: false,
        file_size: 0,
        modified: None,
        exif,
        histogram,
        decode_time: Duration::ZERO,
        reduced,
        tiles,
        tiled_size,
        profile,
        color_converted,
    })
}

/// Converts the final pixels (after shrinking, so it is cheap) from an
/// embedded profile to sRGB. Returns the profile name and whether
/// anything was converted.
fn manage_color(
    icc: Option<&[u8]>,
    image: &mut ColorImage,
    tiles: &mut [Tile],
) -> (Option<String>, bool) {
    let Some(icc) = icc else {
        return (None, false);
    };
    let profile = super::color::prepare(icc);
    let name = profile.name().map(str::to_owned);
    let Some(fix) = profile.fix() else {
        return (name, false);
    };
    fix.apply(image);
    for t in tiles {
        if let Some(img) = Arc::get_mut(&mut t.image) {
            fix.apply(img);
        }
    }
    (name, true)
}

fn opaque_color_image(img: DynamicImage) -> ColorImage {
    let rgba = img.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let mut pixels = vec![Color32::BLACK; size[0] * size[1]];
    bytemuck::cast_slice_mut::<Color32, u8>(&mut pixels).copy_from_slice(rgba.as_raw());
    ColorImage::new(size, pixels)
}

type Animation = Option<(Vec<Frame>, u32, u32, bool)>;

fn decode_animation<'a>(
    dec: impl AnimationDecoder<'a>,
    opts: DecodeOpts,
) -> Result<Animation, String> {
    let mut frames = Vec::new();
    let mut total = 0usize;
    let mut reduced = false;
    let (mut w, mut h) = (0, 0);
    for frame in dec.into_frames() {
        let frame = frame.map_err(|e| e.to_string())?;
        let (num, den) = frame.delay().numer_denom_ms();
        let ms = num.checked_div(den).unwrap_or(100);
        // Browsers treat very short delays as 100 ms; do the same.
        let delay = Duration::from_millis(if ms < 20 { 100 } else { ms as u64 });
        let buf = frame.into_buffer();
        w = buf.width();
        h = buf.height();
        let size = [w as usize, h as usize];
        let (image, r) = fit_for_display(
            ColorImage::from_rgba_unmultiplied(size, buf.as_raw()),
            opts,
            Orientation::default(),
        );
        reduced |= r;
        total += image.pixels.len() * 4;
        frames.push(Frame {
            image: Arc::new(image),
            delay,
        });
        if total > MAX_ANIMATION_BYTES {
            break;
        }
    }
    Ok(match frames.len() {
        0 => return Err("no frames".into()),
        1 => None, // a single-frame GIF/WebP is a still image
        _ => Some((frames, w, h, reduced)),
    })
}

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
fn gpu_target(size: [usize; 2], max_side: usize) -> Option<[usize; 2]> {
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
fn finish(image: ColorImage, opts: DecodeOpts, o: Orientation) -> (ColorImage, Vec<Tile>, bool) {
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

fn tiled_size(tiles: &[Tile]) -> [usize; 2] {
    tiles.iter().fold([0, 0], |[w, h], t| {
        [w.max(t.x + t.image.size[0]), h.max(t.y + t.image.size[1])]
    })
}

/// Size to shrink to so the RGBA pixels fit in `budget` bytes.
fn budget_target(size: [usize; 2], budget: usize) -> Option<[usize; 2]> {
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
fn fit_for_display(image: ColorImage, opts: DecodeOpts, o: Orientation) -> (ColorImage, bool) {
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
fn area_taps(src: usize, dst: usize) -> Vec<(usize, Vec<u32>)> {
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
fn area_resample(src: &[Color32], size: [usize; 2], target: [usize; 2]) -> ColorImage {
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

fn to_color(acc: [u32; 4]) -> Color32 {
    let c = |v: u32| ((v + 32768) >> 16).min(255) as u8;
    Color32::from_rgba_premultiplied(c(acc[0]), c(acc[1]), c(acc[2]), c(acc[3]))
}

/// Averages `k`×`k` blocks (the right/bottom remainder is dropped).
fn box_shrink(src: &[Color32], w: usize, h: usize, k: usize) -> Vec<Color32> {
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

pub fn has_alpha(img: &ColorImage) -> bool {
    img.pixels.iter().any(|p| p.a() < 255)
}

pub fn histogram_of(img: &ColorImage) -> Vec<u32> {
    let mut hist = vec![0u32; 256];
    // Sample at most ~2M pixels; the shape is what matters.
    let step = (img.pixels.len() / 2_000_000).max(1);
    for p in img.pixels.iter().step_by(step) {
        let l = (p.r() as u32 * 54 + p.g() as u32 * 183 + p.b() as u32 * 19) >> 8;
        hist[l.min(255) as usize] += 1;
    }
    hist
}

fn format_name(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Png => "PNG",
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WEBP",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Tga => "TGA",
        ImageFormat::Ico => "ICO",
        ImageFormat::Pnm => "PNM",
        ImageFormat::Qoi => "QOI",
        ImageFormat::Dds => "DDS",
        ImageFormat::Hdr => "HDR",
        ImageFormat::OpenExr => "EXR",
        _ => "IMAGE",
    }
}

fn color_name(c: image::ColorType) -> String {
    use image::ColorType::*;
    match c {
        L8 => "GRAY 8",
        La8 => "GRAY+A 8",
        Rgb8 => "RGB 8",
        Rgba8 => "RGBA 8",
        L16 => "GRAY 16",
        La16 => "GRAY+A 16",
        Rgb16 => "RGB 16",
        Rgba16 => "RGBA 16",
        Rgb32F => "RGB 32F",
        Rgba32F => "RGBA 32F",
        _ => "OTHER",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(img: DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), format).unwrap();
        out
    }

    fn png_bytes(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        encoded(
            DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(w, h, image::Rgba(rgba))),
            ImageFormat::Png,
        )
    }

    #[test]
    fn decodes_png() {
        let d = decode_bytes(
            &png_bytes(40, 20, [10, 200, 30, 255]),
            Path::new("a.png"),
            DecodeOpts::full(8192),
        )
        .unwrap();
        assert_eq!((d.width, d.height), (40, 20));
        assert_eq!(d.format, "PNG");
        assert!(!d.has_alpha);
        assert!(!d.is_animated());
        assert_eq!(d.histogram.iter().sum::<u32>(), 800);
    }

    #[test]
    fn decodes_jpeg_on_fast_path() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            64,
            32,
            image::Rgb([200, 40, 40]),
        ));
        let d = decode_bytes(
            &encoded(img, ImageFormat::Jpeg),
            Path::new("a.jpg"),
            DecodeOpts::full(8192),
        )
        .unwrap();
        assert_eq!((d.width, d.height), (64, 32));
        assert_eq!(d.format, "JPEG");
        let p = d.frames[0].image.pixels[0];
        assert!(p.r() > 180 && p.g() < 70 && p.a() == 255, "{p:?}");
    }

    #[test]
    fn detects_alpha() {
        let d = decode_bytes(
            &png_bytes(4, 4, [0, 0, 0, 10]),
            Path::new("a.png"),
            DecodeOpts::full(8192),
        )
        .unwrap();
        assert!(d.has_alpha);
    }

    #[test]
    fn downscales_to_texture_limit_but_reports_original_size() {
        let d = decode_bytes(
            &png_bytes(600, 300, [1, 2, 3, 255]),
            Path::new("a.png"),
            DecodeOpts::full(256),
        )
        .unwrap();
        assert_eq!((d.width, d.height), (600, 300));
        assert_eq!(d.texture_size(), [256, 128]);
        assert!(d.is_downscaled());
    }

    #[test]
    fn screen_quality_fits_the_screen_and_is_marked_reduced() {
        let opts = DecodeOpts {
            fit: Some([100, 100]),
            ..DecodeOpts::full(8192)
        };
        let d = decode_bytes(
            &png_bytes(600, 300, [9, 9, 9, 255]),
            Path::new("a.png"),
            opts,
        )
        .unwrap();
        assert_eq!((d.width, d.height), (600, 300));
        assert_eq!(d.texture_size(), [100, 50]);
        assert!(d.reduced);
        // Already small enough: untouched and not reduced.
        let d = decode_bytes(&png_bytes(80, 40, [9, 9, 9, 255]), Path::new("a.png"), opts).unwrap();
        assert_eq!(d.texture_size(), [80, 40]);
        assert!(!d.reduced);
    }

    #[test]
    fn screen_target_respects_exif_rotation() {
        let none = Orientation::default();
        let cw = Orientation::from_exif(6);
        // 32.5 MP camera file on a 2560x1440 screen.
        assert_eq!(
            screen_target([6960, 4640], [2560, 1440], none),
            Some([2160, 1440])
        );
        // Stored landscape, shown portrait (960x1440 on screen).
        assert_eq!(
            screen_target([6960, 4640], [2560, 1440], cw),
            Some([1440, 960])
        );
        // ...and on a portrait monitor it may use the full height.
        assert_eq!(
            screen_target([6960, 4640], [1440, 2560], cw),
            Some([2160, 1440])
        );
        assert_eq!(screen_target([800, 600], [2560, 1440], none), None);
        // Memory per image: 129 MB -> 12.4 MB.
        assert_eq!(6960 * 4640 * 4 / 1_000_000, 129);
        assert_eq!(2160 * 1440 * 4 / 100_000, 124);
    }

    #[test]
    fn box_shrink_averages_blocks() {
        // 4x2: left half white, right half black; halve to 2x1.
        let w = Color32::WHITE;
        let b = Color32::BLACK;
        let img = ColorImage::new([4, 2], vec![w, w, b, b, w, w, b, b]);
        let out = downscale(&img, [2, 1]);
        assert_eq!(out.size, [2, 1]);
        assert_eq!(out.pixels, vec![w, b]);
        // Non-integer ratio still lands on the exact size.
        let img = ColorImage::new([10, 7], vec![Color32::GRAY; 70]);
        let out = downscale(&img, [3, 2]);
        assert_eq!(out.size, [3, 2]);
        assert!(
            out.pixels.iter().all(|p| *p == Color32::GRAY),
            "{:?}",
            out.pixels
        );
    }

    #[test]
    fn huge_images_are_tiled_at_full_quality() {
        let bytes = png_bytes(700, 300, [5, 6, 7, 255]);
        // GPU limit 256: tiles of 256, no pixel lost.
        let d = decode_bytes(&bytes, Path::new("a.png"), DecodeOpts::tiled(256)).unwrap();
        assert!(d.is_tiled());
        assert_eq!(d.tiles.len(), 3 * 2);
        assert_eq!(d.texture_size(), [700, 300]);
        assert!(!d.is_downscaled());
        assert!(!d.reduced);
        let last = d.tiles.last().unwrap();
        assert_eq!((last.x, last.y), (512, 256));
        assert_eq!(last.image.size, [188, 44]);
        let covered: usize = d.tiles.iter().map(|t| t.image.pixels.len()).sum();
        assert_eq!(covered, 700 * 300);
        // The preview stays within the GPU limit.
        assert!(d.frames[0].image.size[0] <= 256);
        // Without tiling it is shrunk as before.
        let d = decode_bytes(&bytes, Path::new("a.png"), DecodeOpts::full(256)).unwrap();
        assert!(!d.is_tiled());
        assert_eq!(d.texture_size(), [256, 110]);
    }

    #[test]
    fn split_tiles_keeps_pixels_in_place() {
        let pixels: Vec<Color32> = (0..15).map(|i| Color32::from_gray(i as u8)).collect();
        let img = ColorImage::new([5, 3], pixels);
        let tiles = split_tiles(&img, 2);
        assert_eq!(tiles.len(), 3 * 2);
        // Tile at (4, 2) holds the single bottom-right pixel.
        let t = tiles.iter().find(|t| t.x == 4 && t.y == 2).unwrap();
        assert_eq!(t.image.pixels, vec![Color32::from_gray(14)]);
        let t = tiles.iter().find(|t| t.x == 2 && t.y == 0).unwrap();
        assert_eq!(
            t.image.pixels,
            [2, 3, 7, 8].map(Color32::from_gray).to_vec()
        );
        assert_eq!(budget_target([100, 100], 40_000), None);
        assert_eq!(budget_target([200, 100], 40_000), Some([141, 70]));
    }

    #[test]
    fn embedded_display_p3_is_converted() {
        let img =
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([200, 60, 60])));
        let mut png = Vec::new();
        let mut enc = image::codecs::png::PngEncoder::new(&mut png);
        use image::ImageEncoder;
        enc.set_icc_profile(moxcms::ColorProfile::new_display_p3().encode().unwrap())
            .unwrap();
        enc.write_image(img.as_bytes(), 8, 8, image::ExtendedColorType::Rgb8)
            .unwrap();
        let d = decode_bytes(&png, Path::new("p3.png"), DecodeOpts::full(8192)).unwrap();
        assert!(d.color_converted);
        assert!(d.profile.is_some());
        let p = d.frames[0].image.pixels[0];
        assert!(p.r() > 205 && p.g() < 60, "{p:?}");
        // Turned off: the numbers are shown as they are.
        let off = DecodeOpts {
            color: false,
            ..DecodeOpts::full(8192)
        };
        let d = decode_bytes(&png, Path::new("p3.png"), off).unwrap();
        assert!(!d.color_converted);
        assert_eq!(d.frames[0].image.pixels[0], Color32::from_rgb(200, 60, 60));
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_bytes(b"hello", Path::new("x.txt"), DecodeOpts::full(8192)).is_err());
    }

    #[test]
    fn exif_orientation_table() {
        assert_eq!(Orientation::from_exif(1), Orientation::default());
        assert_eq!(
            Orientation::from_exif(6),
            Orientation {
                turns: 1,
                flip: false
            }
        );
        assert_eq!(
            Orientation::from_exif(8),
            Orientation {
                turns: 3,
                flip: false
            }
        );
        assert!(Orientation::from_exif(5).swaps_axes());
    }

    #[test]
    fn upright_rotates_pixels() {
        // 2x1: red, green. Rotated clockwise it becomes 1x2: red on top.
        let img = ColorImage::new([2, 1], vec![Color32::RED, Color32::GREEN]);
        let r = upright(
            &img,
            Orientation {
                turns: 1,
                flip: false,
            },
        );
        assert_eq!(r.size, [1, 2]);
        assert_eq!(r.pixels, vec![Color32::RED, Color32::GREEN]);
        let r = upright(
            &img,
            Orientation {
                turns: 0,
                flip: true,
            },
        );
        assert_eq!(r.pixels, vec![Color32::GREEN, Color32::RED]);
        let r = upright(
            &img,
            Orientation {
                turns: 3,
                flip: false,
            },
        );
        assert_eq!(r.pixels, vec![Color32::GREEN, Color32::RED]);
    }

    #[test]
    fn shrink_keeps_aspect() {
        let img = ColorImage::new([400, 200], vec![Color32::WHITE; 400 * 200]);
        assert_eq!(shrink(&img, 100).size, [100, 50]);
        assert_eq!(shrink(&img, 1000).size, [400, 200]);
    }

    /// A photo-like test JPEG: smooth gradients plus sensor-style noise, so
    /// the entropy decoder has real work (flat images decode unrealistically
    /// fast).
    pub(crate) fn synthetic_photo(w: u32, h: u32) -> Vec<u8> {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let n = (seed % 24) as i32 - 12;
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let wave = ((fx * 37.0).sin() * (fy * 23.0).cos() * 40.0) as i32;
            let c = |base: f32| (base as i32 + wave + n).clamp(0, 255) as u8;
            image::Rgb([
                c(60.0 + 150.0 * fx),
                c(90.0 + 120.0 * fy),
                c(200.0 - 140.0 * fx * fy),
            ])
        });
        let mut out = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
        if std::env::var_os("MUUD_BENCH_P3").is_some() {
            use image::ImageEncoder;
            let _ = enc.set_icc_profile(moxcms::ColorProfile::new_display_p3().encode().unwrap());
        }
        enc.encode_image(&img).unwrap();
        out
    }

    /// `cargo test --release -- --ignored --nocapture bench_jpeg`, optionally
    /// with MUUD_BENCH_JPEG=path/to/big.jpg (otherwise a synthetic 32.5 MP
    /// file is used).
    #[test]
    #[ignore]
    fn bench_jpeg() {
        let (bytes, name) = match std::env::var("MUUD_BENCH_JPEG") {
            Ok(p) => (std::fs::read(&p).unwrap(), p),
            Err(_) => (synthetic_photo(6960, 4640), "synthetic.jpg".into()),
        };
        println!("{name}: {:.1} MB", bytes.len() as f64 / 1e6);
        let screen = DecodeOpts {
            fit: Some([2560, 1440]),
            ..DecodeOpts::full(16384)
        };
        for (label, opts) in [("full", DecodeOpts::full(16384)), ("screen", screen)] {
            for _ in 0..3 {
                let t = std::time::Instant::now();
                let d = decode_bytes(&bytes, Path::new(&name), opts).unwrap();
                let [tw, th] = d.texture_size();
                println!(
                    "{label:>6}: {}x{} -> texture {tw}x{th} ({:.1} MB) in {:.1} ms",
                    d.width,
                    d.height,
                    d.bytes() as f64 / 1e6,
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
    }
}
