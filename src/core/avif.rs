//! AVIF: AV1 pictures in a HEIF (ISO BMFF) container. The AV1 decoder is
//! rav1d (BSD-2-Clause, the Rust port of dav1d); the container is read
//! here, because the usual Rust parser (`mp4parse`, used by the `image`
//! crate) is MPL-2.0. Handles the still image an AVIF shows: grids of
//! tiles, alpha, rotation and mirroring (`irot`, `imir`), colour (`colr`:
//! CICP or ICC) and the Exif item. Animated AVIF shows its still image.

use std::ptr::NonNull;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{Color32, ColorImage};

use super::decode::{
    finish_keep, histogram_of, tiled_size, with_full, DecodeOpts, Decoded, Frame, Orientation,
};

/// `ftyp` naming an AVIF brand.
pub fn is_avif(bytes: &[u8]) -> bool {
    let Some((b"ftyp", body)) = boxes(bytes).next() else {
        return false;
    };
    // Major brand, minor version, then compatible brands.
    body.chunks_exact(4)
        .enumerate()
        .any(|(i, b)| i != 1 && (b == b"avif" || b == b"avis"))
}

// ---- ISO BMFF boxes -------------------------------------------------------

/// The boxes in `data`: (type, body). Stops at the first malformed one.
fn boxes(mut data: &[u8]) -> impl Iterator<Item = (&[u8; 4], &[u8])> {
    std::iter::from_fn(move || {
        let size = u32::from_be_bytes(data.get(0..4)?.try_into().ok()?) as u64;
        let kind: &[u8; 4] = data.get(4..8)?.try_into().ok()?;
        let (header, size) = match size {
            0 => (8, data.len() as u64),
            1 => (16, u64::from_be_bytes(data.get(8..16)?.try_into().ok()?)),
            n => (8, n),
        };
        if size < header as u64 || size > data.len() as u64 {
            data = &[];
            return None;
        }
        let body = &data[header..size as usize];
        data = &data[size as usize..];
        Some((kind, body))
    })
}

fn find<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _)| *k == kind).map(|(_, b)| b)
}

/// Big-endian reads that fail instead of panicking.
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let (a, b) = (self.0.get(..n)?, self.0.get(n..)?);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.bytes(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.bytes(4)?.try_into().ok()?))
    }
    /// An unsigned number of `n` bytes (0, 4 or 8, as `iloc` uses).
    fn sized(&mut self, n: u8) -> Option<u64> {
        match n {
            0 => Some(0),
            4 => self.u32().map(u64::from),
            8 => Some(u64::from_be_bytes(self.bytes(8)?.try_into().ok()?)),
            _ => None,
        }
    }
    /// Full box header: (version, flags).
    fn full(&mut self) -> Option<(u8, u32)> {
        let v = self.u32()?;
        Some(((v >> 24) as u8, v & 0xFF_FFFF))
    }
    fn id(&mut self, wide: bool) -> Option<u32> {
        if wide {
            self.u32()
        } else {
            self.u16().map(u32::from)
        }
    }
}

// ---- The item structure (`meta`) ------------------------------------------

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct Nclx {
    primaries: u16,
    transfer: u16,
    matrix: u16,
    full_range: bool,
}

#[derive(Default, Debug, Clone)]
struct Props {
    av1c: Option<Vec<u8>>,
    size: Option<[u32; 2]>,
    nclx: Option<Nclx>,
    icc: Option<Vec<u8>>,
    /// Anticlockwise quarter turns.
    rotation: u8,
    /// Mirror axis after rotation: 0 = left-right, 1 = top-bottom.
    mirror: Option<u8>,
    alpha_urn: bool,
    /// Clean aperture (`clap`): the shown size when smaller than `ispe`.
    clean: Option<[u32; 2]>,
}

#[derive(Debug, Clone)]
struct Item {
    id: u32,
    kind: [u8; 4],
    /// (offset, length) pieces in the file (or in `idat`).
    extents: Vec<(u64, u64)>,
    in_idat: bool,
    props: Props,
}

struct Meta<'a> {
    primary: u32,
    items: Vec<Item>,
    /// (reference type, from, to…)
    refs: Vec<([u8; 4], u32, Vec<u32>)>,
    idat: &'a [u8],
}

fn bad() -> String {
    "damaged AVIF file".into()
}

