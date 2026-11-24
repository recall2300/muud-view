//! Background decoding.
//!
//! The UI thread never decodes. It tells the [`Loader`] which files it wants,
//! most important first, and worker threads decode them into ready-to-upload
//! [`ColorImage`]s.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use eframe::egui;

pub use super::decode::Decoded;

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
    let mut decoded = super::decode::decode_bytes(&bytes, path, max_side)?;
    decoded.file_size = meta.as_ref().map(|m| m.len()).unwrap_or(bytes.len() as u64);
    decoded.modified = meta.and_then(|m| m.modified().ok());
    decoded.decode_time = started.elapsed();
    Ok(decoded)
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
