//! Background reading and decoding.
//!
//! The UI thread never touches the disk or decodes. Two stages run in the
//! background:
//!
//! - **Read:** one reader thread keeps the raw bytes of the next files in
//!   memory (the [`Library`](super::library::Library) asks for about ten), so
//!   a slow disk or a NAS is read while you look at the current image.
//! - **Decode:** one worker per CPU core turns bytes into ready-to-upload
//!   pixels. A worker takes the bytes from the read cache when they are
//!   there, and otherwise reads the file itself rather than wait.
//!
//! Jobs come in two qualities: [`Quality::Screen`] shrinks the image to the
//! screen (what fit-to-window needs, a fraction of the memory) and
//! [`Quality::Full`] keeps every pixel, for when you zoom in.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use eframe::egui;

pub use super::decode::{DecodeOpts, Decoded};

pub type LoadResult = Result<Arc<Decoded>, String>;

/// Raw file bytes kept in memory ahead of decoding.
pub const READ_BUDGET: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Quality {
    /// Shrunk to the screen.
    Screen,
    /// Every pixel the GPU can hold.
    Full,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Job {
    pub path: PathBuf,
    pub quality: Quality,
}

impl Job {
    pub fn screen(path: PathBuf) -> Self {
        Self {
            path,
            quality: Quality::Screen,
        }
    }

    pub fn full(path: PathBuf) -> Self {
        Self {
            path,
            quality: Quality::Full,
        }
    }
}

pub struct Done {
    pub path: PathBuf,
    pub quality: Quality,
    pub result: LoadResult,
}

/// File bytes read ahead of time. Only the files on the current read list
/// are kept, and never more than the budget.
#[derive(Default)]
pub struct ByteCache {
    map: HashMap<PathBuf, Arc<Vec<u8>>>,
    budget: usize,
}

impl ByteCache {
    pub fn new(budget: usize) -> Self {
        Self {
            map: HashMap::new(),
            budget,
        }
    }

    pub fn total(&self) -> usize {
        self.map.values().map(|b| b.len()).sum()
    }

    pub fn get(&self, path: &Path) -> Option<Arc<Vec<u8>>> {
        self.map.get(path).cloned()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.map.contains_key(path)
    }

    pub fn insert(&mut self, path: PathBuf, bytes: Arc<Vec<u8>>) {
        self.map.insert(path, bytes);
    }

    /// Forgets every file that is not in `keep`.
    pub fn retain(&mut self, keep: &[PathBuf]) {
        let keep: HashSet<&PathBuf> = keep.iter().collect();
        self.map.retain(|p, _| keep.contains(p));
    }

    /// The first file in `order` that is not cached yet, unless the cache is
    /// already full.
    pub fn next_to_read<'a>(&self, order: &'a [PathBuf]) -> Option<&'a PathBuf> {
        if self.total() >= self.budget {
            return None;
        }
        order.iter().find(|p| !self.map.contains_key(*p))
    }
}

struct State {
    jobs: VecDeque<Job>,
    in_flight: HashSet<Job>,
    /// Files to keep read, most important first.
    reads: Vec<PathBuf>,
    /// The file the reader thread is reading right now.
    reading: Option<PathBuf>,
    bytes: ByteCache,
    shutdown: bool,
}

struct Shared {
    state: Mutex<State>,
    /// Wakes decoders (new jobs, or bytes they wait for arrived).
    wake: Condvar,
    /// Wakes the reader (new read list, or space freed).
    wake_reader: Condvar,
    max_side: AtomicUsize,
    screen_w: AtomicUsize,
    screen_h: AtomicUsize,
    color: AtomicBool,
}

impl Shared {
    fn opts(&self, quality: Quality) -> DecodeOpts {
        let max_side = self.max_side.load(Ordering::Relaxed);
        let color = self.color.load(Ordering::Relaxed);
        let opts = match quality {
            // Full quality draws huge images in tiles instead of shrinking.
            Quality::Full => DecodeOpts::tiled(max_side),
            Quality::Screen => {
                let w = self.screen_w.load(Ordering::Relaxed);
                let h = self.screen_h.load(Ordering::Relaxed);
                DecodeOpts {
                    fit: (w > 0 && h > 0).then_some([w, h]),
                    ..DecodeOpts::full(max_side)
                }
            }
        };
        DecodeOpts { color, ..opts }
    }
}