fn parse(file: &[u8]) -> Result<Meta<'_>, String> {
    let meta = find(file, b"meta").ok_or("no image in this AVIF file (no meta box)")?;
    let children = meta.get(4..).ok_or_else(bad)?;
    let pitm = find(children, b"pitm").ok_or_else(bad)?;
    let mut r = Reader(pitm);
    let (v, _) = r.full().ok_or_else(bad)?;
    let primary = r.id(v > 0).ok_or_else(bad)?;

    // Item types.
    let mut items = Vec::new();
    let iinf = find(children, b"iinf").ok_or_else(bad)?;
    let mut r = Reader(iinf);
    let (v, _) = r.full().ok_or_else(bad)?;
    r.id(v > 0).ok_or_else(bad)?; // entry count
    for (kind, body) in boxes(r.0) {
        if kind != b"infe" {
            continue;
        }
        let mut r = Reader(body);
        let (v, _) = r.full().ok_or_else(bad)?;
        if v < 2 {
            continue;
        }
        let id = r.id(v > 2).ok_or_else(bad)?;
        r.u16().ok_or_else(bad)?; // protection index
        let kind: [u8; 4] = r.bytes(4).ok_or_else(bad)?.try_into().map_err(|_| bad())?;
        items.push(Item {
            id,
            kind,
            extents: Vec::new(),
            in_idat: false,
            props: Props::default(),
        });
    }

    // Where each item's data is.
    let iloc = find(children, b"iloc").ok_or_else(bad)?;
    let mut r = Reader(iloc);
    let (v, _) = r.full().ok_or_else(bad)?;
    let a = r.u8().ok_or_else(bad)?;
    let b = r.u8().ok_or_else(bad)?;
    let (offset_size, length_size, base_size) = (a >> 4, a & 15, b >> 4);
    let index_size = if v >= 1 { b & 15 } else { 0 };
    let count = r.id(v >= 2).ok_or_else(bad)?;
    for _ in 0..count {
        let id = r.id(v >= 2).ok_or_else(bad)?;
        let method = if v >= 1 {
            r.u16().ok_or_else(bad)? & 15
        } else {
            0
        };
        r.u16().ok_or_else(bad)?; // data reference index
        let base = r.sized(base_size).ok_or_else(bad)?;
        let n = r.u16().ok_or_else(bad)?;
        let mut extents = Vec::new();
        for _ in 0..n {
            r.sized(index_size).ok_or_else(bad)?;
            let off = r.sized(offset_size).ok_or_else(bad)?;
            let len = r.sized(length_size).ok_or_else(bad)?;
            extents.push((base.checked_add(off).ok_or_else(bad)?, len));
        }
        if let Some(item) = items.iter_mut().find(|i| i.id == id) {
            item.extents = extents;
            item.in_idat = method == 1;
        }
    }

    // Properties, and which item has which.
    if let Some(iprp) = find(children, b"iprp") {
        let ipco: Vec<_> = find(iprp, b"ipco")
            .map(|b| boxes(b).collect())
            .unwrap_or_default();
        for (kind, body) in boxes(iprp) {
            if kind != b"ipma" {
                continue;
            }
            let mut r = Reader(body);
            let (v, flags) = r.full().ok_or_else(bad)?;
            let n = r.u32().ok_or_else(bad)?;
            for _ in 0..n {
                let id = r.id(v >= 1).ok_or_else(bad)?;
                let k = r.u8().ok_or_else(bad)?;
                for _ in 0..k {
                    let index = if flags & 1 != 0 {
                        (r.u16().ok_or_else(bad)? & 0x7FFF) as usize
                    } else {
                        (r.u8().ok_or_else(bad)? & 0x7F) as usize
                    };
                    let (Some(item), Some(&(kind, body))) = (
                        items.iter_mut().find(|i| i.id == id),
                        index.checked_sub(1).and_then(|i| ipco.get(i)),
                    ) else {
                        continue;
                    };
                    apply_property(&mut item.props, kind, body);
                }
            }
        }
    }

    let mut refs = Vec::new();
    if let Some(iref) = find(children, b"iref") {
        let mut r = Reader(iref);
        let (v, _) = r.full().ok_or_else(bad)?;
        for (kind, body) in boxes(r.0) {
            let mut r = Reader(body);
            let Some(from) = r.id(v > 0) else { continue };
            let Some(n) = r.u16() else { continue };
            let to: Option<Vec<u32>> = (0..n).map(|_| r.id(v > 0)).collect();
            if let Some(to) = to {
                refs.push((*kind, from, to));
            }
        }
    }
    let idat = find(children, b"idat").unwrap_or(&[]);
    Ok(Meta {
        primary,
        items,
        refs,
        idat,
    })
}

fn apply_property(p: &mut Props, kind: &[u8; 4], body: &[u8]) {
    let mut r = Reader(body);
    match kind {
        b"av1C" => p.av1c = body.get(4..).map(<[u8]>::to_vec),
        b"ispe" => {
            r.full();
            if let (Some(w), Some(h)) = (r.u32(), r.u32()) {
                p.size = Some([w, h]);
            }
        }
        b"colr" => match r.bytes(4) {
            Some(b"nclx") => {
                if let (Some(primaries), Some(transfer), Some(matrix), Some(f)) =
                    (r.u16(), r.u16(), r.u16(), r.u8())
                {
                    p.nclx = Some(Nclx {
                        primaries,
                        transfer,
                        matrix,
                        full_range: f & 0x80 != 0,
                    });
                }
            }
            Some(b"prof") | Some(b"rICC") => p.icc = Some(r.0.to_vec()),
            _ => {}
        },
        b"irot" => p.rotation = r.u8().unwrap_or(0) & 3,
        b"clap" => {
            let mut frac = || -> Option<u32> {
                let (n, d) = (r.u32()?, r.u32()?);
                (d != 0).then(|| ((n as f64 / d as f64).round()) as u32)
            };
            if let (Some(w), Some(h)) = (frac(), frac()) {
                p.clean = Some([w, h]);
            }
        }
        b"imir" => p.mirror = r.u8().map(|a| a & 1),
        b"auxC" => {
            r.full();
            let urn = r.0.split(|&b| b == 0).next().unwrap_or(&[]);
            p.alpha_urn = urn == b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha"
                || urn == b"urn:mpeg:hevc:2015:auxid:1";
        }
        _ => {}
    }
}

