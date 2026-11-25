//! JPEG: libjpeg-turbo first (SIMD, can shrink while decoding), zune-jpeg
//! for what it does not take.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{Color32, ColorImage};

use super::*;
/// zune-jpeg straight into the texture buffer: no RGB→RGBA pass and no
/// premultiply pass (JPEG has no alpha).
pub(super) fn decode_jpeg(
    bytes: &[u8],
    opts: DecodeOpts,
    orientation: Orientation,
    exif: Option<ExifInfo>,
) -> Result<Decoded, String> {
    // libjpeg-turbo first (SIMD, and it can shrink while decoding); zune-jpeg
    // for what it does not take (CMYK, lossless, damaged files).
    let px = match turbo_pixels(bytes, opts, orientation) {
        Ok(px) => px,
        Err(_) => zune_pixels(bytes)?,
    };
    let JpegPixels {
        image,
        full_size: [w, h],
        gray,
    } = px;
    let icc = if opts.color {
        icc_from_jpeg(bytes)
    } else {
        None
    };
    // A shrunk decode is never kept as "full" (it is not).
    let shrunk = image.size != [w, h];
    let opts = DecodeOpts {
        keep_full: opts.keep_full && !shrunk,
        ..opts
    };
    let (mut image, mut tiles, reduced, kept) = finish_keep(image, opts, orientation);
    let reduced = reduced || shrunk;
    let tiled_size = tiled_size(&tiles);
    let (profile, color_converted) = manage_color(icc.as_deref(), &mut image, &mut tiles);

    let (uw, uh) = if orientation.swaps_axes() {
        (h, w)
    } else {
        (w, h)
    };
    let histogram = histogram_of(&image);
    let decoded = Decoded {
        frames: vec![Frame {
            image: Arc::new(image),
            delay: Duration::ZERO,
        }],
        width: uw as u32,
        height: uh as u32,
        orientation,
        format: "JPEG",
        color: if gray { "GRAY 8" } else { "YCbCr 8" }.into(),
        has_alpha: false,
        file_size: 0,
        modified: None,
        exif,
        histogram,
        decode_time: Duration::ZERO,
        reduced,
        tiles,
        tiled_size,
        profile,
        color_converted,
        complete: true,
        full: None,
    };
    // Zooming in later uses these pixels instead of decoding again.
    // Not when colours were converted: that would cost a full-size
    // conversion for every image, zoomed or not.
    Ok(with_full(decoded, kept.filter(|_| !color_converted)))
}

pub(super) struct JpegPixels {
    image: ColorImage,
    /// The JPEG's own size (the image may be smaller if shrunk on decode).
    full_size: [usize; 2],
    gray: bool,
}

/// The scaling factor to decode a `size` JPEG at so it is still at least
/// `target`: libjpeg-turbo shrinks in the IDCT itself, much cheaper than
/// decoding everything and shrinking. Only 1/2, 1/4 and 1/8 — those have
/// SIMD code; the other n/8 factors run plain C, which on Windows (MSVC)
/// is slower than a full decode.
pub fn scale_for(size: [usize; 2], target: [usize; 2]) -> turbojpeg::ScalingFactor {
    use turbojpeg::ScalingFactor as F;
    [F::ONE_EIGHTH, F::ONE_QUARTER, F::ONE_HALF]
        .into_iter()
        .find(|f| f.scale(size[0]) >= target[0] && f.scale(size[1]) >= target[1])
        .unwrap_or(F::ONE)
}

pub(super) fn turbo_pixels(
    bytes: &[u8],
    opts: DecodeOpts,
    o: Orientation,
) -> Result<JpegPixels, String> {
    use turbojpeg::{Colorspace, Decompressor, Image, PixelFormat};
    let mut d = Decompressor::new().map_err(|e| e.to_string())?;
    let header = d.read_header(bytes).map_err(|e| e.to_string())?;
    if header.is_lossless || matches!(header.colorspace, Colorspace::CMYK | Colorspace::YCCK) {
        return Err("left to zune-jpeg".into());
    }
    let (w, h) = (header.width, header.height);
    // Screen quality without keeping the full pixels: shrink while decoding.
    let factor = match opts.fit {
        Some(fit) if !opts.keep_full => match screen_target([w, h], fit, o) {
            Some(t) => scale_for([w, h], t),
            None => turbojpeg::ScalingFactor::ONE,
        },
        _ => turbojpeg::ScalingFactor::ONE,
    };
    d.set_scaling_factor(factor).map_err(|e| e.to_string())?;
    let (sw, sh) = (factor.scale(w), factor.scale(h));
    let mut pixels = vec![Color32::BLACK; sw * sh];
    let out = Image {
        pixels: bytemuck::cast_slice_mut::<Color32, u8>(&mut pixels),
        width: sw,
        pitch: sw * 4,
        height: sh,
        format: PixelFormat::RGBA,
    };
    d.decompress(bytes, out).map_err(|e| e.to_string())?;
    Ok(JpegPixels {
        image: ColorImage::new([sw, sh], pixels),
        full_size: [w, h],
        gray: header.colorspace == Colorspace::Gray,
    })
}

pub(super) fn zune_pixels(bytes: &[u8]) -> Result<JpegPixels, String> {
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;

    let zopts = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(1 << 16)
        .set_max_height(1 << 16);
    let mut dec = zune_jpeg::JpegDecoder::new_with_options(Cursor::new(bytes), zopts);
    dec.decode_headers().map_err(|e| format!("{e:?}"))?;
    let (w, h) = dec.dimensions().ok_or("no dimensions")?;
    let gray = matches!(dec.input_colorspace(), Some(ColorSpace::Luma));
    let mut pixels = vec![Color32::BLACK; w * h];
    dec.decode_into(bytemuck::cast_slice_mut(&mut pixels))
        .map_err(|e| format!("{e:?}"))?;
    Ok(JpegPixels {
        image: ColorImage::new([w, h], pixels),
        full_size: [w, h],
        gray,
    })
}

/// The ICC profile stored in a JPEG's APP2 segments (it may be split over
/// several, numbered from 1).
pub fn icc_from_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut parts: Vec<(u8, &[u8])> = Vec::new();
    let mut i = 2;
    while i + 4 <= bytes.len() && bytes[i] == 0xFF {
        let marker = bytes[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let body = bytes.get(i + 4..i + 2 + len)?;
        if marker == 0xE2 && body.len() > 14 && body.starts_with(b"ICC_PROFILE\0") {
            parts.push((body[12], &body[14..]));
        }
        i += 2 + len;
    }
    if parts.is_empty() {
        return None;
    }
    parts.sort_by_key(|p| p.0);
    Some(
        parts
            .into_iter()
            .flat_map(|p| p.1.iter().copied())
            .collect(),
    )
}
