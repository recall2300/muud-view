//! Windows Imaging Component: pictures decoded by codecs installed in
//! Windows. Used for HEIC (iPhone photos): its HEVC decoder is under patent
//! pools and its libraries are LGPL, so muud view does not carry one; with
//! Microsoft's "HEIF Image Extensions" and "HEVC Video Extensions" installed,
//! Windows decodes it. Only the pixels come from here; rotation, colour
//! profile and Exif are read from the file by `core::avif`'s HEIF reader.

#![cfg_attr(not(windows), allow(dead_code))]

/// Shown when the codec is not installed.
pub const MISSING_CODEC: &str = "HEIC needs the HEIF Image Extensions and HEVC Video Extensions \
    (Microsoft Store)";

/// Largest picture accepted (pixels), against hostile sizes.
const MAX_PIXELS: u64 = 1 << 28;

/// The first frame as RGBA (not premultiplied), and its size.
#[cfg(windows)]
pub fn decode(bytes: &[u8]) -> Result<(Vec<u8>, [usize; 2]), String> {
    use windows::core::IUnknown;
    use windows::Win32::Foundation::WINCODEC_ERR_COMPONENTNOTFOUND;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory, IWICPalette,
        WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };

    thread_local! {
        // Once per thread, never undone (the decoder threads live as long as
        // the app). A thread that already has COM (the UI thread, for drag
        // and drop) keeps its mode; WIC works in either.
        static COM: () = {
            // SAFETY: plain COM initialisation for this thread.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        };
    }
    COM.with(|_| {});
    let err = |e: windows::core::Error| {
        if e.code() == WINCODEC_ERR_COMPONENTNOTFOUND {
            MISSING_CODEC.to_string()
        } else {
            format!("Windows could not decode this file ({e})")
        }
    };
    // SAFETY: COM calls on interfaces we hold; `bytes` outlives the stream
    // and every object reading from it (all dropped before returning).
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(
            &CLSID_WICImagingFactory,
            None::<&IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
        .map_err(err)?;
        let stream = factory.CreateStream().map_err(err)?;
        stream.InitializeFromMemory(bytes).map_err(err)?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .map_err(err)?;
        let frame = decoder.GetFrame(0).map_err(err)?;
        let rgba = factory.CreateFormatConverter().map_err(err)?;
        rgba.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None::<&IWICPalette>,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .map_err(err)?;
        let (mut w, mut h) = (0u32, 0u32);
        rgba.GetSize(&mut w, &mut h).map_err(err)?;
        if w == 0 || h == 0 || w as u64 * h as u64 > MAX_PIXELS {
            return Err("picture too large".into());
        }
        let mut buf = vec![0u8; w as usize * h as usize * 4];
        rgba.CopyPixels(std::ptr::null(), w * 4, &mut buf)
            .map_err(err)?;
        Ok((buf, [w as usize, h as usize]))
    }
}

#[cfg(not(windows))]
pub fn decode(_bytes: &[u8]) -> Result<(Vec<u8>, [usize; 2]), String> {
    Err("HEIC files are decoded by Windows (HEIF and HEVC Video Extensions)".into())
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn windows_decodes_through_wic() {
        // PNG: a codec every Windows has, to check the plumbing.
        let img = image::RgbaImage::from_fn(5, 3, |x, y| {
            image::Rgba([x as u8 * 50, y as u8 * 80, 7, 200])
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let (px, size) = super::decode(&png).unwrap();
        assert_eq!(size, [5, 3]);
        let at = |x: usize, y: usize| &px[(y * 5 + x) * 4..(y * 5 + x) * 4 + 4];
        assert_eq!(at(4, 2), &[200, 160, 7, 200]);
        assert!(super::decode(b"not a picture").is_err());
    }
}