impl<'a> Meta<'a> {
    fn item(&self, id: u32) -> Result<&Item, String> {
        self.items.iter().find(|i| i.id == id).ok_or_else(bad)
    }

    fn data(&self, file: &'a [u8], item: &Item) -> Result<Vec<u8>, String> {
        let src = if item.in_idat { self.idat } else { file };
        let mut out = Vec::new();
        for &(off, len) in &item.extents {
            let start = usize::try_from(off).map_err(|_| bad())?;
            // Length 0: to the end of the source.
            let end = if len == 0 {
                src.len()
            } else {
                start
                    .checked_add(usize::try_from(len).map_err(|_| bad())?)
                    .ok_or_else(bad)?
            };
            out.extend_from_slice(src.get(start..end).ok_or_else(bad)?);
        }
        Ok(out)
    }

    /// Items that `kind`-reference `to` (e.g. the alpha plane of an image).
    fn referring(&self, kind: &[u8; 4], to: u32) -> impl Iterator<Item = u32> + '_ {
        let kind = *kind;
        self.refs
            .iter()
            .filter(move |(k, _, t)| *k == kind && t.contains(&to))
            .map(|(_, from, _)| *from)
    }

    fn referenced(&self, kind: &[u8; 4], from: u32) -> Vec<u32> {
        self.refs
            .iter()
            .find(|(k, f, _)| k == kind && *f == from)
            .map(|(_, _, t)| t.clone())
            .unwrap_or_default()
    }
}

// ---- AV1 through rav1d ----------------------------------------------------

/// A decoded AV1 picture, planes copied out (samples widened to u16).
struct Planes {
    w: usize,
    h: usize,
    bits: u8,
    /// Chroma subsampling, (x, y) shift; `None` for monochrome.
    sub: Option<(u8, u8)>,
    y: Vec<u16>,
    u: Vec<u16>,
    v: Vec<u16>,
    seq: Nclx,
}

#[cfg(not(target_os = "macos"))]
const EAGAIN: i32 = 11;
#[cfg(target_os = "macos")]
const EAGAIN: i32 = 35;

/// Largest picture rav1d may decode (pixels), against hostile sizes.
const MAX_PIXELS: u32 = 1 << 28;

fn decode_av1(obus: &[u8]) -> Result<Planes, String> {
    use rav1d::include::dav1d::data::Dav1dData;
    use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
    use rav1d::include::dav1d::headers::*;
    use rav1d::include::dav1d::picture::Dav1dPicture;
    use rav1d::src::lib::*;

    struct Ctx(Option<Dav1dContext>);
    impl Drop for Ctx {
        fn drop(&mut self) {
            if self.0.is_some() {
                // SAFETY: the context came from dav1d_open and is closed once.
                unsafe { dav1d_close(NonNull::new(&mut self.0)) };
            }
        }
    }
    struct Pic(Dav1dPicture);
    impl Drop for Pic {
        fn drop(&mut self) {
            // SAFETY: unref on a default or decoded picture is allowed.
            unsafe { dav1d_picture_unref(NonNull::new(&mut self.0)) };
        }
    }

    let mut s = std::mem::MaybeUninit::<Dav1dSettings>::uninit();
    // SAFETY: dav1d_default_settings fills every field.
    let mut s = unsafe {
        dav1d_default_settings(NonNull::new(s.as_mut_ptr()).unwrap());
        s.assume_init()
    };
    // Two threads and automatic frame delay, so rav1d uses more than one
    // frame context. Not for speed: with a single one, a decode error in
    // the picture data (any damaged file) takes a rav1d error path that
    // crashes (rav1d 1.1, decode.rs `on_error` unwraps a frame header that
    // is already gone), and that crash aborts the app. The other path
    // handles the same files without trouble (core/fuzz.rs, one process
    // per case: 0 of 432).
    s.n_threads = 2;
    s.max_frame_delay = 0;
    s.frame_size_limit = MAX_PIXELS;
    s.all_layers = 0;
    let mut ctx = Ctx(None);
    // SAFETY: valid pointers to a settings struct and an output slot.
    let r = unsafe { dav1d_open(NonNull::new(&mut ctx.0), NonNull::new(&mut s)) };
    if r.0 != 0 || ctx.0.is_none() {
        return Err("AV1 decoder could not start".into());
    }
    let mut data = Dav1dData::default();
    // SAFETY: dav1d_data_create gives a buffer of exactly `obus.len()` bytes.
    unsafe {
        let buf = dav1d_data_create(NonNull::new(&mut data), obus.len());
        if buf.is_null() {
            return Err("out of memory".into());
        }
        std::ptr::copy_nonoverlapping(obus.as_ptr(), buf, obus.len());
    }
    let mut pic = Pic(Dav1dPicture::default());
    let mut got = false;
    let mut drained = 0;
    for _ in 0..1000 {
        if data.sz > 0 {
            // SAFETY: context open, data owned by us.
            let r = unsafe { dav1d_send_data(ctx.0, NonNull::new(&mut data)) };
            if r.0 < 0 && r.0 != -EAGAIN {
                unsafe { dav1d_data_unref(NonNull::new(&mut data)) };
                return Err("damaged AV1 picture".into());
            }
        }
        // SAFETY: context open, picture slot owned by us.
        let r = unsafe { dav1d_get_picture(ctx.0, NonNull::new(&mut pic.0)) };
        if r.0 == 0 {
            got = true;
            break;
        }
        if r.0 != -EAGAIN {
            break;
        }
        if data.sz == 0 {
            // All sent: the next call drains the frame threads.
            drained += 1;
            if drained > 4 {
                break;
            }
        }
    }
    // SAFETY: unref of our own data (no-op when consumed).
    unsafe { dav1d_data_unref(NonNull::new(&mut data)) };
    if !got {
        return Err("damaged AV1 picture".into());
    }
    let p = &pic.0;
    let (w, h, bits) = (p.p.w as usize, p.p.h as usize, p.p.bpc as u8);
    let sub = match p.p.layout {
        DAV1D_PIXEL_LAYOUT_I400 => None,
        DAV1D_PIXEL_LAYOUT_I420 => Some((1, 1)),
        DAV1D_PIXEL_LAYOUT_I422 => Some((1, 0)),
        _ => Some((0, 0)),
    };
    let seq = p.seq_hdr.map_or(Nclx::default(), |sh| {
        // SAFETY: the picture holds a reference to its sequence header.
        let sh = unsafe { sh.as_ref() };
        Nclx {
            primaries: sh.pri as u16,
            transfer: sh.trc as u16,
            matrix: sh.mtrx as u16,
            full_range: sh.color_range != 0,
        }
    });
    let plane = |i: usize, pw: usize, ph: usize| -> Vec<u16> {
        let Some(ptr) = p.data[i] else {
            return Vec::new();
        };
        let stride = p.stride[i.min(1)];
        let mut out = Vec::with_capacity(pw * ph);
        for row in 0..ph {
            // SAFETY: rows of `pw` samples at `stride` bytes apart are
            // inside the picture dav1d allocated.
            unsafe {
                let start = (ptr.as_ptr() as *const u8).offset(row as isize * stride);
                if bits > 8 {
                    let s = std::slice::from_raw_parts(start as *const u16, pw);
                    out.extend_from_slice(s);
                } else {
                    let s = std::slice::from_raw_parts(start, pw);
                    out.extend(s.iter().map(|&b| b as u16));
                }
            }
        }
        out
    };
    let y = plane(0, w, h);
    let (u, v) = match sub {
        Some((sx, sy)) => {
            let (cw, ch) = ((w + sx as usize) >> sx, (h + sy as usize) >> sy);
            (plane(1, cw, ch), plane(2, cw, ch))
        }
        None => (Vec::new(), Vec::new()),
    };
    Ok(Planes {
        w,
        h,
        bits,
        sub,
        y,
        u,
        v,
        seq,
    })
}

