//! JPEG XL, with jxl-oxide (pure Rust, MIT OR Apache-2.0). Colours are
//! rendered to sRGB (through moxcms, which the other formats use too);
//! animations play like GIFs.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{Color32, ColorImage};
use jxl_oxide::{
    color::ColourSpace, AllocTracker, EnumColourEncoding, JxlImage, Moxcms, RenderingIntent,
};

use super::decode::{
    fit_for_display, histogram_of, DecodeOpts, Decoded, Frame, Orientation, MAX_ANIMATION_BYTES,
    STREAM_EVERY,
};

/// Bare codestream (`FF 0A`) or the ISO BMFF container.
pub fn is_jxl(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0x0A])
        || bytes.starts_with(&[
            0, 0, 0, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A,
        ])
}

/// Most memory the decoder may take for one picture.
const MAX_ALLOC: usize = 4 << 30;

pub fn decode(
    bytes: &[u8],
    _path: &Path,
    opts: DecodeOpts,
    partial: &mut dyn FnMut(Decoded),
) -> Result<Decoded, String> {
    let mut image = JxlImage::builder()
        .alloc_tracker(AllocTracker::with_limit(MAX_ALLOC))
        .read(Cursor::new(bytes))
        .map_err(|e| e.to_string())?;
    let metadata = &image.image_header().metadata;
    let encoding = &metadata.colour_encoding;
    let gray = encoding.colour_space() == ColourSpace::Grey;
    let converted = !gray && !encoding.is_srgb();
    let bits = metadata.bit_depth.bits_per_sample();
    let tick = metadata.animation.as_ref().map(|a| {
        // Seconds per tick = denominator / numerator.
        Duration::from_secs_f64(a.tps_denominator as f64 / a.tps_numerator.max(1) as f64)
    });
    let profile = image
        .original_icc()
        .and_then(|icc| super::color::prepare(icc).name().map(str::to_owned));
    image.set_cms(Moxcms);
    image.request_color_encoding(EnumColourEncoding::srgb(RenderingIntent::Relative));

    let (width, height) = (image.width(), image.height());
    let exif = exif(&image);
    let keyframes = image.num_loaded_keyframes();
    if keyframes == 0 {
        return Err("no complete frame in this JPEG XL file".into());
    }

    let base = |frames: Vec<Frame>, reduced: bool, complete: bool, alpha: bool| Decoded {
        histogram: histogram_of(&frames[0].image),
        frames,
        width,
        height,
        orientation: Orientation::default(),
        format: "JPEG XL",
        color: format!("{} {bits}", if gray { "GRAY" } else { "RGB" })
            + if alpha { " + alpha" } else { "" },
        has_alpha: alpha,
        file_size: 0,
        modified: None,
        exif: exif.clone(),
        decode_time: Duration::ZERO,
        reduced,
        tiles: Vec::new(),
        tiled_size: [0, 0],
        profile: profile.clone(),
        color_converted: converted,
        complete,
        full: None,
    };

    let animated = keyframes > 1 && tick.is_some();
    if !animated {
        let (img, alpha) = render(&image, 0)?;
        // Orientation is already applied by the renderer.
        let (img, tiles, reduced, kept) =
            super::decode::finish_keep(img, opts, Orientation::default());
        let mut d = base(
            vec![Frame {
                image: Arc::new(img),
                delay: Duration::ZERO,
            }],
            reduced,
            true,
            alpha,
        );
        d.tiled_size = super::decode::tiled_size(&tiles);
        d.tiles = tiles;
        return Ok(super::decode::with_full(d, kept));
    }

    let tick = tick.unwrap_or(Duration::from_millis(100));
    let mut frames = Vec::new();
    let mut total = 0usize;
    let mut reduced = false;
    let mut alpha = false;
    let mut sent: Option<std::time::Instant> = None;
    for i in 0..keyframes {
        let ticks = image.frame_header(i).map_or(1, |h| h.duration);
        let (img, a) = match render(&image, i) {
            Ok(r) => r,
            // A damaged later frame: play the ones we have.
            Err(_) if frames.len() > 1 => break,
            Err(e) => return Err(e),
        };
        alpha |= a;
        let (img, r) = fit_for_display(img, opts, Orientation::default());
        reduced |= r;
        total += img.pixels.len() * 4;
        let ms = (tick * ticks).as_millis() as u64;
        frames.push(Frame {
            image: Arc::new(img),
            // Like browsers (and our GIFs): very short delays are 100 ms.
            delay: Duration::from_millis(if ms < 20 { 100 } else { ms }),
        });
        if total > MAX_ANIMATION_BYTES {
            break;
        }
        if sent.is_none_or(|t| t.elapsed() >= STREAM_EVERY) {
            partial(base(frames.clone(), reduced, false, alpha));
            sent = Some(std::time::Instant::now());
        }
    }
    Ok(base(frames, reduced, true, alpha))
}

