//! Animated GIF and WebP, handed over in parts while they decode.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::ColorImage;
use image::AnimationDecoder;

use super::*;
pub(super) type Animation = Option<(Vec<Frame>, u32, u32, bool)>;

/// Receives an animation's frames so far: (frames, width, height, reduced).
pub(super) type StreamFn<'s> = dyn FnMut(&[Frame], u32, u32, bool) + 's;

/// How often a decoding animation is handed over with its new frames.
pub const STREAM_EVERY: Duration = Duration::from_millis(250);

pub(super) fn decode_animation<'a>(
    dec: impl AnimationDecoder<'a>,
    opts: DecodeOpts,
    stream: &mut StreamFn<'_>,
) -> Result<Animation, String> {
    let mut frames = Vec::new();
    let mut total = 0usize;
    let mut reduced = false;
    let (mut w, mut h) = (0, 0);
    let mut sent: Option<std::time::Instant> = None;
    for frame in dec.into_frames() {
        let frame = match frame {
            Ok(f) => f,
            // A truncated file: play the frames we got.
            Err(_) if frames.len() > 1 => break,
            Err(e) => return Err(e.to_string()),
        };
        let (num, den) = frame.delay().numer_denom_ms();
        let ms = num.checked_div(den).unwrap_or(100);
        // Browsers treat very short delays as 100 ms; do the same.
        let delay = Duration::from_millis(if ms < 20 { 100 } else { ms as u64 });
        let buf = frame.into_buffer();
        w = buf.width();
        h = buf.height();
        let size = [w as usize, h as usize];
        let (image, r) = fit_for_display(
            ColorImage::from_rgba_unmultiplied(size, buf.as_raw()),
            opts,
            Orientation::default(),
        );
        reduced |= r;
        total += image.pixels.len() * 4;
        frames.push(Frame {
            image: Arc::new(image),
            delay,
        });
        if total > MAX_ANIMATION_BYTES {
            break;
        }
        if sent.is_none_or(|t| t.elapsed() >= STREAM_EVERY) {
            stream(&frames, w, h, reduced);
            sent = Some(std::time::Instant::now());
        }
    }
    Ok(match frames.len() {
        0 => return Err("no frames".into()),
        1 => None, // a single-frame GIF/WebP is a still image
        _ => Some((frames, w, h, reduced)),
    })
}