/// Checks the OBU framing before rav1d sees it, since a crash inside rav1d
/// cannot be caught (it happens inside an `extern "C"` function, which
/// aborts the process): every OBU must carry its size, the sizes must add
/// up, a sequence header must come before the first frame, and every
/// sequence header must parse.
fn check_obus(mut data: &[u8]) -> Result<(), String> {
    let broken = || "damaged AV1 picture".to_string();
    let parses = |obu: &[u8]| {
        use rav1d::include::dav1d::headers::Dav1dSequenceHeader;
        let mut out = std::mem::MaybeUninit::<Dav1dSequenceHeader>::uninit();
        // SAFETY: `out` is writable, `obu` is a live slice of its length.
        let r = unsafe {
            rav1d::src::lib::dav1d_parse_sequence_header(
                NonNull::new(out.as_mut_ptr()),
                NonNull::new(obu.as_ptr() as *mut u8),
                obu.len(),
            )
        };
        r.0 == 0
    };
    let mut seq = false;
    let mut frame = false;
    while !data.is_empty() {
        let h = data[0];
        let kind = (h >> 3) & 15;
        let ext = h & 4 != 0;
        if h & 0x80 != 0 || h & 2 == 0 {
            return Err(broken());
        }
        let mut at = 1 + ext as usize;
        // LEB128 size.
        let mut size: u64 = 0;
        for i in 0..8 {
            let b = *data.get(at).ok_or_else(broken)?;
            at += 1;
            size |= ((b & 0x7F) as u64) << (7 * i);
            if b & 0x80 == 0 {
                break;
            }
            if i == 7 {
                return Err(broken());
            }
        }
        let end = at
            .checked_add(usize::try_from(size).map_err(|_| broken())?)
            .ok_or_else(broken)?;
        if end > data.len() {
            return Err(broken());
        }
        match kind {
            1 => {
                if !parses(&data[..end]) {
                    return Err(broken());
                }
                seq = true;
            }
            3 | 4 | 6 | 7 => {
                if !seq {
                    return Err(broken());
                }
                frame = true;
            }
            0 | 9..=14 => return Err(broken()),
            _ => {}
        }
        data = &data[end..];
    }
    if seq && frame {
        Ok(())
    } else {
        Err(broken())
    }
}

/// The item's AV1 data with its configuration OBUs (the sequence header)
/// in front, as dav1d wants them in one piece.
fn av1_of(meta: &Meta, file: &[u8], item: &Item) -> Result<Planes, String> {
    let mut obus = item.props.av1c.clone().unwrap_or_default();
    obus.extend(meta.data(file, item)?);
    check_obus(&obus)?;
    decode_av1(&obus)
}

// ---- Pixels ---------------------------------------------------------------

