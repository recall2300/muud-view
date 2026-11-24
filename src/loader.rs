//! Background decoding.
//!
//! The UI thread never decodes. It tells the [`Loader`] which files it wants,
//! most important first, and worker threads decode them into ready-to-upload
//! [`ColorImage`]s.

use std::collections::{HashSet, VecDeque};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{self, ColorImage};
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat};

use crate::exif_info::{self, ExifInfo};

/// Upper bound on decoded animation frames kept in memory (bytes).
const MAX_ANIMATION_BYTES: usize = 768 * 1024 * 1024;

pub struct Frame {
    pub image: Arc<ColorImage>,
    pub delay: Duration,
}

pub struct Decoded {
    pub frames: Vec<Frame>,
    /// Size after EXIF orientation, before any downscaling for the GPU.
    pub width: u32,
    pub height: u32,
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

    /// Size of the texture that is actually shown (may be smaller than the image).
    pub fn texture_size(&self) -> [usize; 2] {
        self.frames.first().map(|f| f.image.size).unwrap_or([1, 1])
    }
}

pub type LoadResult = Result<Arc<Decoded>, String>;

struct State {
    jobs: VecDeque<PathBuf>,
    in_flight: HashSet<PathBuf>,
    shutdown: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    max_side: AtomicUsize,
}

pub struct Loader {
    shared: Arc<Shared>,
    results: Receiver<(PathBuf, LoadResult)>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

impl Loader {
    pub fn new(ctx: egui::Context, threads: usize) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                jobs: VecDeque::new(),
                in_flight: HashSet::new(),
                shutdown: false,
            }),
            wake: Condvar::new(),
            max_side: AtomicUsize::new(8192),
        });
        let (tx, rx) = channel();
        let workers = (0..threads.max(1))
            .map(|i| {
                let shared = shared.clone();
                let tx: Sender<(PathBuf, LoadResult)> = tx.clone();
                let ctx = ctx.clone();
                std::thread::Builder::new()
                    .name(format!("decoder-{i}"))
                    .spawn(move || worker(shared, tx, ctx))
                    .expect("spawn decoder thread")
            })
            .collect();
        Self {
            shared,
            results: rx,
            workers,
        }
    }

    pub fn set_max_texture_side(&self, side: usize) {
        self.shared.max_side.store(side.max(256), Ordering::Relaxed);
    }

    /// Replaces the queue. `paths` are in priority order; paths already being
    /// decoded are not queued again.
    pub fn want(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut st = self.shared.state.lock().unwrap();
        st.jobs.clear();
        for p in paths {
            if !st.in_flight.contains(&p) && !st.jobs.contains(&p) {
                st.jobs.push_back(p);
            }
        }
        drop(st);
        self.shared.wake.notify_all();
    }

    pub fn poll(&self) -> Vec<(PathBuf, LoadResult)> {
        self.results.try_iter().collect()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().shutdown = true;
        self.shared.wake.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn worker(shared: Arc<Shared>, tx: Sender<(PathBuf, LoadResult)>, ctx: egui::Context) {
    loop {
        let path = {
            let mut st = shared.state.lock().unwrap();
            loop {
                if st.shutdown {
                    return;
                }
                if let Some(p) = st.jobs.pop_front() {
                    st.in_flight.insert(p.clone());
                    break p;
                }
                st = shared.wake.wait(st).unwrap();
            }
        };
        let max_side = shared.max_side.load(Ordering::Relaxed);
        let result = decode_file(&path, max_side).map(Arc::new);
        shared.state.lock().unwrap().in_flight.remove(&path);
        if tx.send((path, result)).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

pub fn decode_file(path: &Path, max_side: usize) -> Result<Decoded, String> {
    let started = Instant::now();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let meta = std::fs::metadata(path).ok();
    let mut decoded = decode_bytes(&bytes, path, max_side)?;
    decoded.file_size = meta.as_ref().map(|m| m.len()).unwrap_or(bytes.len() as u64);
    decoded.modified = meta.and_then(|m| m.modified().ok());
    decoded.decode_time = started.elapsed();
    Ok(decoded)
}

pub fn decode_bytes(bytes: &[u8], path: &Path, max_side: usize) -> Result<Decoded, String> {
    let format = image::guess_format(bytes)
        .ok()
        .or_else(|| ImageFormat::from_path(path).ok())
        .ok_or_else(|| "unknown image format".to_string())?;

    let exif = match format {
        ImageFormat::Jpeg | ImageFormat::Tiff | ImageFormat::WebP | ImageFormat::Png => {
            exif_info::read(bytes).filter(|e| !e.is_empty())
        }
        _ => None,
    };

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
    let rgba = img.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let image = Arc::new(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()));
    drop(rgba);

    let histogram = histogram_of(&image);
    let alpha = color.has_alpha() && has_alpha(&image);
    Ok(Decoded {
        frames: vec![Frame {
            image,
            delay: Duration::ZERO,
        }],
        width,
        height,
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

fn has_alpha(img: &ColorImage) -> bool {
    img.pixels.iter().any(|p| p.a() < 255)
}

fn histogram_of(img: &ColorImage) -> Vec<u32> {
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

    fn png_bytes(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba(rgba));
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
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
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_bytes(b"hello", Path::new("x.txt"), 8192).is_err());
    }

    #[test]
    fn loader_decodes_in_background() {
        let dir = std::env::temp_dir().join(format!("muud-loader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("one.png");
        std::fs::write(&p, png_bytes(8, 8, [255, 0, 0, 255])).unwrap();
        let loader = Loader::new(egui::Context::default(), 2);
        loader.want([p.clone()]);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut got = None;
        while got.is_none() && Instant::now() < deadline {
            got = loader.poll().into_iter().next();
            std::thread::sleep(Duration::from_millis(5));
        }
        let (path, res) = got.expect("decoded in time");
        assert_eq!(path, p);
        let d = res.unwrap();
        assert_eq!(d.file_size, std::fs::metadata(&p).unwrap().len());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