pub struct Loader {
    shared: Arc<Shared>,
    results: Receiver<Done>,
    workers: Vec<std::thread::JoinHandle<()>>,
    decoders: usize,
}

impl Loader {
    pub fn new(ctx: egui::Context, threads: usize) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                jobs: VecDeque::new(),
                in_flight: HashSet::new(),
                reads: Vec::new(),
                reading: None,
                bytes: ByteCache::new(READ_BUDGET),
                shutdown: false,
            }),
            wake: Condvar::new(),
            wake_reader: Condvar::new(),
            max_side: AtomicUsize::new(8192),
            screen_w: AtomicUsize::new(0),
            screen_h: AtomicUsize::new(0),
            color: AtomicBool::new(true),
        });
        let (tx, rx) = channel();
        let mut workers: Vec<_> = (0..threads.max(1))
            .map(|i| {
                let shared = shared.clone();
                let tx: Sender<Done> = tx.clone();
                let ctx = ctx.clone();
                std::thread::Builder::new()
                    .name(format!("decoder-{i}"))
                    .spawn(move || worker(shared, tx, ctx))
                    .expect("spawn decoder thread")
            })
            .collect();
        let reader_shared = shared.clone();
        workers.push(
            std::thread::Builder::new()
                .name("reader".into())
                .spawn(move || reader(reader_shared))
                .expect("spawn reader thread"),
        );
        Self {
            shared,
            results: rx,
            workers,
            decoders: threads.max(1),
        }
    }

    pub fn decoders(&self) -> usize {
        self.decoders
    }

    pub fn set_max_texture_side(&self, side: usize) {
        self.shared.max_side.store(side.max(256), Ordering::Relaxed);
    }

    pub fn max_texture_side(&self) -> usize {
        self.shared.max_side.load(Ordering::Relaxed)
    }

    /// The screen size in physical pixels, for [`Quality::Screen`].
    pub fn set_screen_size(&self, size: [usize; 2]) {
        self.shared.screen_w.store(size[0], Ordering::Relaxed);
        self.shared.screen_h.store(size[1], Ordering::Relaxed);
    }

    /// Colour management on or off (takes effect for new decodes).
    pub fn set_color_management(&self, on: bool) {
        self.shared.color.store(on, Ordering::Relaxed);
    }

    /// Replaces the decode queue. `jobs` are in priority order; jobs already
    /// running are not queued again.
    pub fn want(&self, jobs: impl IntoIterator<Item = Job>) {
        let mut st = self.shared.state.lock().unwrap();
        st.jobs.clear();
        for j in jobs {
            if !st.in_flight.contains(&j) && !st.jobs.contains(&j) {
                st.jobs.push_back(j);
            }
        }
        drop(st);
        self.shared.wake.notify_all();
    }

    /// Replaces the read-ahead list (most important first). Bytes of files
    /// no longer on it are dropped.
    pub fn read_ahead(&self, paths: Vec<PathBuf>) {
        let mut st = self.shared.state.lock().unwrap();
        st.bytes.retain(&paths);
        st.reads = paths;
        drop(st);
        self.shared.wake_reader.notify_all();
    }

    /// Files whose bytes are in memory (for diagnostics).
    pub fn read_cache_len(&self) -> usize {
        self.shared.state.lock().unwrap().bytes.map.len()
    }

    pub fn poll(&self) -> Vec<Done> {
        self.results.try_iter().collect()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().shutdown = true;
        self.shared.wake.notify_all();
        self.shared.wake_reader.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn reader(shared: Arc<Shared>) {
    loop {
        let path = {
            let mut st = shared.state.lock().unwrap();
            loop {
                if st.shutdown {
                    return;
                }
                let next = st.bytes.next_to_read(&st.reads).cloned();
                if let Some(p) = next {
                    st.reading = Some(p.clone());
                    break p;
                }
                st = shared.wake_reader.wait(st).unwrap();
            }
        };
        let bytes = super::archive::read(&path).ok().map(Arc::new);
        let mut st = shared.state.lock().unwrap();
        st.reading = None;
        match bytes {
            Some(b) => st.bytes.insert(path, b),
            // Unreadable: drop it from the list so we do not spin on it; the
            // decoder reports the error when it gets there.
            None => st.reads.retain(|p| p != &path),
        }
        drop(st);
        shared.wake.notify_all();
    }
}

fn worker(shared: Arc<Shared>, tx: Sender<Done>, ctx: egui::Context) {
    loop {
        let (job, cached) = {
            let mut st = shared.state.lock().unwrap();
            loop {
                if st.shutdown {
                    return;
                }
                if let Some(j) = st.jobs.pop_front() {
                    // The reader is fetching exactly this file: wait for it
                    // instead of reading it twice (matters on a NAS).
                    while st.reading.as_ref() == Some(&j.path) && !st.shutdown {
                        st = shared.wake.wait(st).unwrap();
                    }
                    let cached = st.bytes.get(&j.path);
                    st.in_flight.insert(j.clone());
                    break (j, cached);
                }
                st = shared.wake.wait(st).unwrap();
            }
        };
        let opts = shared.opts(job.quality);
        let started = Instant::now();
        let bytes = match cached {
            Some(b) => Ok(b),
            None => super::archive::read(&job.path)
                .map(Arc::new)
                .map_err(|e| e.to_string()),
        };
        // Animations arrive in parts so they start playing at once.
        let mut partial = |mut d: Decoded| {
            d.file_size = bytes.as_ref().map(|b| b.len() as u64).unwrap_or(0);
            let _ = tx.send(Done {
                path: job.path.clone(),
                quality: job.quality,
                result: Ok(Arc::new(d)),
            });
            ctx.request_repaint();
        };
        let result = bytes
            .clone()
            .and_then(|b| decode_with_meta(&b, &job.path, opts, started, &mut partial))
            .map(Arc::new);
        {
            let mut st = shared.state.lock().unwrap();
            st.in_flight.remove(&job);
            // Keep what we read ourselves if it is on the read list, so a
            // later full-quality decode does not hit the disk again.
            if let Ok(b) = bytes {
                if !st.bytes.contains(&job.path) && st.reads.contains(&job.path) {
                    st.bytes.insert(job.path.clone(), b);
                }
            }
        }
        let done = Done {
            path: job.path,
            quality: job.quality,
            result,
        };
        if tx.send(done).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

fn decode_with_meta(
    bytes: &[u8],
    path: &Path,
    opts: DecodeOpts,
    started: Instant,
    partial: &mut dyn FnMut(Decoded),
) -> Result<Decoded, String> {
    let meta = std::fs::metadata(path).ok();
    let mut decoded = super::decode::decode_streaming(bytes, path, opts, partial)?;
    decoded.file_size = meta.as_ref().map(|m| m.len()).unwrap_or(bytes.len() as u64);
    decoded.modified = meta.and_then(|m| m.modified().ok());
    decoded.decode_time = started.elapsed();
    Ok(decoded)
}

/// Reads and decodes a file on the calling thread.
pub fn decode_file(path: &Path, opts: DecodeOpts) -> Result<Decoded, String> {
    let started = Instant::now();
    let bytes = super::archive::read(path).map_err(|e| e.to_string())?;
    decode_with_meta(&bytes, path, opts, started, &mut |_| {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn png_bytes(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba(rgba));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "muud-loader-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wait_done(loader: &Loader) -> Done {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(d) = loader.poll().into_iter().next() {
                return d;
            }
            assert!(Instant::now() < deadline, "decoded in time");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn loader_decodes_in_background() {
        let dir = temp_dir("bg");
        let p = dir.join("one.png");
        std::fs::write(&p, png_bytes(8, 8, [255, 0, 0, 255])).unwrap();
        let loader = Loader::new(egui::Context::default(), 2);
        loader.want([Job::screen(p.clone())]);
        let done = wait_done(&loader);
        assert_eq!(done.path, p);
        assert_eq!(done.quality, Quality::Screen);
        let d = done.result.unwrap();
        assert_eq!(d.file_size, std::fs::metadata(&p).unwrap().len());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn screen_and_full_quality() {
        let dir = temp_dir("quality");
        let p = dir.join("big.png");
        std::fs::write(&p, png_bytes(400, 200, [1, 2, 3, 255])).unwrap();
        let loader = Loader::new(egui::Context::default(), 1);
        loader.set_screen_size([100, 100]);
        loader.want([Job::screen(p.clone())]);
        let d = wait_done(&loader).result.unwrap();
        assert_eq!(d.texture_size(), [100, 50]);
        assert!(d.reduced);
        loader.want([Job::full(p.clone())]);
        let done = wait_done(&loader);
        assert_eq!(done.quality, Quality::Full);
        let d = done.result.unwrap();
        assert_eq!(d.texture_size(), [400, 200]);
        assert!(!d.reduced);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_ahead_and_forgets_old_files() {
        let dir = temp_dir("read");
        let files: Vec<PathBuf> = (0..4)
            .map(|i| {
                let p = dir.join(format!("{i}.png"));
                std::fs::write(&p, png_bytes(4, 4, [i, 0, 0, 255])).unwrap();
                p
            })
            .collect();
        let loader = Loader::new(egui::Context::default(), 1);
        loader.read_ahead(files.clone());
        let deadline = Instant::now() + Duration::from_secs(10);
        while loader.read_cache_len() < 4 {
            assert!(Instant::now() < deadline, "read in time");
            std::thread::sleep(Duration::from_millis(5));
        }
        loader.read_ahead(files[2..].to_vec());
        assert_eq!(loader.read_cache_len(), 2);
        // Decoding a read-ahead file works even after the file is gone.
        std::fs::remove_file(&files[3]).unwrap();
        loader.want([Job::screen(files[3].clone())]);
        let done = wait_done(&loader);
        assert!(done.result.is_ok(), "{:?}", done.result.err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn animations_arrive_in_parts() {
        let dir = temp_dir("anim");
        let p = dir.join("a.gif");
        let mut gif = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut gif);
            for i in 0..3u8 {
                let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([i * 80, 0, 0, 255]));
                enc.encode_frame(image::Frame::new(img)).unwrap();
            }
        }
        std::fs::write(&p, gif).unwrap();
        let loader = Loader::new(egui::Context::default(), 1);
        loader.want([Job::screen(p.clone())]);
        let mut got: Vec<Arc<Decoded>> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !got.last().is_some_and(|d| d.complete) {
            assert!(Instant::now() < deadline, "decoded in time");
            got.extend(loader.poll().into_iter().map(|d| d.result.unwrap()));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!got[0].complete, "first part before the rest");
        assert_eq!(got.last().unwrap().frames.len(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn byte_cache_order_and_budget() {
        let a = PathBuf::from("a");
        let b = PathBuf::from("b");
        let c = PathBuf::from("c");
        let order = vec![a.clone(), b.clone(), c.clone()];
        let mut cache = ByteCache::new(10);
        assert_eq!(cache.next_to_read(&order), Some(&a));
        cache.insert(a.clone(), Arc::new(vec![0; 4]));
        assert_eq!(cache.next_to_read(&order), Some(&b));
        cache.insert(b.clone(), Arc::new(vec![0; 6]));
        // Budget reached: stop reading further ahead.
        assert_eq!(cache.next_to_read(&order), None);
        // Moved on: a is dropped, which makes room for c.
        let order = vec![b.clone(), c.clone()];
        cache.retain(&order);
        assert!(!cache.contains(&a));
        assert_eq!(cache.total(), 6);
        assert_eq!(cache.next_to_read(&order), Some(&c));
    }
}
