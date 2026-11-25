//! Formats beyond what the `image` crate reads: SVG drawings, Photoshop
//! files (their stored composite image) and camera RAW files (the full-size
//! JPEG preview every camera embeds).
//!
//! HEIC is deliberately absent — see docs/FORMATS.md. AVIF is in `avif`,
//! JPEG XL in `jxl`.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use eframe::egui::ColorImage;

use super::decode::{finish, DecodeOpts, Decoded, Orientation};

/// Camera RAW extensions whose embedded JPEG is shown.
pub const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "dng", "raf", "orf", "rw2", "pef",
    "srw", "rwl", "3fr", "iiq", "x3f", "erf", "mrw",
];

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Extra {
    Svg,
    Psd,
    Raw,
}

fn ext(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// Which extra format, if any, `bytes` (named `path`) are.
pub fn detect(bytes: &[u8], path: &Path) -> Option<Extra> {
    let e = ext(path);
    if bytes.starts_with(b"8BPS") {
        return Some(Extra::Psd);
    }
    if e == "svg" {
        return Some(Extra::Svg);
    }
    if RAW_EXTENSIONS.contains(&e.as_str()) {
        return Some(Extra::Raw);
    }
    let head = &bytes[..bytes.len().min(512)];
    let text = String::from_utf8_lossy(head);
    if text.trim_start().starts_with("<svg") || (text.contains("<?xml") && text.contains("<svg")) {
        return Some(Extra::Svg);
    }
    None
}

pub fn decode(
    kind: Extra,
    bytes: &[u8],
    path: &Path,
    opts: DecodeOpts,
    partial: &mut dyn FnMut(Decoded),
) -> Result<Decoded, String> {
    match kind {
        Extra::Svg => svg(bytes, opts),
        Extra::Psd => psd(bytes, opts),
        Extra::Raw => raw(bytes, path, opts, partial),
    }
}

/// Wraps finished straight-alpha RGBA pixels as a still image.
fn still(
    rgba: &[u8],
    size: [usize; 2],
    natural: [u32; 2],
    format: &'static str,
    opts: DecodeOpts,
) -> Decoded {
    let image = ColorImage::from_rgba_unmultiplied(size, rgba);
    let (image, tiles, reduced) = finish(image, opts, Orientation::default());
    let mut d = Decoded::from_color_image(image, format);
    d.width = natural[0];
    d.height = natural[1];
    d.tiled_size = tiles.iter().fold([0, 0], |[w, h], t| {
        [w.max(t.x + t.image.size[0]), h.max(t.y + t.image.size[1])]
    });
    d.tiles = tiles;
    d.reduced = reduced;
    d.color = "RGBA 8".into();
    d
}

fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static DB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

/// Renders an SVG. A drawing has no pixels of its own, so it is drawn at
/// the size it will be shown: the screen for screen quality, otherwise at
/// least 2048 px on the long side (sharp when zoomed), within the GPU limit.
fn svg(bytes: &[u8], opts: DecodeOpts) -> Result<Decoded, String> {
    let options = resvg::usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|e| e.to_string())?;
    let natural = tree.size();
    let (nw, nh) = (natural.width().max(1.0), natural.height().max(1.0));
    let long = nw.max(nh);
    let target = match opts.fit {
        Some([fw, fh]) => (fw as f32 / nw).min(fh as f32 / nh) * long,
        None => long.max(2048.0),
    }
    .min(opts.max_side as f32);
    let s = target / long;
    let (w, h) = (
        ((nw * s).round() as u32).max(1),
        ((nh * s).round() as u32).max(1),
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("SVG too large")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(s, s),
        &mut pixmap.as_mut(),
    );
    // tiny-skia pixels are premultiplied, like egui's.
    let image = ColorImage::from_rgba_premultiplied([w as usize, h as usize], pixmap.data());
    let rgba: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|p| p.to_srgba_unmultiplied())
        .collect();
    let opts = DecodeOpts { fit: None, ..opts };
    let mut d = still(&rgba, [w as usize, h as usize], [w, h], "SVG", opts);
    d.has_alpha = true;
    Ok(d)
}

