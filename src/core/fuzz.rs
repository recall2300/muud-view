//! Broken-file tests: every format we open, cut short and with bytes
//! changed, fed to the decoders. A decoder may refuse such a file, and a
//! crash inside one is caught (see [`super::guard`]); what must never happen
//! is a hang or the process dying. Crashes that are not caught by a library
//! itself are counted, so a new one shows up in the test output.
//!
//! Fixed seed: the same cases run every time.

#![cfg(test)]

use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, Instant};

use image::{DynamicImage, ImageFormat};

use super::decode::{decode_unguarded, DecodeOpts};

fn encoded(img: DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), format).unwrap();
    out
}

fn gradient(w: u32, h: u32) -> image::RgbaImage {
    image::RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([
            (x * 255 / w) as u8,
            (y * 255 / h) as u8,
            ((x ^ y) & 255) as u8,
            200,
        ])
    })
}

fn gif(frames: u8) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
        for i in 0..frames {
            let img = image::RgbaImage::from_pixel(24, 16, image::Rgba([i * 60, 0, 255, 255]));
            enc.encode_frame(image::Frame::from_parts(
                img,
                0,
                0,
                image::Delay::from_numer_denom_ms(40, 1),
            ))
            .unwrap();
        }
    }
    out
}

/// A 3x2 RGB Photoshop file, raw or PackBits-compressed.
pub(crate) fn psd(rle: bool) -> Vec<u8> {
    let mut b = b"8BPS".to_vec();
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&[0; 6]);
    b.extend_from_slice(&3u16.to_be_bytes());
    b.extend_from_slice(&2u32.to_be_bytes());
    b.extend_from_slice(&3u32.to_be_bytes());
    b.extend_from_slice(&8u16.to_be_bytes());
    b.extend_from_slice(&3u16.to_be_bytes());
    b.extend_from_slice(&[0; 12]);
    if rle {
        b.extend_from_slice(&1u16.to_be_bytes());
        // Six rows (3 channels x 2): each a literal run of 3 bytes.
        for _ in 0..6 {
            b.extend_from_slice(&4u16.to_be_bytes());
        }
        for row in 0..6u8 {
            b.extend_from_slice(&[2, row * 40, row * 40 + 10, row * 40 + 20]);
        }
    } else {
        b.extend_from_slice(&0u16.to_be_bytes());
        b.extend((0..18).map(|i| i * 13));
    }
    b
}

fn raw(preview: &[u8]) -> Vec<u8> {
    let mut b = b"II*\0\x08\0\0\0".to_vec();
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&[0x55; 32]);
    b.extend_from_slice(preview);
    b.extend_from_slice(&[0x11; 16]);
    b
}

fn data(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name),
    )
    .unwrap()
}

/// One good file per format, named so the extension-based ones are found.
pub(crate) fn seeds() -> Vec<(&'static str, Vec<u8>)> {
    let rgba = DynamicImage::ImageRgba8(gradient(40, 28));
    let rgb = DynamicImage::ImageRgb8(rgba.to_rgb8());
    let jpeg = super::decode::synthetic_photo(64, 48);
    let mut progressive = Vec::new();
    {
        // Gray JPEG: another libjpeg-turbo path.
        let gray = DynamicImage::ImageLuma8(rgba.to_luma8());
        gray.write_to(&mut Cursor::new(&mut progressive), ImageFormat::Jpeg)
            .unwrap();
    }
    let float = DynamicImage::ImageRgb32F(rgb.to_rgb32f());
    vec![
        ("a.jpg", jpeg.clone()),
        ("gray.jpg", progressive),
        ("a.png", encoded(rgba.clone(), ImageFormat::Png)),
        ("a.gif", gif(3)),
        ("a.webp", encoded(rgba.clone(), ImageFormat::WebP)),
        ("a.bmp", encoded(rgba.clone(), ImageFormat::Bmp)),
        ("a.tif", encoded(rgba.clone(), ImageFormat::Tiff)),
        ("a.ico", encoded(rgba.clone(), ImageFormat::Ico)),
        ("a.tga", encoded(rgba.clone(), ImageFormat::Tga)),
        ("a.qoi", encoded(rgba.clone(), ImageFormat::Qoi)),
        ("a.ppm", encoded(rgb.clone(), ImageFormat::Pnm)),
        ("a.hdr", encoded(float.clone(), ImageFormat::Hdr)),
        ("a.exr", encoded(float, ImageFormat::OpenExr)),
        (
            "a.svg",
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20">
            <rect width="40" height="20" fill="#3060c0"/><circle cx="10" cy="10" r="6" fill="red"/>
            <path d="M0 0 L40 20" stroke="black"/><text x="2" y="18" font-size="6">muud</text></svg>"##
                .to_vec(),
        ),
        ("a.psd", psd(false)),
        ("rle.psd", psd(true)),
        ("a.cr2", raw(&jpeg)),
        ("lossy.jxl", data("lossy.jxl")),
        ("alpha.jxl", data("alpha.jxl")),
        ("anim.jxl", data("anim.jxl")),
        ("photo.avif", data("photo.avif")),
        ("alpha.avif", data("alpha.avif")),
        ("rot.avif", data("rot.avif")),
        ("grid.avif", super::avif::tests::grid_of(&data("photo.avif"))),
    ]
}

