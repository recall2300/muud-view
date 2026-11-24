//! "Save for sharing": a smaller copy with no metadata at all — no GPS
//! position, camera serial, dates or thumbnails — next to the original.
//!
//! The pixels are decoded, turned upright (so the copy does not need an
//! orientation tag), converted to sRGB, shrunk and encoded again. The
//! encoders here write pixels only, so nothing from the original file can
//! leak into the copy.

use std::path::{Path, PathBuf};

use eframe::egui::ColorImage;

use super::decode::{downscale, upright, DecodeOpts};

/// Long-side choices offered in the settings; 0 keeps the size.
pub const SIZES: [u32; 4] = [1280, 2048, 3840, 0];

/// Name of the copy: `photo.jpg` → `photo_share.jpg` (or `.png`).
pub fn share_name(src: &Path, png: bool) -> String {
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".into());
    format!("{stem}_share.{}", if png { "png" } else { "jpg" })
}

/// Fits `size` so the long side is at most `max` (0 = no limit).
pub fn fit(size: [usize; 2], max: u32) -> [usize; 2] {
    let [w, h] = size;
    let long = w.max(h);
    if max == 0 || long <= max as usize {
        return size;
    }
    let s = max as f64 / long as f64;
    [
        ((w as f64 * s).round() as usize).max(1),
        ((h as f64 * s).round() as usize).max(1),
    ]
}

/// Encodes without any metadata: JPEG (quality 88) for opaque images, PNG
/// when there is transparency.
pub fn encode(img: &ColorImage, png: bool) -> Result<Vec<u8>, String> {
    let [w, h] = img.size;
    let mut out = Vec::new();
    if png {
        let rgba: Vec<u8> = img
            .pixels
            .iter()
            .flat_map(|p| p.to_srgba_unmultiplied())
            .collect();
        image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .ok_or("buffer")?
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
    } else {
        let rgb: Vec<u8> = img
            .pixels
            .iter()
            .flat_map(|p| [p.r(), p.g(), p.b()])
            .collect();
        let buf = image::RgbImage::from_raw(w as u32, h as u32, rgb).ok_or("buffer")?;
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88)
            .encode_image(&buf)
            .map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Writes the sharing copy of `src` next to it and returns its path.
pub fn export(src: &Path, max: u32) -> Result<PathBuf, String> {
    let d = super::loader::decode_file(src, DecodeOpts::full(1 << 15))?;
    let frame = &d.frames.first().ok_or("no image")?.image;
    let img = upright(frame, d.orientation);
    let img = match fit(img.size, max) {
        t if t != img.size => downscale(&img, t),
        _ => img,
    };
    let png = d.has_alpha;
    let bytes = encode(&img, png)?;
    let dir = src.parent().ok_or("no folder")?;
    let target = super::photo_folders::unique_target(dir, &share_name(src, png));
    std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JPEG with an EXIF block holding orientation 6 and a fake GPS tag.
    fn jpeg_with_exif() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(400, 200, image::Rgb([200, 30, 30]));
        let mut plain = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 90)
            .encode_image(&img)
            .unwrap();
        // TIFF (little endian), IFD0 with Orientation=6 and GPS IFD pointer.
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
        tiff.extend_from_slice(&[0x25, 0x88, 4, 0, 1, 0, 0, 0, 38, 0, 0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        // GPS IFD with GPSLatitudeRef = "N".
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&[0x01, 0x00, 2, 0, 2, 0, 0, 0, b'N', 0, 0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut out = plain[..2].to_vec(); // SOI
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&plain[2..]);
        out
    }

    #[test]
    fn names_and_sizes() {
        assert_eq!(
            share_name(Path::new("a/IMG_1.JPG"), false),
            "IMG_1_share.jpg"
        );
        assert_eq!(share_name(Path::new("logo.png"), true), "logo_share.png");
        assert_eq!(fit([6960, 4640], 2048), [2048, 1365]);
        assert_eq!(fit([800, 600], 2048), [800, 600]);
        assert_eq!(fit([6960, 4640], 0), [6960, 4640]);
    }

    #[test]
    fn copy_is_upright_smaller_and_has_no_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "muud-share-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("photo.jpg");
        let original = jpeg_with_exif();
        assert!(original.windows(4).any(|w| w == b"Exif"));
        std::fs::write(&src, &original).unwrap();
        // The test file really carries the rotation we expect to bake in.
        let d = crate::core::loader::decode_file(&src, DecodeOpts::full(8192)).unwrap();
        assert!(d.orientation.swaps_axes());

        let out = export(&src, 100).unwrap();
        assert_eq!(out, dir.join("photo_share.jpg"));
        let bytes = std::fs::read(&out).unwrap();
        assert!(!bytes.windows(4).any(|w| w == b"Exif"), "no EXIF block");
        let img = image::load_from_memory(&bytes).unwrap();
        // Stored 400x200 with orientation 6 → upright 200x400 → fit 100.
        assert_eq!((img.width(), img.height()), (50, 100));
        // A second export does not overwrite the first.
        let again = export(&src, 100).unwrap();
        assert_ne!(again, out);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
