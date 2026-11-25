//! HEIC (iPhone photos): pixels from Windows' own codec (see
//! `platform::wic` for why), everything else — rotation, colour profile,
//! Exif — from the file, with the HEIF reader in `avif`.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::ColorImage;

use super::decode::{
    finish_keep, histogram_of, manage_color, tiled_size, with_full, DecodeOpts, Decoded, Frame,
    Orientation,
};

pub use super::avif::is_heic;

pub fn decode(bytes: &[u8], opts: DecodeOpts) -> Result<Decoded, String> {
    let info = super::avif::heif_info(bytes)?;
    let (rgba, [w, h]) = crate::platform::wic::decode(bytes)?;
    Ok(build(&rgba, [w, h], info, opts))
}

/// The orientation still to apply to `size` pixels: none if the decoder
/// already turned them (its size is the stored size with sides swapped).
fn still_to_turn(info_size: Option<[u32; 2]>, o: Orientation, size: [usize; 2]) -> Orientation {
    match info_size {
        Some([sw, sh]) if sw != sh && o.swaps_axes() && size == [sh as usize, sw as usize] => {
            Orientation::default()
        }
        _ => o,
    }
}

fn build(
    rgba: &[u8],
    [w, h]: [usize; 2],
    info: super::avif::HeifInfo,
    opts: DecodeOpts,
) -> Decoded {
    let image = ColorImage::from_rgba_unmultiplied([w, h], rgba);
    let has_alpha = image.pixels.iter().any(|p| p.a() < 255);
    let o = still_to_turn(info.size, info.orientation, [w, h]);
    let icc = if opts.color { info.icc } else { None };
    let (mut image, mut tiles, reduced, kept) = finish_keep(image, opts, o);
    let tiled = tiled_size(&tiles);
    let (profile, converted) = manage_color(icc.as_deref(), &mut image, &mut tiles);
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
        format: "HEIC",
        color: "YUV".into(),
        has_alpha,
        file_size: 0,
        modified: None,
        exif: info.exif,
        decode_time: Duration::ZERO,
        reduced,
        tiles,
        tiled_size: tiled,
        profile,
        color_converted: converted,
        complete: true,
        full: None,
    };
    with_full(d, kept.filter(|_| !converted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heic_container_is_read_and_turned() {
        let b = crate::core::avif::tests::data("rot.heic");
        assert!(is_heic(&b));
        assert!(!is_heic(&crate::core::avif::tests::data("photo.avif")));
        let info = crate::core::avif::heif_info(&b).unwrap();
        assert_eq!(info.orientation, Orientation::from_exif(6));
        assert_eq!(info.size, Some([64, 48]));
        assert_eq!(
            info.exif.as_ref().and_then(|e| e.make.as_deref()),
            Some("MuudCam")
        );
        // Pixels as a decoder that does not turn them gives them.
        let rgba = vec![200u8; 64 * 48 * 4];
        let d = build(&rgba, [64, 48], info, DecodeOpts::full(8192));
        assert_eq!((d.width, d.height, d.format), (48, 64, "HEIC"));
        assert_eq!(d.orientation, Orientation::from_exif(6));
    }

    #[test]
    fn pixels_already_turned_are_not_turned_again() {
        let o = Orientation::from_exif(6);
        assert_eq!(still_to_turn(Some([64, 48]), o, [64, 48]), o);
        assert_eq!(
            still_to_turn(Some([64, 48]), o, [48, 64]),
            Orientation::default()
        );
        assert_eq!(still_to_turn(None, o, [48, 64]), o);
    }

    #[test]
    fn heic_opens_or_explains_what_is_missing() {
        let b = crate::core::avif::tests::data("rot.heic");
        match crate::core::decode::decode_bytes(
            &b,
            std::path::Path::new("a.heic"),
            DecodeOpts::full(8192),
        ) {
            // A Windows with the HEIF and HEVC extensions.
            Ok(d) => assert_eq!((d.width, d.height), (48, 64)),
            Err(e) => assert!(e.contains("HEIC"), "{e}"),
        }
    }
}
