//! Thumbnails for the grid view, made on two background threads.
//!
//! Camera JPEGs carry a small preview in their EXIF data; using it makes a
//! grid of 30 MP photos appear at once instead of decoding each one.
//! Everything else is decoded and shrunk.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

use eframe::egui::{self, ColorImage};

use super::decode::{downscale, upright, DecodeOpts, Orientation};

/// Longest side of a thumbnail, in pixels.
pub const THUMB: usize = 256;

/// The JPEG preview stored in a file's EXIF data, if any.
pub fn exif_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    use exif::{In, Tag};
    let exif = exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()?;
    let start = exif
        .get_field(Tag::JPEGInterchangeFormat, In::THUMBNAIL)?
        .value
        .get_uint(0)? as usize;
    let len = exif
        .get_field(Tag::JPEGInterchangeFormatLength, In::THUMBNAIL)?
        .value
        .get_uint(0)? as usize;
    exif.buf().get(start..start + len).map(<[u8]>::to_vec)
}

/// Cuts away black bars (cameras letterbox 3:2 photos into a 4:3 preview).
pub fn trim_black_bars(img: ColorImage) -> ColorImage {
    let [w, h] = img.size;
    let dark = |p: &egui::Color32| p.r() as u16 + p.g() as u16 + p.b() as u16 <= 24;
    let row_dark = |y: usize| img.pixels[y * w..(y + 1) * w].iter().all(dark);
    let col_dark = |x: usize| (0..h).all(|y| dark(&img.pixels[y * w + x]));
    let top = (0..h).take_while(|&y| row_dark(y)).count();
    if top == h {
        return img; // all black: leave it
    }
    let bottom = (0..h).rev().take_while(|&y| row_dark(y)).count();
    let left = (0..w).take_while(|&x| col_dark(x)).count();
    let right = (0..w).rev().take_while(|&x| col_dark(x)).count();
    if top + bottom + left + right == 0 {
        return img;
    }
    let (nw, nh) = (w - left - right, h - top - bottom);
    let mut px = Vec::with_capacity(nw * nh);
    for y in top..top + nh {
        px.extend_from_slice(&img.pixels[y * w + left..y * w + left + nw]);
    }
    ColorImage::new([nw, nh], px)
}

/// An upright thumbnail of the file, at most [`THUMB`] pixels on its long side.
pub fn make(path: &Path) -> Result<ColorImage, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let fit = |img: ColorImage| {
        let [w, h] = img.size;
        let long = w.max(h);
        if long <= THUMB {
            return img;
        }
        let s = THUMB as f64 / long as f64;
        let t = [
            ((w as f64 * s).round() as usize).max(1),
            ((h as f64 * s).round() as usize).max(1),
        ];
        downscale(&img, t)
    };
    let is_jpeg = bytes.starts_with(&[0xFF, 0xD8]);
    if is_jpeg {
        if let Some(thumb) = exif_thumbnail(&bytes) {
            let orientation =
                Orientation::from_exif(super::exif_info::read(&bytes).map_or(1, |e| e.orientation));
            let opts = DecodeOpts::full(4096);
            let preview = super::decode::decode_streaming(
                &thumb,
                Path::new("preview.jpg"),
                opts,
                &mut |_| {},
            );
            if let Some(f) = preview.ok().and_then(|d| d.frames.into_iter().next()) {
                let img = trim_black_bars((*f.image).clone());
                if img.size[0].max(img.size[1]) >= 120 {
                    return Ok(fit(upright(&img, orientation)));
                }
            }
        }
    }
    let opts = DecodeOpts {
        fit: Some([THUMB, THUMB]),
        ..DecodeOpts::full(4096)
    };
    let d = super::decode::decode_streaming(&bytes, path, opts, &mut |_| {})?;
    let frame = &d.frames.first().ok_or("no image")?.image;
    Ok(fit(upright(frame, d.orientation)))
}

struct State {
    queue: VecDeque<PathBuf>,
    shutdown: bool,
}