/// xorshift64: a fixed, repeatable sequence.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Broken versions of `good`: cut short at many points, a few bytes
/// changed, a run of bytes overwritten, a header field turned huge.
fn broken(good: &[u8], rng: &mut Rng) -> Vec<Vec<u8>> {
    let n = good.len();
    let mut out = Vec::new();
    for i in 0..24 {
        out.push(good[..n * i / 24].to_vec());
    }
    for flips in [1, 2, 4, 16, 64] {
        for _ in 0..12 {
            let mut b = good.to_vec();
            for _ in 0..flips {
                let at = rng.below(n);
                b[at] ^= 1 << rng.below(8);
            }
            out.push(b);
        }
    }
    for _ in 0..16 {
        // Header area: where sizes and offsets live.
        let mut b = good.to_vec();
        let at = rng.below(n.min(256));
        let len = 1 + rng.below(4);
        for x in b.iter_mut().skip(at).take(len) {
            *x = [0xFF, 0x00, 0x7F, 0x80][rng.below(4)];
        }
        out.push(b);
    }
    for _ in 0..8 {
        let mut b = good.to_vec();
        let at = rng.below(n);
        let len = rng.below(64).min(n - at);
        for x in &mut b[at..at + len] {
            *x = rng.next() as u8;
        }
        out.push(b);
    }
    out
}

/// One case in its own process, for finding crashes that abort:
/// `MUUD_FUZZ_ONE=name:index cargo test fuzz_one -- --ignored`.
#[test]
#[ignore]
fn fuzz_one() {
    let Ok(which) = std::env::var("MUUD_FUZZ_ONE") else {
        return;
    };
    let (want, index) = which.split_once(':').unwrap();
    let index: usize = index.parse().unwrap();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for (name, good) in seeds() {
        let cases = broken(&good, &mut rng);
        if name == want {
            let o = DecodeOpts::full(8192);
            let _ = decode_unguarded(&cases[index], Path::new(name), o, &mut |_| {});
            return;
        }
    }
}

#[test]
fn broken_files_never_hang_or_take_the_app_down() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let opts = [
        DecodeOpts::full(8192),
        DecodeOpts {
            fit: Some([32, 32]),
            ..DecodeOpts::full(8192)
        },
    ];
    let mut crashes = Vec::new();
    let mut cases = 0;
    let started = Instant::now();
    for (name, good) in seeds() {
        // The good file itself decodes (so the broken ones test something).
        let ok = decode_unguarded(&good, Path::new(name), opts[0], &mut |_| {});
        assert!(ok.is_ok(), "{name}: {:?}", ok.err());
        for (i, bad) in broken(&good, &mut rng).into_iter().enumerate() {
            let o = opts[i % 2];
            let t = Instant::now();
            let r = catch_unwind(AssertUnwindSafe(|| {
                decode_unguarded(&bad, Path::new(name), o, &mut |_| {})
            }));
            cases += 1;
            if r.is_err() {
                crashes.push(format!("{name} #{i}"));
            }
            assert!(
                t.elapsed() < Duration::from_secs(5),
                "{name} case {i} took {:?}",
                t.elapsed()
            );
        }
    }
    eprintln!("broken files: {cases} cases in {:?}", started.elapsed());
    assert!(cases > 1500);
    // Caught, so harmless to the app, but each one is a library trusting a
    // broken file: check such input before handing it over (as for PSD).
    assert!(
        crashes.is_empty(),
        "decoder crashes on broken files: {crashes:?}"
    );
}

#[test]
fn broken_archives_never_hang_or_take_the_app_down() {
    use super::archive;
    use std::io::Write;
    let png = encoded(DynamicImage::ImageRgba8(gradient(16, 16)), ImageFormat::Png);
    // A deflated zip (what most tools write) and a stored one with raw
    // Korean names (old Windows tools).
    let mut deflated = Vec::new();
    {
        let mut z = zip::ZipWriter::new(Cursor::new(&mut deflated));
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for name in ["01.png", "02.png", "sub/03.png"] {
            z.start_file(name, o).unwrap();
            z.write_all(&png).unwrap();
        }
        z.finish().unwrap();
    }
    let stored = archive::tests::raw_zip(&[(b"\xc7\xd1.png", &png), (b"b.png", &png)]);
    let dir = archive::tests::temp_dir("fuzz-zip");
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut crashes = Vec::new();
    let mut cases = 0;
    for (tag, good) in [("deflated", deflated), ("stored", stored)] {
        for (i, bad) in broken(&good, &mut rng).into_iter().enumerate() {
            let p = dir.join(format!("{tag}-{i}.cbz"));
            std::fs::write(&p, &bad).unwrap();
            let t = Instant::now();
            let r = catch_unwind(AssertUnwindSafe(|| {
                for entry in archive::list(&p).unwrap_or_default() {
                    if let Ok(bytes) = archive::read(&entry) {
                        let _ =
                            decode_unguarded(&bytes, &entry, DecodeOpts::full(8192), &mut |_| {});
                    }
                }
            }));
            cases += 1;
            if r.is_err() {
                crashes.push(format!("{tag} #{i}"));
            }
            assert!(t.elapsed() < Duration::from_secs(5), "{tag} case {i}");
        }
    }
    archive::close_all();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(cases > 200);
    assert!(
        crashes.is_empty(),
        "archive crashes on broken files: {crashes:?}"
    );
}