/// The flattened image Photoshop stores alongside the layers.
/// The `psd` crate trusts every length in the file and slices without
/// checking, so a broken file makes it crash. It also parses every layer,
/// which a viewer showing the stored composite does not need. So: check the
/// layout here and hand it a copy with the image resources and layers left
/// out and the composite image unpacked — header, colour table and raw
/// planes only.
fn psd_composite_only(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let bad = || "damaged Photoshop file".to_string();
    let be16 = |at: usize| -> Option<usize> {
        Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?) as usize)
    };
    let be32 = |at: usize| -> Option<usize> {
        Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    if !bytes.starts_with(b"8BPS") || be16(4) != Some(1) {
        return Err("unsupported Photoshop file (only PSD, not PSB)".into());
    }
    let channels = be16(12).ok_or_else(bad)?;
    let (h, w) = (be32(14).ok_or_else(bad)?, be32(18).ok_or_else(bad)?);
    let depth = be16(22).ok_or_else(bad)?;
    if !(1..=56).contains(&channels) || w == 0 || h == 0 || w > 30_000 || h > 30_000 {
        return Err(bad());
    }
    if depth != 8 {
        return Err(format!("unsupported Photoshop bit depth ({depth})"));
    }
    // Colour mode data, image resources, layers and masks: length-prefixed.
    let colour = 26;
    let resources = colour + 4 + be32(colour).ok_or_else(bad)?;
    let layers = resources + 4 + be32(resources).ok_or_else(bad)?;
    let image = layers + 4 + be32(layers).ok_or_else(bad)?;
    let compression = be16(image).ok_or_else(bad)?;
    let data = image + 2;
    let plane = w * h;
    let mut out = Vec::with_capacity(resources + 10 + plane * channels);
    out.extend_from_slice(&bytes[..resources]);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&0u16.to_be_bytes()); // raw
    match compression {
        0 => {
            let raw = bytes.get(data..data + plane * channels).ok_or_else(bad)?;
            out.extend_from_slice(raw);
        }
        1 => {
            // PackBits: a byte count per row and channel, then the rows.
            // Unpacked here, with every row held to the image width.
            let rows = channels * h;
            let table = bytes.get(data..data + rows * 2).ok_or_else(bad)?;
            let mut at = data + rows * 2;
            for c in table.chunks_exact(2) {
                let len = u16::from_be_bytes([c[0], c[1]]) as usize;
                let packed = bytes.get(at..at + len).ok_or_else(bad)?;
                at += len;
                let start = out.len();
                unpack_bits(packed, &mut out, w);
                out.resize(start + w, 0);
            }
        }
        _ => return Err(format!("unsupported Photoshop compression ({compression})")),
    }
    Ok(out)
}

/// PackBits rows, up to `max` bytes.
fn unpack_bits(mut src: &[u8], out: &mut Vec<u8>, max: usize) {
    let end = out.len() + max;
    while let Some((&n, rest)) = src.split_first() {
        src = rest;
        let room = end - out.len();
        if n < 128 {
            let take = (n as usize + 1).min(src.len());
            out.extend_from_slice(&src[..take.min(room)]);
            src = &src[take..];
        } else if n > 128 {
            let Some((&b, rest)) = src.split_first() else {
                break;
            };
            src = rest;
            out.extend(std::iter::repeat_n(b, (257 - n as usize).min(room)));
        }
        if out.len() >= end {
            break;
        }
    }
}

fn psd(bytes: &[u8], opts: DecodeOpts) -> Result<Decoded, String> {
    let bytes = psd_composite_only(bytes)?;
    let psd = psd::Psd::from_bytes(&bytes).map_err(|e| e.to_string())?;
    let (w, h) = (psd.width(), psd.height());
    let rgba = psd.rgba();
    let mut d = still(&rgba, [w as usize, h as usize], [w, h], "PSD", opts);
    d.has_alpha = rgba.chunks_exact(4).any(|p| p[3] != 255);
    Ok(d)
}

/// The end of a JPEG that starts at `start` (just after its EOI marker),
/// found by walking its segments to the image data and then scanning for
/// the end marker.
fn jpeg_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 2;
    loop {
        if *bytes.get(i)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(i + 1)?;
        match marker {
            0xD8 | 0x01 | 0xD0..=0xD7 => i += 2,
            0xFF => i += 1,
            0xDA => {
                // Start of scan: entropy-coded data until FF D9 (FF 00 and
                // restart markers are part of the data).
                let len = u16::from_be_bytes([*bytes.get(i + 2)?, *bytes.get(i + 3)?]) as usize;
                let mut j = i + 2 + len;
                while j + 1 < bytes.len() {
                    if bytes[j] == 0xFF && bytes[j + 1] == 0xD9 {
                        return Some(j + 2);
                    }
                    j += 1;
                }
                return None;
            }
            _ => {
                let len = u16::from_be_bytes([*bytes.get(i + 2)?, *bytes.get(i + 3)?]) as usize;
                if len < 2 {
                    return None;
                }
                i += 2 + len;
            }
        }
    }
}

