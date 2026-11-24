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
}

impl Decoded {
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// Approximate memory held by the decoded frames.
    pub fn bytes(&self) -> usize {
        self.frames.iter().map(|f| f.image.pixels.len() * 4).sum()
    }

    /// Stored texture size (not rotated).
    pub fn texture_size(&self) -> [usize; 2] {
        self.frames.first().map(|f| f.image.size).unwrap_or([1, 1])
    }

    /// True if the texture holds fewer pixels than the image.
    pub fn is_downscaled(&self) -> bool {
        let [tw, th] = self.texture_size();
        tw.max(th) < self.width.max(self.height) as usize
    }

    /// A still image wrapped as `Decoded` (used for pasted images).
    #[allow(dead_code)] // wired up in M4
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
        }
    }
}

pub fn decode_bytes(bytes: &[u8], path: &Path, max_side: usize) -> Result<Decoded, String> {
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
        if let Ok(d) = decode_jpeg(bytes, max_side, orientation, exif.clone()) {
            return Ok(d);
        }
    }

    let animated = match format {
        ImageFormat::Gif => decode_animation(
            image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?,
            max_side,
        )?,
        ImageFormat::WebP => {
            let dec = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
                .map_err(|e| e.to_string())?;
            if dec.has_animation() {
                decode_animation(dec, max_side)?
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some((frames, w, h)) = animated {
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
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);

    let (width, height) = (img.width(), img.height());
    let img = fit_texture(img, max_side);
    let image = if color.has_alpha() {
        let rgba = img.into_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
    } else {
        // Opaque: premultiplied and unmultiplied are identical, skip the maths.
        opaque_color_image(img)
    };

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
    })
}

/// zune-jpeg straight into the texture buffer: no RGB→RGBA pass and no
/// premultiply pass (JPEG has no alpha).
fn decode_jpeg(
    bytes: &[u8],
    max_side: usize,
    orientation: Orientation,
    exif: Option<ExifInfo>,
) -> Result<Decoded, String> {
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;

    let opts = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(1 << 16)
        .set_max_height(1 << 16);
    let mut dec = zune_jpeg::JpegDecoder::new_with_options(Cursor::new(bytes), opts);
    dec.decode_headers().map_err(|e| format!("{e:?}"))?;
    let (w, h) = dec.dimensions().ok_or("no dimensions")?;
    let gray = matches!(dec.input_colorspace(), Some(ColorSpace::Luma));
    let mut pixels = vec![Color32::BLACK; w * h];
    dec.decode_into(bytemuck::cast_slice_mut(&mut pixels))
        .map_err(|e| format!("{e:?}"))?;
    let mut image = ColorImage::new([w, h], pixels);

    if w > max_side || h > max_side {
        let rgba: Vec<u8> = bytemuck::cast_slice(&image.pixels).to_vec();
        let buf = image::RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or("bad buffer")?;
        image = opaque_color_image(fit_texture(DynamicImage::ImageRgba8(buf), max_side));
    }

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
    })
}

fn opaque_color_image(img: DynamicImage) -> ColorImage {
    let rgba = img.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let mut pixels = vec![Color32::BLACK; size[0] * size[1]];
    bytemuck::cast_slice_mut::<Color32, u8>(&mut pixels).copy_from_slice(rgba.as_raw());
    ColorImage::new(size, pixels)
}

type Animation = Option<(Vec<Frame>, u32, u32)>;

fn decode_animation<'a>(
    dec: impl AnimationDecoder<'a>,
    max_side: usize,
) -> Result<Animation, String> {
    let mut frames = Vec::new();
    let mut total = 0usize;
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
        let img = fit_texture(DynamicImage::ImageRgba8(buf), max_side).into_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        total += size[0] * size[1] * 4;
        frames.push(Frame {
            image: Arc::new(ColorImage::from_rgba_unmultiplied(size, img.as_raw())),
            delay,
        });
        if total > MAX_ANIMATION_BYTES {
            break;
        }
    }
    Ok(match frames.len() {
        0 => return Err("no frames".into()),
        1 => None, // a single-frame GIF/WebP is a still image
        _ => Some((frames, w, h)),
    })
}

/// Downscales so neither side exceeds what the GPU can hold in one texture.
fn fit_texture(img: DynamicImage, max_side: usize) -> DynamicImage {
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w <= max_side && h <= max_side {
        return img;
    }
    let s = max_side as f64 / w.max(h) as f64;
    let nw = ((w as f64 * s).round() as u32).max(1);
    let nh = ((h as f64 * s).round() as u32).max(1);
    img.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
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
#[allow(dead_code)] // wired up in M4
pub fn shrink(img: &ColorImage, max_side: usize) -> ColorImage {
    let [w, h] = img.size;
    if w.max(h) <= max_side {
        return img.clone();
    }
    let rgba: Vec<u8> = img
        .pixels
        .iter()
        .flat_map(|p| p.to_srgba_unmultiplied())
        .collect();
    let buf = image::RgbaImage::from_raw(w as u32, h as u32, rgba).expect("buffer size");
    let small = fit_texture(DynamicImage::ImageRgba8(buf), max_side).into_rgba8();
    ColorImage::from_rgba_unmultiplied(
        [small.width() as usize, small.height() as usize],
        small.as_raw(),
    )
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
            8192,
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
        let d = decode_bytes(&encoded(img, ImageFormat::Jpeg), Path::new("a.jpg"), 8192).unwrap();
        assert_eq!((d.width, d.height), (64, 32));
        assert_eq!(d.format, "JPEG");
        let p = d.frames[0].image.pixels[0];
        assert!(p.r() > 180 && p.g() < 70 && p.a() == 255, "{p:?}");
    }

    #[test]
    fn detects_alpha() {
        let d = decode_bytes(&png_bytes(4, 4, [0, 0, 0, 10]), Path::new("a.png"), 8192).unwrap();
        assert!(d.has_alpha);
    }

    #[test]
    fn downscales_to_texture_limit_but_reports_original_size() {
        let d = decode_bytes(
            &png_bytes(600, 300, [1, 2, 3, 255]),
            Path::new("a.png"),
            256,
        )
        .unwrap();
        assert_eq!((d.width, d.height), (600, 300));
        assert_eq!(d.texture_size(), [256, 128]);
        assert!(d.is_downscaled());
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_bytes(b"hello", Path::new("x.txt"), 8192).is_err());
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

    /// `cargo test --release -- --ignored --nocapture bench_jpeg` with
    /// MUUD_BENCH_JPEG=path/to/big.jpg
    #[test]
    #[ignore]
    fn bench_jpeg() {
        let path = std::env::var("MUUD_BENCH_JPEG").expect("set MUUD_BENCH_JPEG");
        let bytes = std::fs::read(&path).unwrap();
        for _ in 0..3 {
            let t = std::time::Instant::now();
            let d = decode_bytes(&bytes, Path::new(&path), 16384).unwrap();
            println!(
                "{}x{} in {:.1} ms",
                d.width,
                d.height,
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}
