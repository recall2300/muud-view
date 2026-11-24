//! Colour management: images with an embedded ICC profile (Display P3 from
//! phones, Adobe RGB from cameras, ...) are converted to sRGB so they look
//! the way they were shot instead of washed out or oversaturated.
//!
//! Uses moxcms (pure Rust, BSD-3-Clause OR Apache-2.0) — the MPL-licensed
//! qcms is ruled out by our licence policy, and LittleCMS would need a C
//! build. moxcms is already in the tree as a dependency of `image`.

use std::sync::Arc;

use eframe::egui::{Color32, ColorImage};
use moxcms::{
    ColorProfile, DataColorSpace, Layout, ProfileText, TransformExecutor, TransformOptions,
};

/// A ready conversion from an image's profile to sRGB.
pub struct ColorFix {
    transform: Arc<dyn TransformExecutor<u8> + Send + Sync>,
}

/// What an embedded profile means for drawing.
pub enum Profile {
    /// No conversion needed (sRGB, or a profile we cannot use).
    Keep(Option<String>),
    /// Convert with this, from the named profile.
    Convert(String, ColorFix),
}

impl Profile {
    pub fn name(&self) -> Option<&str> {
        match self {
            Profile::Keep(n) => n.as_deref(),
            Profile::Convert(n, _) => Some(n),
        }
    }

    pub fn fix(&self) -> Option<&ColorFix> {
        match self {
            Profile::Convert(_, f) => Some(f),
            Profile::Keep(_) => None,
        }
    }
}

/// Reads an embedded ICC profile and prepares a conversion to sRGB if one
/// is needed.
pub fn prepare(icc: &[u8]) -> Profile {
    let Ok(src) = ColorProfile::new_from_slice(icc) else {
        return Profile::Keep(None);
    };
    let name = profile_name(&src);
    // Only RGB profiles: grey and CMYK JPEGs are already turned into RGB
    // by the decoder, and the profile no longer matches those pixels.
    if src.color_space != DataColorSpace::Rgb {
        return Profile::Keep(name);
    }
    if name
        .as_deref()
        .is_some_and(|n| n.to_ascii_lowercase().contains("srgb"))
    {
        return Profile::Keep(name);
    }
    let dst = ColorProfile::new_srgb();
    match src.create_transform_8bit(
        Layout::Rgba,
        &dst,
        Layout::Rgba,
        TransformOptions::default(),
    ) {
        Ok(t) => Profile::Convert(
            name.unwrap_or_else(|| "ICC".into()),
            ColorFix { transform: t },
        ),
        Err(_) => Profile::Keep(name),
    }
}

fn profile_name(p: &ColorProfile) -> Option<String> {
    let text = match p.description.as_ref()? {
        ProfileText::PlainString(s) => s.clone(),
        ProfileText::Localizable(v) => v
            .iter()
            .find(|l| l.language == "en")
            .or(v.first())?
            .value
            .clone(),
        ProfileText::Description(d) => {
            if d.unicode_string.is_empty() {
                d.ascii_string.clone()
            } else {
                d.unicode_string.clone()
            }
        }
    };
    let text = text.trim_matches(char::from(0)).trim().to_string();
    (!text.is_empty()).then_some(text)
}

impl ColorFix {
    /// Converts the pixels to sRGB in place.
    pub fn apply(&self, img: &mut ColorImage) {
        let opaque = img.pixels.iter().all(|p| p.a() == 255);
        let src: Vec<u8> = if opaque {
            bytemuck::cast_slice(&img.pixels).to_vec()
        } else {
            // Profiles describe straight (not premultiplied) colour.
            img.pixels
                .iter()
                .flat_map(|p| p.to_srgba_unmultiplied())
                .collect()
        };
        let mut dst = vec![0u8; src.len()];
        if self.transform.transform(&src, &mut dst).is_err() {
            return;
        }
        if opaque {
            bytemuck::cast_slice_mut::<Color32, u8>(&mut img.pixels).copy_from_slice(&dst);
        } else {
            for (p, c) in img.pixels.iter_mut().zip(dst.chunks_exact(4)) {
                *p = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_p3_is_converted_and_srgb_left_alone() {
        let p3 = ColorProfile::new_display_p3().encode().unwrap();
        let profile = prepare(&p3);
        let fix = profile.fix().expect("P3 needs converting");
        assert!(profile.name().is_some());
        // Neutral grey stays grey (same white point and curve)...
        let mut img = ColorImage::new([2, 1], vec![Color32::from_gray(128); 2]);
        // ...and a P3 red is more saturated than the same numbers in sRGB.
        img.pixels[1] = Color32::from_rgb(200, 60, 60);
        fix.apply(&mut img);
        let g = img.pixels[0];
        assert!(
            (g.r() as i32 - 128).abs() <= 2 && g.r() == g.g() && g.g() == g.b(),
            "{g:?}"
        );
        let r = img.pixels[1];
        assert!(r.r() > 205 && r.g() < 60, "{r:?}");

        let srgb = ColorProfile::new_srgb().encode().unwrap();
        assert!(prepare(&srgb).fix().is_none());
        assert!(prepare(b"not a profile").fix().is_none());
    }

    #[test]
    fn transparent_pixels_survive() {
        let p3 = ColorProfile::new_display_p3().encode().unwrap();
        let profile = prepare(&p3);
        let mut img = ColorImage::new(
            [2, 1],
            vec![
                Color32::from_rgba_unmultiplied(200, 60, 60, 128),
                Color32::TRANSPARENT,
            ],
        );
        profile.fix().unwrap().apply(&mut img);
        assert_eq!(img.pixels[0].a(), 128);
        assert_eq!(img.pixels[1], Color32::TRANSPARENT);
    }
}
