//! Decoding files into ready-to-upload pixel buffers.
//!
//! JPEG — by far the most common format, and the slowest for big camera
//! files — takes a fast path ([`jpeg`]): libjpeg-turbo decodes straight into
//! the RGBA buffer that becomes the GPU texture, shrinking while it decodes
//! when only the screen size is needed, and EXIF orientation is applied at
//! draw time instead of by moving pixels. JPEG XL, SVG, PSD and RAW have
//! their own modules; everything else goes through the `image` crate.
//! Shrinking and tiling are in [`resize`], animations in [`animation`].

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use eframe::egui::{Color32, ColorImage};
use image::{DynamicImage, ImageDecoder, ImageFormat};

use super::exif_info::{self, ExifInfo};

mod animation;
mod jpeg;
mod resize;
#[cfg(test)]
mod tests;

pub use animation::STREAM_EVERY;
use animation::*;
use jpeg::*;
pub use resize::{downscale, screen_target, shrink, upright};
pub(crate) use resize::{finish, finish_keep, fit_for_display, tiled_size};

/// Upper bound on decoded animation frames kept in memory (bytes).
pub(super) const MAX_ANIMATION_BYTES: usize = 768 * 1024 * 1024;

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
    /// Screen quality: also hand back the full-resolution pixels that were
    /// decoded on the way (in [`Decoded::full`]), so zooming in later needs
    /// no second decode.
    pub keep_full: bool,
}

impl DecodeOpts {
    /// Every pixel up to `max_side`, in one image.
    pub fn full(max_side: usize) -> Self {
        Self {
            max_side,
            fit: None,
            tile: false,
            color: true,
            keep_full: false,
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
#[derive(Clone)]
pub struct Tile {
    /// Position and size in stored (not rotated) pixels.
    pub x: usize,
    pub y: usize,
    pub image: Arc<ColorImage>,
}

#[derive(Clone)]
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

#[derive(Clone)]
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
    /// False for an animation still being decoded (more frames follow).
    pub complete: bool,
    /// With [`DecodeOpts::keep_full`]: the same image at full resolution.
    pub full: Option<Box<Decoded>>,
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
            complete: true,
            full: None,
        }
    }
}

#[cfg(test)]
pub fn decode_bytes(bytes: &[u8], path: &Path, opts: DecodeOpts) -> Result<Decoded, String> {
    decode_streaming(bytes, path, opts, &mut |_| {})
}

/// Like [`decode_bytes`], but an animation is also handed to `partial` while
/// it decodes (after the first frame, then every [`STREAM_EVERY`]), so a
/// big GIF starts playing right away.
pub fn decode_streaming(
    bytes: &[u8],
    path: &Path,
    opts: DecodeOpts,
    partial: &mut dyn FnMut(Decoded),
) -> Result<Decoded, String> {
    super::guard::catch("decoder", || decode_unguarded(bytes, path, opts, partial))
}

pub(crate) fn decode_unguarded(
    bytes: &[u8],
    path: &Path,
    opts: DecodeOpts,
    partial: &mut dyn FnMut(Decoded),
) -> Result<Decoded, String> {
    #[cfg(test)]
    if path
        .file_name()
        .is_some_and(|n| n == super::guard::PANIC_FILE)
    {
        panic!("test panic");
    }
    if super::avif::is_avif(bytes) {
        return super::avif::decode(bytes, opts);
    }
    if super::jxl::is_jxl(bytes) {
        return super::jxl::decode(bytes, path, opts, partial);
    }
    if let Some(kind) = super::formats::detect(bytes, path) {
        return super::formats::decode(kind, bytes, path, opts, partial);
    }
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

    let animation = |frames: Vec<Frame>, w: u32, h: u32, reduced: bool, complete: bool| {
        let histogram = histogram_of(&frames[0].image);
        Decoded {
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
            exif: exif.clone(),
            decode_time: Duration::ZERO,
            reduced,
            tiles: Vec::new(),
            tiled_size: [0, 0],
            profile: None,
            color_converted: false,
            complete,
            full: None,
        }
    };
    let mut stream = |frames: &[Frame], w: u32, h: u32, reduced: bool| {
        partial(animation(frames.to_vec(), w, h, reduced, false));
    };
    let animated = match format {
        ImageFormat::Gif => decode_animation(
            image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?,
            opts,
            &mut stream,
        )?,
        ImageFormat::WebP => {
            let dec = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
                .map_err(|e| e.to_string())?;
            if dec.has_animation() {
                decode_animation(dec, opts, &mut stream)?
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some((frames, w, h, reduced)) = animated {
        return Ok(animation(frames, w, h, reduced, true));
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
    let (mut image, mut tiles, reduced, kept) = finish_keep(image, opts, Orientation::default());
    let tiled_size = tiled_size(&tiles);
    let (profile, color_converted) = manage_color(icc.as_deref(), &mut image, &mut tiles);

    let histogram = histogram_of(&image);
    let alpha = color.has_alpha() && has_alpha(&image);
    let decoded = Decoded {
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
        complete: true,
        full: None,
    };
    // Zooming in later uses these pixels instead of decoding again.
    // Not when colours were converted: that would cost a full-size
    // conversion for every image, zoomed or not.
    Ok(with_full(decoded, kept.filter(|_| !color_converted)))
}

/// Attaches the full-resolution pixels to a screen-size decode.
pub(super) fn with_full(mut d: Decoded, full: Option<ColorImage>) -> Decoded {
    if let Some(img) = full {
        let mut f = d.clone();
        f.frames = vec![Frame {
            image: Arc::new(img),
            delay: Duration::ZERO,
        }];
        f.reduced = false;
        d.full = Some(Box::new(f));
    }
    d
}

/// Converts the final pixels (after shrinking, so it is cheap) from an
/// embedded profile to sRGB. Returns the profile name and whether
/// anything was converted.
pub(crate) fn manage_color(
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

/// A photo-like test JPEG: smooth gradients plus sensor-style noise, so
/// the entropy decoder has real work (flat images decode unrealistically
/// fast).
#[cfg(test)]
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