/// YUV → RGB constants (Kr, Kb) for an AV1/H.273 matrix code.
fn kr_kb(matrix: u16) -> (f32, f32) {
    match matrix {
        1 => (0.2126, 0.0722),      // BT.709
        9 | 10 => (0.2627, 0.0593), // BT.2020
        4 => (0.30, 0.11),          // FCC
        7 => (0.212, 0.087),        // SMPTE 240
        _ => (0.299, 0.114),        // BT.601 (also "unspecified", as libavif)
    }
}

fn to_rgba(p: &Planes, colour: Nclx) -> Vec<Color32> {
    let max = ((1u32 << p.bits) - 1) as f32;
    let s = (1u32 << (p.bits - 8)) as f32;
    let (y_off, y_scale, c_scale) = if colour.full_range {
        (0.0, 1.0 / max, 1.0 / max)
    } else {
        (16.0 * s, 1.0 / (219.0 * s), 1.0 / (224.0 * s))
    };
    let half = (1u32 << (p.bits - 1)) as f32;
    let (kr, kb) = kr_kb(colour.matrix);
    let kg = 1.0 - kr - kb;
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let mut out = Vec::with_capacity(p.w * p.h);
    let Some((sx, sy)) = p.sub else {
        out.extend(
            p.y.iter()
                .map(|&y| Color32::from_gray(q((y as f32 - y_off) * y_scale))),
        );
        return out;
    };
    let cw = (p.w + sx as usize) >> sx;
    for row in 0..p.h {
        let crow = (row >> sy) * cw;
        for col in 0..p.w {
            let y = (p.y[row * p.w + col] as f32 - y_off) * y_scale;
            let c = crow + (col >> sx);
            let cb = (p.u[c] as f32 - half) * c_scale;
            let cr = (p.v[c] as f32 - half) * c_scale;
            let px = if colour.matrix == 0 {
                // Identity: the planes are G, B, R.
                let g = p.y[row * p.w + col] as f32;
                Color32::from_rgb(
                    q((p.v[c] as f32 - y_off) * y_scale),
                    q((g - y_off) * y_scale),
                    q((p.u[c] as f32 - y_off) * y_scale),
                )
            } else {
                let r = y + 2.0 * (1.0 - kr) * cr;
                let b = y + 2.0 * (1.0 - kb) * cb;
                let g = (y - kr * r - kb * b) / kg;
                Color32::from_rgb(q(r), q(g), q(b))
            };
            out.push(px);
        }
    }
    out
}