/// One keyframe as RGBA (premultiplied, as egui keeps it), and whether any
/// of it is see-through.
fn render(image: &JxlImage, keyframe: usize) -> Result<(ColorImage, bool), String> {
    let render = image.render_frame(keyframe).map_err(|e| e.to_string())?;
    let mut stream = render.stream();
    let (w, h, c) = (
        stream.width() as usize,
        stream.height() as usize,
        stream.channels() as usize,
    );
    let mut buf = vec![0u8; w * h * c];
    stream.write_to_buffer(&mut buf);
    let mut alpha = false;
    let pixels = buf
        .chunks_exact(c)
        .map(|p| match *p {
            [l] => Color32::from_gray(l),
            [l, a] => {
                alpha |= a != 255;
                Color32::from_rgba_unmultiplied(l, l, l, a)
            }
            [r, g, b] => Color32::from_rgb(r, g, b),
            [r, g, b, a, ..] => {
                alpha |= a != 255;
                Color32::from_rgba_unmultiplied(r, g, b, a)
            }
            _ => Color32::BLACK,
        })
        .collect();
    Ok((ColorImage::new([w, h], pixels), alpha))
}

/// The Exif box, if the file has one: a TIFF block after a 4-byte offset.
fn exif(image: &JxlImage) -> Option<super::exif_info::ExifInfo> {
    let jxl_oxide::AuxBoxData::Data(raw) = image.aux_boxes().first_exif().ok()? else {
        return None;
    };
    let tiff = raw.payload().get(raw.tiff_header_offset() as usize..)?;
    super::exif_info::read_tiff(tiff.to_vec()).filter(|e| !e.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/data")
                .join(name),
        )
        .unwrap()
    }

    fn open(name: &str, opts: DecodeOpts) -> Decoded {
        let b = data(name);
        assert!(is_jxl(&b), "{name}");
        crate::core::decode::decode_bytes(&b, Path::new(name), opts).unwrap()
    }

    fn rgb(d: &Decoded, i: usize) -> (u8, u8, u8) {
        let p = d.frames[0].image.pixels[i];
        (p.r(), p.g(), p.b())
    }

    #[test]
    fn lossy_jpeg_xl_is_shown_in_srgb() {
        let d = open("lossy.jxl", DecodeOpts::full(8192));
        assert_eq!((d.width, d.height, d.format), (64, 48, "JPEG XL"));
        assert!(!d.has_alpha);
        // The red corner, through XYB and back.
        let (r, g, b) = rgb(&d, 2 * 64 + 2);
        assert!(r > 230 && g < 30 && b < 30, "{:?}", (r, g, b));
        // Screen size: shrunk.
        let small = DecodeOpts {
            fit: Some([32, 32]),
            ..DecodeOpts::full(8192)
        };
        let d = open("lossy.jxl", small);
        assert_eq!(d.texture_size(), [32, 24]);
        assert!(d.reduced);
    }

    #[test]
    fn jpeg_xl_alpha_grayscale_and_wide_gamut() {
        let d = open("alpha.jxl", DecodeOpts::full(8192));
        assert!(d.has_alpha);
        assert_eq!(rgb(&d, 0), (200, 30, 90));
        assert_eq!(d.frames[0].image.pixels[15].a(), 0);

        let d = open("gray.jxl", DecodeOpts::full(8192));
        assert_eq!((d.width, d.height), (24, 16));
        assert!(d.color.starts_with("GRAY"), "{}", d.color);
        assert_eq!(rgb(&d, 5), (50, 50, 50));

        // Display P3 orange (200, 100, 50) is more saturated in sRGB.
        let d = open("p3.jxl", DecodeOpts::full(8192));
        assert!(d.color_converted);
        let (r, g, b) = rgb(&d, 0);
        assert!(r > 205 && g < 100 && b < 50, "{:?}", (r, g, b));
    }

    #[test]
    fn jpeg_xl_animation_plays() {
        let b = data("anim.jxl");
        let d = crate::core::decode::decode_bytes(&b, Path::new("a.jxl"), DecodeOpts::full(8192))
            .unwrap();
        assert_eq!(d.frames.len(), 3);
        assert!(d.complete);
        let reds: Vec<u8> = d.frames.iter().map(|f| f.image.pixels[0].r()).collect();
        assert_eq!(reds, [0, 100, 200]);
        assert!(d
            .frames
            .iter()
            .all(|f| f.delay >= Duration::from_millis(20)));
    }
}

#[cfg(test)]
mod timing {
    /// `MUUD_DECODE_BENCH=file.jxl cargo test --release decode_timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn decode_timing() {
        use crate::core::decode::{decode_bytes, DecodeOpts};
        let Some(p) = std::env::var_os("MUUD_DECODE_BENCH") else {
            return;
        };
        let b = std::fs::read(&p).unwrap();
        let p = std::path::Path::new(&p);
        for (name, o) in [
            ("full", DecodeOpts::full(16384)),
            (
                "screen",
                DecodeOpts {
                    fit: Some([2560, 1440]),
                    ..DecodeOpts::full(16384)
                },
            ),
        ] {
            for _ in 0..3 {
                let t = std::time::Instant::now();
                decode_bytes(&b, p, o).unwrap();
                eprintln!("{name}: {:?}", t.elapsed());
            }
        }
    }
}
