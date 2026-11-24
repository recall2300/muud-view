//! Saving a rotation into a JPEG without losing quality (`Ctrl+Shift+S`).
//!
//! The pixels are not touched: only the EXIF Orientation value changes, so
//! not a single bit of the image data is re-encoded. Windows, phones,
//! browsers and photo apps all honour that value. The new value is the one
//! that reproduces exactly what is on screen (EXIF orientation combined
//! with the rotation and flips applied in the viewer).

use std::path::Path;

use super::decode::Orientation;
use super::view::compose_uvs;

/// The EXIF Orientation (1-8) that shows the image the way `o` plus the
/// viewer's `turns` and flips do.
pub fn exif_code_for(o: Orientation, turns: u8, flip_h: bool, flip_v: bool) -> u16 {
    let want = compose_uvs(o, turns, flip_h, flip_v);
    (1..=8u16)
        .find(|&c| compose_uvs(Orientation::from_exif(c as u32), 0, false, false) == want)
        .unwrap_or(1)
}

fn u16_at(b: &[u8], i: usize, le: bool) -> Option<u16> {
    let x = [*b.get(i)?, *b.get(i + 1)?];
    Some(if le {
        u16::from_le_bytes(x)
    } else {
        u16::from_be_bytes(x)
    })
}

fn u32_at(b: &[u8], i: usize, le: bool) -> Option<u32> {
    let x = [*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?];
    Some(if le {
        u32::from_le_bytes(x)
    } else {
        u32::from_be_bytes(x)
    })
}

/// A minimal EXIF block holding only the orientation.
fn exif_segment(code: u16) -> Vec<u8> {
    let mut tiff = b"II*\0\x08\0\0\0".to_vec();
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0]);
    tiff.extend_from_slice(&code.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    let mut seg = vec![0xFF, 0xE1];
    seg.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
    seg.extend_from_slice(b"Exif\0\0");
    seg.extend_from_slice(&tiff);
    seg
}

/// The JPEG with its orientation set to `code`. An existing tag is changed
/// in place (same length, nothing else moves); a file without EXIF gets a
/// small EXIF block. A file whose EXIF lacks the tag is refused rather than
/// rewritten.
pub fn set_orientation(jpeg: &[u8], code: u16) -> Result<Vec<u8>, String> {
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return Err("not a JPEG".into());
    }
    let mut i = 2;
    let mut after_app0 = 2;
    while i + 4 <= jpeg.len() && jpeg[i] == 0xFF {
        let marker = jpeg[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            break; // image data: no EXIF before it
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        let body = i + 4;
        if marker == 0xE0 {
            after_app0 = i + 2 + len;
        }
        if marker == 0xE1 && jpeg.get(body..body + 6) == Some(b"Exif\0\0") {
            let t = body + 6; // TIFF header
            let le = match jpeg.get(t..t + 2) {
                Some(b"II") => true,
                Some(b"MM") => false,
                _ => return Err("damaged EXIF".into()),
            };
            let ifd = t + u32_at(jpeg, t + 4, le).ok_or("damaged EXIF")? as usize;
            let n = u16_at(jpeg, ifd, le).ok_or("damaged EXIF")? as usize;
            for k in 0..n {
                let e = ifd + 2 + k * 12;
                if u16_at(jpeg, e, le) == Some(0x0112) {
                    let mut out = jpeg.to_vec();
                    let v = if le {
                        code.to_le_bytes()
                    } else {
                        code.to_be_bytes()
                    };
                    out[e + 8..e + 10].copy_from_slice(&v);
                    return Ok(out);
                }
            }
            return Err("this JPEG's EXIF has no orientation tag".into());
        }
        i += 2 + len;
    }
    // No EXIF at all: add one (after the JFIF header, if any).
    let mut out = jpeg[..after_app0].to_vec();
    out.extend_from_slice(&exif_segment(code));
    out.extend_from_slice(&jpeg[after_app0..]);
    Ok(out)
}

/// Writes the rotation into the file (through a temporary file, so a
/// failure never leaves a half-written photo). Returns the new value.
pub fn save(
    path: &Path,
    o: Orientation,
    turns: u8,
    flip_h: bool,
    flip_v: bool,
) -> Result<u16, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let code = exif_code_for(o, turns, flip_h, flip_v);
    let out = set_orientation(&bytes, code)?;
    let tmp = path.with_extension("muud-rotate.tmp");
    std::fs::write(&tmp, out).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_fn(8, 4, |x, _| image::Rgb([x as u8 * 30, 0, 0]));
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
            .encode_image(&img)
            .unwrap();
        out
    }

    fn orientation_of(b: &[u8]) -> u32 {
        crate::core::exif_info::read(b).map_or(1, |e| e.orientation)
    }

    /// The entropy-coded image data (everything from SOS on).
    fn scan(b: &[u8]) -> &[u8] {
        let at = b.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
        &b[at..]
    }

    #[test]
    fn codes_match_what_is_shown() {
        let none = Orientation::default();
        assert_eq!(exif_code_for(none, 0, false, false), 1);
        assert_eq!(exif_code_for(none, 1, false, false), 6);
        assert_eq!(exif_code_for(none, 2, false, false), 3);
        assert_eq!(exif_code_for(none, 3, false, false), 8);
        assert_eq!(exif_code_for(none, 0, true, false), 2);
        assert_eq!(exif_code_for(none, 0, false, true), 4);
        // Already rotated by the camera, then turned once more.
        let cw = Orientation::from_exif(6);
        assert_eq!(exif_code_for(cw, 1, false, false), 3);
        assert_eq!(exif_code_for(cw, 3, false, false), 1);
        // Every code round-trips.
        for c in 1..=8u32 {
            assert_eq!(
                exif_code_for(Orientation::from_exif(c), 0, false, false),
                c as u16
            );
        }
    }

    #[test]
    fn adds_exif_to_a_plain_jpeg_without_touching_pixels() {
        let plain = plain_jpeg();
        let out = set_orientation(&plain, 6).unwrap();
        assert_eq!(orientation_of(&out), 6);
        assert_eq!(scan(&out), scan(&plain));
        image::load_from_memory(&out).unwrap();
        // Changing it again edits the tag in place: same length.
        let again = set_orientation(&out, 3).unwrap();
        assert_eq!(again.len(), out.len());
        assert_eq!(orientation_of(&again), 3);
        assert_eq!(scan(&again), scan(&plain));
    }

    #[test]
    fn saves_through_a_temporary_file() {
        let dir = crate::core::archive::tests::temp_dir("rotate");
        let p = dir.join("a.jpg");
        std::fs::write(&p, plain_jpeg()).unwrap();
        let code = save(&p, Orientation::default(), 1, false, false).unwrap();
        assert_eq!(code, 6);
        let d = crate::core::loader::decode_file(&p, crate::core::decode::DecodeOpts::full(8192))
            .unwrap();
        assert_eq!((d.width, d.height), (4, 8));
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "no temp file left"
        );
        assert!(set_orientation(b"not a jpeg", 6).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
