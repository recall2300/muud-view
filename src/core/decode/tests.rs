use super::jpeg::*;
use super::resize::*;
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
    let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([200, 60, 60])));
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

fn gif_bytes(frames: usize) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
        for i in 0..frames {
            let img = image::RgbaImage::from_pixel(16, 16, image::Rgba([i as u8 * 40, 0, 0, 255]));
            enc.encode_frame(image::Frame::from_parts(
                img,
                0,
                0,
                image::Delay::from_numer_denom_ms(50, 1),
            ))
            .unwrap();
        }
    }
    out
}

#[test]
fn animations_stream_from_the_first_frame() {
    let bytes = gif_bytes(5);
    let mut partials = Vec::new();
    let d = decode_streaming(
        &bytes,
        Path::new("a.gif"),
        DecodeOpts::full(8192),
        &mut |p| partials.push((p.frames.len(), p.complete)),
    )
    .unwrap();
    // Handed over after the first frame, before the rest were decoded.
    assert_eq!(partials.first(), Some(&(1, false)));
    assert!(partials.iter().all(|(n, c)| *n < 5 && !c));
    assert!(d.complete);
    assert_eq!(d.frames.len(), 5);
    assert!(d.is_animated());
}

#[test]
fn truncated_animation_plays_what_it_has() {
    let bytes = gif_bytes(4);
    let cut = &bytes[..bytes.len() * 3 / 4];
    let d = decode_bytes(cut, Path::new("a.gif"), DecodeOpts::full(8192)).unwrap();
    assert!(d.frames.len() >= 2, "{} frames", d.frames.len());
}

#[test]
fn screen_decode_can_keep_the_full_pixels() {
    let bytes = png_bytes(600, 300, [9, 9, 9, 255]);
    let opts = DecodeOpts {
        fit: Some([100, 100]),
        keep_full: true,
        ..DecodeOpts::full(8192)
    };
    let d = decode_bytes(&bytes, Path::new("a.png"), opts).unwrap();
    assert_eq!(d.texture_size(), [100, 50]);
    let full = d.full.as_ref().expect("full pixels kept");
    assert_eq!(full.texture_size(), [600, 300]);
    assert!(!full.reduced && full.full.is_none());
    // Same for the JPEG fast path.
    let jpeg = synthetic_photo(640, 480);
    let d = decode_bytes(&jpeg, Path::new("a.jpg"), opts).unwrap();
    assert_eq!(d.full.as_ref().unwrap().texture_size(), [640, 480]);
    // Not asked, or nothing was shrunk: nothing extra.
    let d = decode_bytes(
        &bytes,
        Path::new("a.png"),
        DecodeOpts {
            keep_full: false,
            ..opts
        },
    )
    .unwrap();
    assert!(d.full.is_none());
    let d = decode_bytes(&png_bytes(50, 50, [1, 1, 1, 255]), Path::new("b.png"), opts).unwrap();
    assert!(d.full.is_none());
}

#[test]
fn libjpeg_turbo_shrinks_while_decoding() {
    // Smallest n/8 that still covers the target.
    assert_eq!(scale_for([6960, 4640], [2160, 1440]).to_string(), "1/2");
    assert_eq!(scale_for([6960, 4640], [1600, 1000]).to_string(), "1/4");
    assert_eq!(scale_for([6960, 4640], [870, 580]).to_string(), "1/8");
    assert_eq!(scale_for([6960, 4640], [6960, 4640]).to_string(), "1/1");
    let jpeg = synthetic_photo(1600, 1200);
    let opts = DecodeOpts {
        fit: Some([400, 300]),
        ..DecodeOpts::full(8192)
    };
    let d = decode_bytes(&jpeg, Path::new("a.jpg"), opts).unwrap();
    // Shown at 400x300, decoded from a 1/4 IDCT, original size kept.
    assert_eq!(d.texture_size(), [400, 300]);
    assert_eq!((d.width, d.height), (1600, 1200));
    assert!(d.reduced);
    // Same pixels as decoding everything and shrinking (roughly).
    let full = decode_bytes(&jpeg, Path::new("a.jpg"), DecodeOpts::full(8192)).unwrap();
    let small = downscale(&full.frames[0].image, [400, 300]);
    let diff: u32 = small
        .pixels
        .iter()
        .zip(&d.frames[0].image.pixels)
        .map(|(a, b)| a.r().abs_diff(b.r()) as u32)
        .sum::<u32>()
        / (400 * 300);
    assert!(diff < 6, "mean difference {diff}");
}

#[test]
fn icc_profile_is_read_from_app2_segments() {
    let p3 = moxcms::ColorProfile::new_display_p3().encode().unwrap();
    let img = image::RgbImage::from_pixel(8, 8, image::Rgb([200, 60, 60]));
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
    use image::ImageEncoder;
    enc.set_icc_profile(p3.clone()).unwrap();
    enc.encode_image(&img).unwrap();
    assert_eq!(icc_from_jpeg(&out), Some(p3));
    let d = decode_bytes(&out, Path::new("p3.jpg"), DecodeOpts::full(8192)).unwrap();
    assert!(d.color_converted);
    assert_eq!(icc_from_jpeg(&synthetic_photo(16, 16)), None);
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

/// `cargo test --release -- --ignored --nocapture bench_gif`: time to
/// the first playable frame vs. the whole animation.
#[test]
#[ignore]
fn bench_gif() {
    let mut out = Vec::new();
    {
        let mut enc = image::codecs::gif::GifEncoder::new_with_speed(&mut out, 30);
        let mut seed = 1u32;
        for i in 0..120u32 {
            let img = image::RgbaImage::from_fn(1200, 900, |x, y| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let v = ((x + y + i * 7) % 255) as u8;
                image::Rgba([v, (seed >> 24) as u8 & 0x3f, 255 - v, 255])
            });
            enc.encode_frame(image::Frame::new(img)).unwrap();
        }
    }
    println!("gif: {:.1} MB, 120 frames 1200x900", out.len() as f64 / 1e6);
    let t = std::time::Instant::now();
    let mut first = None;
    let d = decode_streaming(
        &out,
        Path::new("a.gif"),
        DecodeOpts::full(8192),
        &mut |_| {
            first.get_or_insert(t.elapsed());
        },
    )
    .unwrap();
    println!(
        "first frame after {:.0} ms, all {} frames after {:.0} ms",
        first.unwrap().as_secs_f64() * 1000.0,
        d.frames.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
}
