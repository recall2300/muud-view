//! Speed regression check, run in CI (`.github/workflows/ci.yml`, job
//! "Speed"):
//!
//! ```text
//! cargo test --profile bench-ci -- --ignored --nocapture bench_ci
//! ```
//!
//! Shared CI machines differ in speed from run to run, so every workload is
//! timed against a fixed calibration loop in the same process and the
//! *ratio* is compared with `bench/baseline.txt`. A ratio more than
//! [`TOLERANCE`] above its baseline fails the job. After an intended
//! change, update the file from the numbers the job prints
//! (`MUUD_BENCH_UPDATE=1` rewrites it locally).

use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui::{Color32, ColorImage};

use super::decode::{decode_bytes, downscale, synthetic_photo, DecodeOpts};

/// How much slower than the baseline a workload may get.
pub const TOLERANCE: f64 = 1.30;
const RUNS: usize = 5;
const BASELINE: &str = "bench/baseline.txt";

fn median(mut f: impl FnMut()) -> Duration {
    f(); // warm up caches and lazy statics
    let mut t: Vec<Duration> = (0..RUNS)
        .map(|_| {
            let s = Instant::now();
            f();
            s.elapsed()
        })
        .collect();
    t.sort();
    t[RUNS / 2]
}

/// A fixed mix of integer maths and memory traffic, roughly the kind of
/// work decoding is. Its time is the unit everything else is measured in.
fn calibrate() {
    let mut buf = vec![0u32; 8 << 20];
    let mut x = 0x9E37_79B9u32;
    for _ in 0..3 {
        for v in buf.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *v = v.wrapping_add(x.rotate_left(7));
        }
    }
    black_box(buf.iter().fold(0u32, |a, v| a.wrapping_add(*v)));
}

pub fn parse_baseline(text: &str) -> Vec<(String, f64)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_owned(), v.trim().parse().ok()?))
        })
        .collect()
}

/// `None` when there is no baseline yet; otherwise whether it is within
/// tolerance.
pub fn verdict(ratio: f64, baseline: Option<f64>) -> Option<bool> {
    baseline.map(|b| ratio <= b * TOLERANCE)
}

#[test]
#[ignore]
fn bench_ci() {
    let jpeg = synthetic_photo(6960, 4640);
    let path = Path::new("synthetic.jpg");
    let screen = DecodeOpts {
        fit: Some([2560, 1440]),
        ..DecodeOpts::full(16384)
    };
    let big = ColorImage::new(
        [6960, 4640],
        vec![Color32::from_rgb(90, 120, 200); 6960 * 4640],
    );

    let unit = median(calibrate);
    let results: Vec<(&str, Duration)> = vec![
        (
            "jpeg_full",
            median(|| {
                black_box(decode_bytes(&jpeg, path, DecodeOpts::full(16384)).unwrap());
            }),
        ),
        (
            "jpeg_screen",
            median(|| {
                black_box(decode_bytes(&jpeg, path, screen).unwrap());
            }),
        ),
        (
            "downscale",
            median(|| {
                black_box(downscale(&big, [2160, 1440]));
            }),
        ),
    ];

    let base = std::fs::read_to_string(BASELINE).unwrap_or_default();
    let base = parse_baseline(&base);
    let mut summary = String::from(
        "### Speed (32.5 MP synthetic JPEG)\n\n| workload | ms | ratio | baseline | |\n|---|---|---|---|---|\n",
    );
    let mut failed = Vec::new();
    let mut new_baseline =
        String::from("# Workload time / calibration time on CI (see src/core/bench.rs).\n");
    println!("calibration: {:.1} ms", unit.as_secs_f64() * 1000.0);
    for (name, t) in &results {
        let ratio = t.as_secs_f64() / unit.as_secs_f64();
        let b = base.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        let mark = match verdict(ratio, b) {
            None => "new",
            Some(true) => "ok",
            Some(false) => {
                failed.push(*name);
                "SLOWER"
            }
        };
        let ms = t.as_secs_f64() * 1000.0;
        let b_text = b.map(|b| format!("{b:.3}")).unwrap_or_else(|| "-".into());
        println!("{name}: {ms:.1} ms, ratio {ratio:.3} (baseline {b_text}) {mark}");
        // GitHub turns this into an annotation on the run.
        println!(
            "::notice title=speed {name}::{ms:.1} ms, ratio {ratio:.3}, baseline {b_text}, {mark}"
        );
        summary.push_str(&format!(
            "| {name} | {ms:.1} | {ratio:.3} | {b_text} | {mark} |\n"
        ));
        new_baseline.push_str(&format!("{name} = {ratio:.3}\n"));
    }
    if let Some(p) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let _ = std::fs::write(p, &summary);
    }
    if std::env::var_os("MUUD_BENCH_UPDATE").is_some() {
        std::fs::write(BASELINE, new_baseline).unwrap();
    }
    assert!(
        failed.is_empty(),
        "more than {:.0}% slower than bench/baseline.txt: {failed:?}",
        (TOLERANCE - 1.0) * 100.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_file_format() {
        let b = parse_baseline("# comment\njpeg_full = 2.5\n\nbad line\ndownscale=0.4\n");
        assert_eq!(
            b,
            vec![("jpeg_full".to_owned(), 2.5), ("downscale".to_owned(), 0.4)]
        );
        assert_eq!(verdict(3.0, None), None);
        assert_eq!(verdict(3.2, Some(2.5)), Some(true));
        assert_eq!(verdict(3.3, Some(2.5)), Some(false));
    }

    #[test]
    fn checked_in_baseline_parses() {
        let text = std::fs::read_to_string(BASELINE).unwrap();
        let b = parse_baseline(&text);
        assert!(b.iter().all(|(_, v)| *v > 0.0));
    }
}
