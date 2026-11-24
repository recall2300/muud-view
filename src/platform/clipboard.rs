//! Clipboard: images and file lists.
//!
//! On Windows, "copy image" puts the picture *and* the file on the clipboard
//! in one go, so pasting into Explorer creates the file and pasting into a
//! chat or document inserts the picture. That needs raw Win32 calls, because
//! clipboard libraries clear the clipboard for every format they set.

use std::path::{Path, PathBuf};

use eframe::egui::{Color32, ColorImage};

/// A bottom-up 32-bit `BITMAPINFOHEADER` + BGRA pixels, as `CF_DIB` expects.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn dib_from_image(img: &ColorImage) -> Vec<u8> {
    let [w, h] = img.size;
    let mut out = Vec::with_capacity(40 + w * h * 4);
    let push_u32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    let push_u16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    push_u32(&mut out, 40); // biSize
    push_u32(&mut out, w as u32); // biWidth
    push_u32(&mut out, h as u32); // biHeight (positive = bottom-up)
    push_u16(&mut out, 1); // biPlanes
    push_u16(&mut out, 32); // biBitCount
    push_u32(&mut out, 0); // biCompression = BI_RGB
    push_u32(&mut out, (w * h * 4) as u32); // biSizeImage
    push_u32(&mut out, 2835); // 72 dpi
    push_u32(&mut out, 2835);
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    for row in img.pixels.chunks_exact(w).rev() {
        for p in row {
            let [r, g, b, a] = p.to_srgba_unmultiplied();
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    out
}

/// A `DROPFILES` block (`CF_HDROP`) listing `paths` as UTF-16.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn dropfiles(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&20u32.to_le_bytes()); // pFiles: offset of the list
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    out.extend_from_slice(&0u32.to_le_bytes()); // fNC
    out.extend_from_slice(&1u32.to_le_bytes()); // fWide
    for p in paths {
        for unit in wide(p) {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(windows)]
fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().collect()
}

#[cfg(not(windows))]
#[allow(dead_code)] // only reached through `dropfiles`, which is Windows-only outside tests
fn wide(p: &Path) -> Vec<u16> {
    p.to_string_lossy().encode_utf16().collect()
}

/// Puts an image and/or files on the clipboard, replacing what was there.
#[cfg(windows)]
pub fn write(image: Option<&ColorImage>, files: &[PathBuf]) -> Result<(), String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    const CF_DIB: u32 = 8;
    const CF_HDROP: u32 = 15;

    let mut blocks: Vec<(u32, Vec<u8>)> = Vec::new();
    if let Some(img) = image {
        blocks.push((CF_DIB, dib_from_image(img)));
    }
    if !files.is_empty() {
        blocks.push((CF_HDROP, dropfiles(files)));
    }

    // Another program may hold the clipboard for a moment.
    // SAFETY: plain Win32 clipboard protocol; every opened clipboard is closed.
    unsafe {
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !opened {
            return Err("clipboard is busy".into());
        }
        EmptyClipboard();
        let mut result = Ok(());
        for (format, bytes) in blocks {
            let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if mem.is_null() {
                result = Err("out of memory".to_string());
                break;
            }
            let dst = GlobalLock(mem) as *mut u8;
            if dst.is_null() {
                GlobalFree(mem);
                result = Err("cannot lock memory".to_string());
                break;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
            GlobalUnlock(mem);
            // On success the system owns the memory.
            if SetClipboardData(format, mem).is_null() {
                GlobalFree(mem);
                result = Err("cannot set clipboard data".to_string());
                break;
            }
        }
        CloseClipboard();
        result
    }
}

#[cfg(not(windows))]
pub fn write(image: Option<&ColorImage>, files: &[PathBuf]) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    if let Some(img) = image {
        let bytes: Vec<u8> = img
            .pixels
            .iter()
            .flat_map(|p| p.to_srgba_unmultiplied())
            .collect();
        cb.set_image(arboard::ImageData {
            width: img.size[0],
            height: img.size[1],
            bytes: bytes.into(),
        })
        .map_err(|e| e.to_string())
    } else {
        cb.set().file_list(files).map_err(|e| e.to_string())
    }
}

/// What a paste would bring in.
pub enum Pasted {
    Files(Vec<PathBuf>),
    Image(ColorImage),
    Nothing,
}

pub fn read() -> Pasted {
    let Ok(mut cb) = arboard::Clipboard::new() else {
        return Pasted::Nothing;
    };
    if let Ok(files) = cb.get().file_list() {
        if !files.is_empty() {
            return Pasted::Files(files);
        }
    }
    if let Ok(img) = cb.get_image() {
        let mut pixels = vec![Color32::BLACK; img.width * img.height];
        for (dst, src) in pixels.iter_mut().zip(img.bytes.chunks_exact(4)) {
            *dst = Color32::from_rgba_unmultiplied(src[0], src[1], src[2], src[3]);
        }
        return Pasted::Image(ColorImage::new([img.width, img.height], pixels));
    }
    Pasted::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_layout() {
        // 2x2: top row red, green; bottom row blue, white.
        let img = ColorImage::new(
            [2, 2],
            vec![Color32::RED, Color32::GREEN, Color32::BLUE, Color32::WHITE],
        );
        let d = dib_from_image(&img);
        assert_eq!(d.len(), 40 + 16);
        assert_eq!(&d[0..4], &40u32.to_le_bytes());
        assert_eq!(&d[14..16], &32u16.to_le_bytes());
        // Bottom-up: first stored pixel is the bottom-left (blue), as BGRA.
        assert_eq!(&d[40..44], &[255, 0, 0, 255]);
        // Then the top-left (red) starts the second stored row.
        assert_eq!(&d[48..52], &[0, 0, 255, 255]);
    }

    #[test]
    fn dropfiles_layout() {
        let d = dropfiles(&[PathBuf::from("a.jpg"), PathBuf::from("사진.png")]);
        assert_eq!(&d[0..4], &20u32.to_le_bytes());
        assert_eq!(&d[16..20], &1u32.to_le_bytes()); // wide strings
        let units: Vec<u16> = d[20..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16(&units).unwrap();
        assert_eq!(text, "a.jpg\0사진.png\0\0");
    }
}