/// Every complete JPEG inside `bytes`, largest first.
pub fn embedded_jpegs(bytes: &[u8]) -> Vec<&[u8]> {
    let mut found: Vec<&[u8]> = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
            if let Some(end) = jpeg_end(bytes, i) {
                found.push(&bytes[i..end]);
                i = end;
                continue;
            }
        }
        i += 1;
    }
    found.sort_by_key(|j| std::cmp::Reverse(j.len()));
    found
}

/// How much of a RAW file is read to find its preview.
const RAW_HEAD: usize = 512 * 1024;
/// A preview further into the file than this is not worth the trick.
const RAW_MAX_SPARSE: u64 = 256 * 1024 * 1024;

/// Reads only what a RAW file's preview needs: the header (with the EXIF
/// data) and the largest JPEG its TIFF directories point to, placed at
/// their original offsets in an otherwise empty buffer — so offsets in the
/// EXIF data stay valid. A 50 MB RAW on a NAS costs a few MB instead of
/// 50. `None` when the layout is unknown (CR3, CRW, …): read it all.
pub fn read_raw_preview(path: &Path) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    if !RAW_EXTENSIONS.contains(&ext(path).as_str()) {
        return None;
    }
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    if len as usize <= RAW_HEAD * 2 {
        return None; // small anyway
    }
    let mut head = vec![0u8; RAW_HEAD];
    f.read_exact(&mut head).ok()?;
    let (offset, size) = if head.starts_with(b"FUJIFILMCCD-RAW") {
        // RAF: the JPEG's offset and length sit at fixed places.
        let be = |i: usize| u32::from_be_bytes(head[i..i + 4].try_into().unwrap()) as u64;
        (be(84), be(88))
    } else {
        largest_tiff_jpeg(&head)?
    };
    if size == 0 || offset + size > len || offset + size > RAW_MAX_SPARSE {
        return None;
    }
    let end = (offset + size) as usize;
    let mut buf = vec![0u8; end.max(RAW_HEAD)];
    buf[..RAW_HEAD].copy_from_slice(&head);
    if end > RAW_HEAD {
        let start = (offset as usize).max(RAW_HEAD);
        f.seek(SeekFrom::Start(start as u64)).ok()?;
        f.read_exact(&mut buf[start..end]).ok()?;
    }
    Some(buf)
}