/// Alpha values (0–255) from a monochrome AV1 picture.
fn alpha_of(p: &Planes, full_range: bool) -> Vec<u8> {
    let max = ((1u32 << p.bits) - 1) as f32;
    let s = (1u32 << (p.bits - 8)) as f32;
    p.y.iter()
        .map(|&a| {
            let v = if full_range {
                a as f32 / max
            } else {
                (a as f32 - 16.0 * s) / (219.0 * s)
            };
            (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
        })
        .collect()
}

/// An image item as pixels: one AV1 picture, or a grid of them.
/// Returns (pixels, size, first picture: for its colour description).
fn item_pixels(
    meta: &Meta,
    file: &[u8],
    id: u32,
    colour: Option<Nclx>,
    alpha: bool,
) -> Result<(Vec<Color32>, [usize; 2], Nclx, u8), String> {
    let item = meta.item(id)?;
    match &item.kind {
        b"av01" => {
            let p = av1_of(meta, file, item)?;
            let colour = colour.unwrap_or(p.seq);
            let px = if alpha {
                alpha_of(&p, colour.full_range)
                    .into_iter()
                    .map(Color32::from_gray)
                    .collect()
            } else {
                to_rgba(&p, colour)
            };
            Ok((px, [p.w, p.h], colour, p.bits))
        }
        b"grid" => {
            let desc = meta.data(file, item)?;
            let mut r = Reader(&desc);
            r.u8().ok_or_else(bad)?; // version
            let wide = r.u8().ok_or_else(bad)? & 1 != 0;
            let rows = r.u8().ok_or_else(bad)? as usize + 1;
            let cols = r.u8().ok_or_else(bad)? as usize + 1;
            let (w, h) = if wide {
                (r.u32().ok_or_else(bad)?, r.u32().ok_or_else(bad)?)
            } else {
                (
                    r.u16().ok_or_else(bad)? as u32,
                    r.u16().ok_or_else(bad)? as u32,
                )
            };
            let (w, h) = (w as usize, h as usize);
            if w == 0 || h == 0 || (w as u64 * h as u64) > MAX_PIXELS as u64 {
                return Err(bad());
            }
            let tiles = meta.referenced(b"dimg", id);
            if tiles.len() != rows * cols {
                return Err(bad());
            }
            let mut out = vec![Color32::BLACK; w * h];
            let mut first: Option<(Nclx, u8)> = None;
            let mut tile_size = [0, 0];
            for (i, &t) in tiles.iter().enumerate() {
                let (px, [tw, th], c, bits) = item_pixels(meta, file, t, colour, alpha)?;
                if i == 0 {
                    tile_size = [tw, th];
                    first = Some((c, bits));
                } else if [tw, th] != tile_size {
                    return Err(bad());
                }
                let (x0, y0) = ((i % cols) * tw, (i / cols) * th);
                for row in 0..th {
                    let y = y0 + row;
                    if y >= h || x0 >= w {
                        continue;
                    }
                    let n = tw.min(w - x0);
                    out[y * w + x0..y * w + x0 + n].copy_from_slice(&px[row * tw..row * tw + n]);
                }
            }
            let (c, bits) = first.ok_or_else(bad)?;
            Ok((out, [w, h], c, bits))
        }
        k => Err(format!(
            "unsupported AVIF image item ({})",
            String::from_utf8_lossy(k)
        )),
    }
}

/// HEIF's rotation (anticlockwise) and mirror as our draw-time orientation
/// (clockwise turns, then a left-right mirror).
fn orientation(p: &Props) -> Orientation {
    let turns = (4 - p.rotation) % 4;
    match p.mirror {
        None => Orientation { turns, flip: false },
        Some(0) => Orientation { turns, flip: true },
        // Top-bottom mirror = left-right mirror after a half turn.
        Some(_) => Orientation {
            turns: (turns + 2) % 4,
            flip: true,
        },
    }
}

/// ICC bytes for colours that are not sRGB: the file's own profile, or
/// one made from its CICP primaries (Display P3, BT.2020).
fn icc_for(props: &Props, colour: Nclx) -> Option<Vec<u8>> {
    if let Some(icc) = &props.icc {
        return Some(icc.clone());
    }
    let profile = match colour.primaries {
        12 => moxcms::ColorProfile::new_display_p3(),
        9 => moxcms::ColorProfile::new_bt2020(),
        _ => return None,
    };
    profile.encode().ok()
}

/// The Exif item describing image `id`: a TIFF block after a 4-byte offset.
fn exif_of(meta: &Meta, bytes: &[u8], id: u32) -> Option<super::exif_info::ExifInfo> {
    meta.referring(b"cdsc", id)
        .filter_map(|id| meta.item(id).ok())
        .find(|i| &i.kind == b"Exif")
        .and_then(|i| meta.data(bytes, i).ok())
        .and_then(|d| {
            let off = u32::from_be_bytes(d.get(..4)?.try_into().ok()?) as usize;
            d.get(4 + off..).map(<[u8]>::to_vec)
        })
        .and_then(super::exif_info::read_tiff)
        .filter(|e| !e.is_empty())
}

/// What a HEIF file (HEIC too) says about its main picture, beside the
/// pixels: for decoders that only give pixels (Windows' HEIC codec).
pub struct HeifInfo {
    pub orientation: Orientation,
    /// Stored size (`clap`, else `ispe`), before rotation.
    pub size: Option<[u32; 2]>,
    /// Colour profile when not sRGB: the file's ICC, or one made from CICP.
    pub icc: Option<Vec<u8>>,
    pub exif: Option<super::exif_info::ExifInfo>,
}

pub fn heif_info(bytes: &[u8]) -> Result<HeifInfo, String> {
    let meta = parse(bytes)?;
    let primary = meta.item(meta.primary)?;
    let first_tile = meta.referenced(b"dimg", primary.id).first().copied();
    let tile = first_tile.and_then(|t| meta.item(t).ok());
    let mut props = primary.props.clone();
    if props.icc.is_none() && props.nclx.is_none() {
        if let Some(t) = tile {
            props.icc = t.props.icc.clone();
            props.nclx = t.props.nclx;
        }
    }
    Ok(HeifInfo {
        orientation: orientation(&primary.props),
        size: primary.props.clean.or(primary.props.size),
        icc: icc_for(&props, props.nclx.unwrap_or_default()),
        exif: exif_of(&meta, bytes, primary.id),
    })
}

/// HEIF brands whose pictures are HEVC (HEIC): decoded by Windows.
pub fn is_heic(bytes: &[u8]) -> bool {
    let Some((b"ftyp", body)) = boxes(bytes).next() else {
        return false;
    };
    body.chunks_exact(4).enumerate().any(|(i, b)| {
        i != 1 && matches!(b, b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis")
    }) && !is_avif(bytes)
}

pub fn decode(bytes: &[u8], opts: DecodeOpts) -> Result<Decoded, String> {
    let meta = parse(bytes)?;
    let primary = meta.item(meta.primary)?.clone();
    // Colour from `colr` on the image (or its first tile), else from AV1.
    let first_tile = meta.referenced(b"dimg", primary.id).first().copied();
    let tile_nclx = first_tile
        .and_then(|t| meta.item(t).ok())
        .and_then(|i| i.props.nclx);
    let nclx = primary.props.nclx.or(tile_nclx);
    let (pixels, [w, h], colour, bits) = item_pixels(&meta, bytes, primary.id, nclx, false)?;
    let mut image = ColorImage::new([w, h], pixels);

    // Alpha: an auxiliary image referring to this one.
    let alpha_item = meta
        .referring(b"auxl", primary.id)
        .find(|&a| meta.item(a).is_ok_and(|i| i.props.alpha_urn));
    let mut has_alpha = false;
    if let Some(a) = alpha_item {
        if let Ok((alpha, size, _, _)) = item_pixels(&meta, bytes, a, None, true) {
            if size == [w, h] {
                has_alpha = alpha.iter().any(|p| p.r() != 255);
                for (px, a) in image.pixels.iter_mut().zip(alpha) {
                    *px = Color32::from_rgba_unmultiplied(px.r(), px.g(), px.b(), a.r());
                }
            }
        }
    }

    let exif = exif_of(&meta, bytes, primary.id);

    let o = orientation(&primary.props);
    let icc = if opts.color {
        icc_for(&primary.props, colour)
    } else {
        None
    };
    let (mut image, mut tiles, reduced, kept) = finish_keep(image, opts, o);
    let tiled = tiled_size(&tiles);
    let (profile, converted) = super::decode::manage_color(icc.as_deref(), &mut image, &mut tiles);
    let (uw, uh) = if o.swaps_axes() { (h, w) } else { (w, h) };
    let d = Decoded {
        histogram: histogram_of(&image),
        frames: vec![Frame {
            image: Arc::new(image),
            delay: Duration::ZERO,
        }],
        width: uw as u32,
        height: uh as u32,
        orientation: o,
        format: "AVIF",
        color: format!("YUV {bits}") + if has_alpha { " + alpha" } else { "" },
        has_alpha,
        file_size: 0,
        modified: None,
        exif,
        decode_time: Duration::ZERO,
        reduced,
        tiles,
        tiled_size: tiled,
        profile,
        color_converted: converted,
        complete: true,
        full: None,
    };
    Ok(with_full(d, kept.filter(|_| !converted)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;

    pub fn data(name: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/data")
                .join(name),
        )
        .unwrap()
    }

    fn open(name: &str) -> Decoded {
        let b = data(name);
        assert!(is_avif(&b), "{name}");
        crate::core::decode::decode_bytes(&b, Path::new(name), DecodeOpts::full(8192)).unwrap()
    }

    fn rgb(d: &Decoded, i: usize) -> (u8, u8, u8) {
        let p = d.frames[0].image.pixels[i];
        (p.r(), p.g(), p.b())
    }

    fn near(a: (u8, u8, u8), b: (u8, u8, u8), tol: i32) -> bool {
        (a.0 as i32 - b.0 as i32).abs() <= tol
            && (a.1 as i32 - b.1 as i32).abs() <= tol
            && (a.2 as i32 - b.2 as i32).abs() <= tol
    }

    #[test]
    fn avif_photo_alpha_gray_and_ten_bit() {
        let d = open("photo.avif");
        assert_eq!((d.width, d.height, d.format), (64, 48, "AVIF"));
        assert!(
            near(rgb(&d, 4 * 64 + 4), (255, 0, 0), 12),
            "{:?}",
            rgb(&d, 4 * 64 + 4)
        );
        // The gradient: right edge reddish, bottom greenish.
        assert!(
            near(rgb(&d, 40 * 64 + 60), (240, 213, 40), 16),
            "{:?}",
            rgb(&d, 40 * 64 + 60)
        );

        let d = open("alpha.avif");
        assert!(d.has_alpha);
        assert!(near(rgb(&d, 0), (200, 30, 90), 4), "{:?}", rgb(&d, 0));
        assert_eq!(d.frames[0].image.pixels[15].a(), 0);
        assert_eq!(d.frames[0].image.pixels[0].a(), 255);

        let d = open("gray.avif");
        assert_eq!((d.width, d.height), (24, 16));
        assert!(near(rgb(&d, 5), (50, 50, 50), 2), "{:?}", rgb(&d, 5));

        let d = open("ten.avif");
        assert!(d.color.contains("10"), "{}", d.color);
        assert!(
            near(rgb(&d, 4 * 64 + 4), (255, 0, 0), 12),
            "{:?}",
            rgb(&d, 4 * 64 + 4)
        );
    }

    #[test]
    fn avif_rotation_exif_and_wide_gamut() {
        // Saved with EXIF orientation 6: an irot box, shown upright.
        let d = open("rot.avif");
        assert_eq!(d.orientation, Orientation::from_exif(6));
        assert_eq!((d.width, d.height), (48, 64));
        assert_eq!(
            d.exif.as_ref().and_then(|e| e.make.as_deref()),
            Some("MuudCam")
        );

        // Display P3 orange (200, 100, 50) is more saturated in sRGB.
        let d = open("p3.avif");
        assert!(d.color_converted);
        let (r, g, b) = rgb(&d, 0);
        assert!(r > 205 && g < 100 && b < 50, "{:?}", (r, g, b));
    }

    #[test]
    fn heif_transforms_map_to_draw_orientation() {
        let o = |rotation, mirror| {
            orientation(&Props {
                rotation,
                mirror,
                ..Props::default()
            })
        };
        // irot 3 (270° anticlockwise) = 90° clockwise = EXIF 6.
        assert_eq!(o(3, None), Orientation::from_exif(6));
        assert_eq!(o(1, None), Orientation::from_exif(8));
        assert_eq!(o(2, None), Orientation::from_exif(3));
        assert_eq!(o(0, Some(0)), Orientation::from_exif(2));
        assert_eq!(o(0, Some(1)), Orientation::from_exif(4));
    }

    /// A 2×1 grid AVIF made from two copies of `tile`'s picture.
    pub fn grid_of(tile: &[u8]) -> Vec<u8> {
        let meta = parse(tile).unwrap();
        let item = meta.item(meta.primary).unwrap();
        let av1 = meta.data(tile, item).unwrap();
        let av1c = item.props.av1c.clone().unwrap();
        let [tw, th] = item.props.size.unwrap();
        let bx = |kind: &[u8; 4], body: &[u8]| {
            let mut b = ((body.len() + 8) as u32).to_be_bytes().to_vec();
            b.extend_from_slice(kind);
            b.extend_from_slice(body);
            b
        };
        let full = |v: u8, body: &[u8]| {
            let mut b = vec![v, 0, 0, 0];
            b.extend_from_slice(body);
            b
        };
        let infe = |id: u16, kind: &[u8; 4]| {
            let mut b = id.to_be_bytes().to_vec();
            b.extend_from_slice(&[0, 0]);
            b.extend_from_slice(kind);
            b.push(0);
            bx(b"infe", &full(2, &b))
        };
        let mut iinf = 3u16.to_be_bytes().to_vec();
        iinf.extend(infe(1, b"grid"));
        iinf.extend(infe(2, b"av01"));
        iinf.extend(infe(3, b"av01"));
        let w = (tw * 2 - 8) as u16; // cropped a little: the grid can be smaller
        let grid_desc = [
            &[0u8, 0, 0, 1][..],
            &w.to_be_bytes(),
            &(th as u16).to_be_bytes(),
        ]
        .concat();
        let ispe = |w: u32, h: u32| {
            bx(
                b"ispe",
                &full(0, &[w.to_be_bytes(), h.to_be_bytes()].concat()),
            )
        };
        let av1c_box = bx(b"av1C", &[&[0x81u8, 0x00, 0x0C, 0x00][..], &av1c].concat());
        let ipco = bx(
            b"ipco",
            &[ispe(w as u32, th), ispe(tw, th), av1c_box].concat(),
        );
        // item 1: ispe(grid); items 2, 3: ispe(tile) + av1C
        let ipma_body = [
            &3u32.to_be_bytes()[..],
            &[0, 1, 1, 0x81],
            &[0, 2, 2, 0x02, 0x83],
            &[0, 3, 2, 0x02, 0x83],
        ]
        .concat();
        let iprp = bx(b"iprp", &[ipco, bx(b"ipma", &full(0, &ipma_body))].concat());
        let dimg = bx(
            b"dimg",
            &[
                &1u16.to_be_bytes()[..],
                &2u16.to_be_bytes(),
                &2u16.to_be_bytes(),
                &3u16.to_be_bytes(),
            ]
            .concat(),
        );
        let iref = bx(b"iref", &full(0, &dimg));
        let ftyp = bx(b"ftyp", b"avif\0\0\0\0avifmif1miaf");
        // Data: grid description in idat, the tiles in mdat.
        let idat = bx(b"idat", &grid_desc);
        let hdlr = bx(b"hdlr", &full(0, b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0\0"));
        let pitm = bx(b"pitm", &full(0, &1u16.to_be_bytes()));
        let build = |mdat_at: u32| {
            let mut iloc = vec![0x44, 0x00];
            iloc.extend_from_slice(&3u16.to_be_bytes());
            // item 1 in idat (construction method 1)
            iloc.extend_from_slice(&1u16.to_be_bytes());
            iloc.extend_from_slice(&[0, 1, 0, 0, 0, 1]);
            iloc.extend_from_slice(&0u32.to_be_bytes());
            iloc.extend_from_slice(&(grid_desc.len() as u32).to_be_bytes());
            for (id, at) in [(2u16, mdat_at), (3, mdat_at + av1.len() as u32)] {
                iloc.extend_from_slice(&id.to_be_bytes());
                iloc.extend_from_slice(&[0, 0, 0, 0, 0, 1]);
                iloc.extend_from_slice(&at.to_be_bytes());
                iloc.extend_from_slice(&(av1.len() as u32).to_be_bytes());
            }
            let iloc = bx(b"iloc", &full(1, &iloc));
            let iinf = bx(b"iinf", &full(0, &iinf));
            let meta = bx(
                b"meta",
                &full(
                    0,
                    &[
                        hdlr.clone(),
                        pitm.clone(),
                        iloc,
                        iinf,
                        iref.clone(),
                        iprp.clone(),
                        idat.clone(),
                    ]
                    .concat(),
                ),
            );
            [ftyp.clone(), meta].concat()
        };
        let head = build(0).len() as u32 + 8;
        let mut out = build(head);
        out.extend(bx(b"mdat", &[av1.clone(), av1].concat()));
        out
    }

    #[test]
    fn avif_grids_are_put_together() {
        let tile = data("photo.avif");
        let grid = grid_of(&tile);
        let d =
            crate::core::decode::decode_bytes(&grid, Path::new("g.avif"), DecodeOpts::full(8192))
                .unwrap();
        assert_eq!((d.width, d.height), (64 * 2 - 8, 48));
        let img = &d.frames[0].image;
        // The red corner of each tile.
        let at = |x: usize, y: usize| {
            let p = img.pixels[y * img.size[0] + x];
            (p.r(), p.g(), p.b())
        };
        assert!(near(at(4, 4), (255, 0, 0), 12));
        assert!(near(at(64 + 4, 4), (255, 0, 0), 12));
        assert!(!near(at(32, 30), (255, 0, 0), 60));
    }

    #[test]
    fn not_avif_and_broken_containers() {
        assert!(!is_avif(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic"));
        assert!(is_avif(b"\0\0\0\x18ftypmif1\0\0\0\0mif1avif"));
        let b = data("photo.avif");
        for cut in [20, 40, 100, 200, b.len() - 10] {
            assert!(
                decode(&b[..cut], DecodeOpts::full(8192)).is_err(),
                "cut at {cut}"
            );
        }
    }
}