/// Background thumbnail maker. Ask for what is on screen with [`want`];
/// finished thumbnails arrive through [`poll`].
///
/// [`want`]: ThumbLoader::want
/// [`poll`]: ThumbLoader::poll
pub struct ThumbLoader {
    state: Arc<(Mutex<State>, Condvar)>,
    rx: Receiver<(PathBuf, Result<ColorImage, String>)>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl ThumbLoader {
    pub fn new(ctx: egui::Context, threads: usize) -> Self {
        let state = Arc::new((
            Mutex::new(State {
                queue: VecDeque::new(),
                shutdown: false,
            }),
            Condvar::new(),
        ));
        let (tx, rx) = channel();
        let threads = (0..threads.max(1))
            .map(|i| {
                let state = state.clone();
                let tx: Sender<_> = tx.clone();
                let ctx = ctx.clone();
                std::thread::Builder::new()
                    .name(format!("thumbs-{i}"))
                    .spawn(move || loop {
                        let path = {
                            let (m, cv) = &*state;
                            let mut st = m.lock().unwrap();
                            loop {
                                if st.shutdown {
                                    return;
                                }
                                if let Some(p) = st.queue.pop_front() {
                                    break p;
                                }
                                st = cv.wait(st).unwrap();
                            }
                        };
                        let result = make(&path);
                        if tx.send((path, result)).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    })
                    .expect("spawn thumbnail thread")
            })
            .collect();
        Self { state, rx, threads }
    }

    /// Replaces the queue (most important first).
    pub fn want(&self, paths: Vec<PathBuf>) {
        let (m, cv) = &*self.state;
        m.lock().unwrap().queue = paths.into();
        cv.notify_all();
    }

    pub fn poll(&self) -> Vec<(PathBuf, Result<ColorImage, String>)> {
        self.rx.try_iter().collect()
    }
}

impl Drop for ThumbLoader {
    fn drop(&mut self) {
        let (m, cv) = &*self.state;
        m.lock().unwrap().shutdown = true;
        cv.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    /// A JPEG whose EXIF holds orientation 6 and a 160x120 preview with
    /// black bars (like Canon's).
    fn jpeg_with_preview() -> Vec<u8> {
        let main = image::RgbImage::from_pixel(600, 400, image::Rgb([10, 200, 10]));
        let mut plain = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 80)
            .encode_image(&main)
            .unwrap();
        let preview = image::RgbImage::from_fn(160, 120, |_, y| {
            if (14..106).contains(&y) {
                image::Rgb([220, 30, 30])
            } else {
                image::Rgb([0, 0, 0])
            }
        });
        let mut thumb = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut thumb, 95)
            .encode_image(&preview)
            .unwrap();
        // TIFF: IFD0 (Orientation=6) -> IFD1 (thumbnail offset/length).
        let ifd0 = 8u32;
        let ifd1 = ifd0 + 2 + 12 + 4;
        let data = ifd1 + 2 + 24 + 4;
        let mut t = b"II*\0".to_vec();
        t.extend_from_slice(&ifd0.to_le_bytes());
        t.extend_from_slice(&1u16.to_le_bytes());
        t.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
        t.extend_from_slice(&ifd1.to_le_bytes());
        t.extend_from_slice(&2u16.to_le_bytes());
        t.extend_from_slice(&[0x01, 0x02, 4, 0, 1, 0, 0, 0]);
        t.extend_from_slice(&data.to_le_bytes());
        t.extend_from_slice(&[0x02, 0x02, 4, 0, 1, 0, 0, 0]);
        t.extend_from_slice(&(thumb.len() as u32).to_le_bytes());
        t.extend_from_slice(&0u32.to_le_bytes());
        t.extend_from_slice(&thumb);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&t);
        let mut out = plain[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&plain[2..]);
        out
    }

    fn temp(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "muud-thumbs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn camera_preview_is_used_trimmed_and_turned() {
        let bytes = jpeg_with_preview();
        assert!(exif_thumbnail(&bytes).is_some());
        let p = temp("cam.jpg", &bytes);
        let t = make(&p).unwrap();
        // 160x92 after trimming the bars, then turned upright (portrait).
        assert!(t.size[1] > t.size[0], "{:?}", t.size);
        assert!((150..=170).contains(&t.size[1]), "{:?}", t.size);
        // Red preview, not the green main image: it really came from EXIF.
        let c = t.pixels[t.pixels.len() / 2];
        assert!(c.r() > 150 && c.g() < 100, "{c:?}");
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }

    #[test]
    fn other_images_are_decoded_and_shrunk() {
        let img = image::RgbaImage::from_pixel(1000, 500, image::Rgba([1, 2, 3, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let p = temp("a.png", &png);
        assert_eq!(make(&p).unwrap().size, [THUMB, THUMB / 2]);
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }

    #[test]
    fn black_bars_are_trimmed() {
        let mut px = vec![Color32::BLACK; 4 * 4];
        for y in 1..3 {
            for x in 0..4 {
                px[y * 4 + x] = Color32::WHITE;
            }
        }
        let t = trim_black_bars(ColorImage::new([4, 4], px));
        assert_eq!(t.size, [4, 2]);
        let all_black = ColorImage::new([2, 2], vec![Color32::BLACK; 4]);
        assert_eq!(trim_black_bars(all_black).size, [2, 2]);
    }

    #[test]
    fn loader_makes_what_is_asked() {
        let img = image::RgbaImage::from_pixel(10, 10, image::Rgba([9, 9, 9, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let p = temp("t.png", &png);
        let l = ThumbLoader::new(egui::Context::default(), 2);
        l.want(vec![p.clone()]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut got = Vec::new();
        while got.is_empty() && std::time::Instant::now() < deadline {
            got = l.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(got[0].0, p);
        assert!(got[0].1.is_ok());
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
}