/// The largest JPEG (offset, length) referenced by the TIFF directories in
/// `head`: thumbnail tags, or JPEG-compressed strips (CR2, NEF, ARW, DNG,
/// ORF, RW2, PEF all use one of the two).
fn largest_tiff_jpeg(head: &[u8]) -> Option<(u64, u64)> {
    let le = match head.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |i: usize| -> Option<u32> {
        let b: [u8; 2] = head.get(i..i + 2)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        } as u32)
    };
    let u32_at = |i: usize| -> Option<u32> {
        let b: [u8; 4] = head.get(i..i + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    // A value of `count` items of `ty` at entry `e`: inline or at an offset.
    let first_value = |e: usize| -> Option<u32> {
        let ty = u16_at(e + 2)?;
        match ty {
            3 => u16_at(e + 8),
            4 | 13 => {
                let count = u32_at(e + 4)?;
                if count == 1 {
                    u32_at(e + 8)
                } else {
                    u32_at(u32_at(e + 8)? as usize)
                }
            }
            _ => None,
        }
    };
    let mut best: Option<(u64, u64)> = None;
    let mut queue = vec![u32_at(4)? as usize];
    let mut seen = 0;
    while let Some(ifd) = queue.pop() {
        seen += 1;
        if seen > 32 || ifd == 0 || ifd + 2 > head.len() {
            continue;
        }
        let n = u16_at(ifd)? as usize;
        let (mut jif, mut jlen, mut strip, mut strip_len, mut compression) =
            (None, None, None, None, None);
        for k in 0..n.min(512) {
            let e = ifd + 2 + k * 12;
            match u16_at(e)? {
                0x0201 => jif = first_value(e),
                0x0202 => jlen = first_value(e),
                0x0111 => strip = first_value(e),
                0x0117 => strip_len = first_value(e),
                0x0103 => compression = first_value(e),
                // SubIFDs and the EXIF directory may hold more previews.
                0x014A => {
                    let count = u32_at(e + 4)? as usize;
                    if count == 1 {
                        queue.push(u32_at(e + 8)? as usize);
                    } else {
                        let at = u32_at(e + 8)? as usize;
                        for i in 0..count.min(8) {
                            if let Some(o) = u32_at(at + i * 4) {
                                queue.push(o as usize);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let mut consider = |o: Option<u32>, l: Option<u32>| {
            if let (Some(o), Some(l)) = (o, l) {
                if best.is_none_or(|(_, bl)| l as u64 > bl) {
                    best = Some((o as u64, l as u64));
                }
            }
        };
        consider(jif, jlen);
        if matches!(compression, Some(6) | Some(7)) {
            consider(strip, strip_len);
        }
        if let Some(next) = u32_at(ifd + 2 + n * 12) {
            queue.push(next as usize);
        }
    }
    best
}

/// A camera RAW file: its largest embedded JPEG, turned by the RAW's own
/// orientation tag (the preview usually carries none).
fn raw(
    bytes: &[u8],
    path: &Path,
    opts: DecodeOpts,
    partial: &mut dyn FnMut(Decoded),
) -> Result<Decoded, String> {
    let info = super::exif_info::read(bytes);
    let raw_o = Orientation::from_exif(info.as_ref().map_or(1, |e| e.orientation));
    for jpeg in embedded_jpegs(bytes).into_iter().take(3) {
        let Ok(mut d) =
            super::decode::decode_streaming(jpeg, Path::new("preview.jpg"), opts, partial)
        else {
            continue;
        };
        if d.orientation == Orientation::default() && raw_o != Orientation::default() {
            d.orientation = raw_o;
            if raw_o.swaps_axes() {
                std::mem::swap(&mut d.width, &mut d.height);
            }
        }
        d.format = match ext(path).as_str() {
            "dng" => "DNG",
            "cr2" | "cr3" | "crw" => "RAW · CANON",
            "nef" | "nrw" => "RAW · NIKON",
            "arw" | "srf" | "sr2" => "RAW · SONY",
            "raf" => "RAW · FUJIFILM",
            _ => "RAW",
        };
        if let Some(e) = info.filter(|e| !e.is_empty()) {
            d.exif = Some(e);
        }
        return Ok(d);
    }
    Err("no preview image in this RAW file".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
            .encode_image(&img)
            .unwrap();
        out
    }

    #[test]
    fn svg_is_rendered_sharp_at_its_shown_size() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
            <rect width="100" height="50" fill="#ff0000"/></svg>"##;
        assert_eq!(detect(svg, Path::new("logo.svg")), Some(Extra::Svg));
        assert_eq!(detect(svg, Path::new("noext")), Some(Extra::Svg));
        let d = decode(
            Extra::Svg,
            svg,
            Path::new("logo.svg"),
            DecodeOpts::full(8192),
            &mut |_| {},
        )
        .unwrap();
        // Drawn at 2048 wide (vectors stay sharp when zoomed).
        assert_eq!(d.texture_size(), [2048, 1024]);
        let p = d.frames[0].image.pixels[1024 * 512];
        assert_eq!((p.r(), p.g(), p.b()), (255, 0, 0));
        // Screen quality: drawn at the screen size directly.
        let screen = DecodeOpts {
            fit: Some([400, 400]),
            ..DecodeOpts::full(8192)
        };
        let d = decode(Extra::Svg, svg, Path::new("logo.svg"), screen, &mut |_| {}).unwrap();
        assert_eq!(d.texture_size(), [400, 200]);
    }

    #[test]
    fn psd_composite_is_shown() {
        // Minimal RGB PSD: 2x1, raw image data, no layers.
        let mut b = b"8BPS".to_vec();
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&[0; 6]);
        b.extend_from_slice(&3u16.to_be_bytes()); // channels
        b.extend_from_slice(&1u32.to_be_bytes()); // height
        b.extend_from_slice(&2u32.to_be_bytes()); // width
        b.extend_from_slice(&8u16.to_be_bytes()); // depth
        b.extend_from_slice(&3u16.to_be_bytes()); // RGB
        b.extend_from_slice(&0u32.to_be_bytes()); // colour mode data
        b.extend_from_slice(&0u32.to_be_bytes()); // image resources
        b.extend_from_slice(&0u32.to_be_bytes()); // layers and masks
        b.extend_from_slice(&0u16.to_be_bytes()); // raw data
        b.extend_from_slice(&[10, 200, 30, 40, 50, 60]); // R, G, B planes
        assert_eq!(detect(&b, Path::new("art.psd")), Some(Extra::Psd));
        let d = decode(
            Extra::Psd,
            &b,
            Path::new("art.psd"),
            DecodeOpts::full(8192),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!((d.width, d.height), (2, 1));
        let p = d.frames[0].image.pixels[0];
        assert_eq!((p.r(), p.g(), p.b()), (10, 30, 50));
    }

    #[test]
    fn packed_psd_is_unpacked_and_broken_ones_are_refused() {
        let b = crate::core::fuzz::psd(true);
        let d = decode(
            Extra::Psd,
            &b,
            Path::new("a.psd"),
            DecodeOpts::full(8192),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!((d.width, d.height), (3, 2));
        let px = |i: usize| {
            let p = d.frames[0].image.pixels[i];
            (p.r(), p.g(), p.b())
        };
        // Rows are stored channel by channel: R rows 0-1, G 2-3, B 4-5.
        assert_eq!(px(0), (0, 80, 160));
        assert_eq!(px(5), (60, 140, 220));
        // A row count past the end of the file: refused, not a crash.
        let mut cut = b.clone();
        cut.truncate(cut.len() - 3);
        assert!(psd_composite_only(&cut).is_err());
        // PSB (version 2) is not read.
        let mut psb = b;
        psb[5] = 2;
        assert!(psd_composite_only(&psb).unwrap_err().contains("PSB"));
    }

    #[test]
    fn raw_files_are_read_only_up_to_their_preview() {
        let preview = jpeg(300, 200, [255, 0, 0]);
        let at: u32 = 1_200_000; // past the header that is read
        let mut b = b"II*\0\x08\0\0\0".to_vec();
        b.extend_from_slice(&3u16.to_le_bytes());
        b.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
        b.extend_from_slice(&[0x01, 0x02, 4, 0, 1, 0, 0, 0]);
        b.extend_from_slice(&at.to_le_bytes());
        b.extend_from_slice(&[0x02, 0x02, 4, 0, 1, 0, 0, 0]);
        b.extend_from_slice(&(preview.len() as u32).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.resize(at as usize, 0x55);
        b.extend_from_slice(&preview);
        b.resize(b.len() + 3_000_000, 0x66); // sensor data after it
        let dir = crate::core::archive::tests::temp_dir("rawread");
        let p = dir.join("IMG_2.NEF");
        std::fs::write(&p, &b).unwrap();
        let read = read_raw_preview(&p).expect("partial read");
        assert_eq!(read.len(), at as usize + preview.len());
        assert!(read.len() < b.len() / 2);
        // The general reader uses it, and it decodes like the whole file.
        assert_eq!(crate::core::archive::read(&p).unwrap().len(), read.len());
        let d = decode(Extra::Raw, &read, &p, DecodeOpts::full(8192), &mut |_| {}).unwrap();
        assert_eq!((d.width, d.height), (200, 300));
        // Not a RAW, or unknown layout: read normally.
        assert!(read_raw_preview(&dir.join("x.jpg")).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn raw_shows_its_largest_preview_turned_upright() {
        // A TIFF-based RAW: orientation 6 in IFD0, then a small thumbnail
        // and the big preview somewhere in the file.
        let mut b = b"II*\0\x08\0\0\0".to_vec();
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&[0x55; 64]); // sensor data stand-in
        b.extend_from_slice(&jpeg(16, 12, [0, 0, 255]));
        b.extend_from_slice(&[0x77; 64]);
        b.extend_from_slice(&jpeg(300, 200, [255, 0, 0]));
        b.extend_from_slice(&[0x11; 16]);
        assert_eq!(embedded_jpegs(&b).len(), 2);
        assert_eq!(detect(&b, Path::new("IMG_1.CR2")), Some(Extra::Raw));
        let d = decode(
            Extra::Raw,
            &b,
            Path::new("IMG_1.CR2"),
            DecodeOpts::full(8192),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(d.format, "RAW · CANON");
        // 300x200 stored, shown upright as 200x300.
        assert_eq!(d.texture_size(), [300, 200]);
        assert_eq!((d.width, d.height), (200, 300));
        assert!(d.orientation.swaps_axes());
        let p = d.frames[0].image.pixels[0];
        assert!(p.r() > 200 && p.b() < 60, "{p:?}");
    }
}
