//! The handful of EXIF fields the info panel shows.

use exif::{In, Tag, Value};

#[derive(Clone, Debug, Default)]
pub struct ExifInfo {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub taken: Option<String>,
    pub aperture: Option<String>,
    pub shutter: Option<String>,
    pub iso: Option<String>,
    pub focal: Option<String>,
    pub focal_35: Option<String>,
    pub bias: Option<String>,
    pub flash: Option<String>,
    pub gps: Option<(f64, f64)>,
    pub altitude: Option<String>,
    /// EXIF Orientation tag (1 = upright).
    pub orientation: u32,
}

impl ExifInfo {
    pub fn is_empty(&self) -> bool {
        self.camera().is_none()
            && self.taken.is_none()
            && self.aperture.is_none()
            && self.shutter.is_none()
            && self.iso.is_none()
            && self.gps.is_none()
    }

    /// "MAKE MODEL", without repeating the maker when the model already starts with it.
    pub fn camera(&self) -> Option<String> {
        match (&self.make, &self.model) {
            (Some(make), Some(model)) => {
                let first = make.split_whitespace().next().unwrap_or(make);
                if model.to_lowercase().starts_with(&first.to_lowercase()) {
                    Some(model.clone())
                } else {
                    Some(format!("{make} {model}"))
                }
            }
            (None, Some(m)) | (Some(m), None) => Some(m.clone()),
            (None, None) => None,
        }
    }

    /// One block of text for "copy EXIF to clipboard".
    pub fn summary(&self) -> String {
        let gps = self.gps.map(|(lat, lon)| format!("{lat:.6}, {lon:.6}"));
        let rows = [
            ("Camera", self.camera()),
            ("Lens", self.lens.clone()),
            ("Taken", self.taken.clone()),
            ("Aperture", self.aperture.clone()),
            ("Shutter", self.shutter.clone()),
            ("ISO", self.iso.clone()),
            ("Focal length", self.focal.clone()),
            ("35mm equivalent", self.focal_35.clone()),
            ("Exposure bias", self.bias.clone()),
            ("Flash", self.flash.clone()),
            ("GPS", gps),
            ("Altitude", self.altitude.clone()),
        ];
        rows.iter()
            .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}: {v}")))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn read(bytes: &[u8]) -> Option<ExifInfo> {
    let exif = exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()?;
    Some(info(&exif))
}

/// From a bare TIFF block (as JPEG XL and HEIF files store Exif).
pub fn read_tiff(tiff: Vec<u8>) -> Option<ExifInfo> {
    let exif = exif::Reader::new().read_raw(tiff).ok()?;
    Some(info(&exif))
}

fn info(exif: &exif::Exif) -> ExifInfo {
    let ascii = |tag| -> Option<String> {
        let f = exif.get_field(tag, In::PRIMARY)?;
        match &f.value {
            Value::Ascii(v) => {
                let s = v
                    .iter()
                    .map(|b| String::from_utf8_lossy(b).trim().to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                let s = s.trim().trim_matches('\0').to_string();
                (!s.is_empty()).then_some(s)
            }
            _ => None,
        }
    };
    let rational = |tag| -> Option<f64> {
        match &exif.get_field(tag, In::PRIMARY)?.value {
            Value::Rational(v) => v.first().map(|r| r.to_f64()),
            Value::SRational(v) => v.first().map(|r| r.to_f64()),
            _ => None,
        }
    };
    let uint = |tag| exif.get_field(tag, In::PRIMARY)?.value.get_uint(0);

    let aperture = rational(Tag::FNumber)
        .filter(|f| f.is_finite() && *f > 0.0)
        .map(|f| format!("f/{}", trim_float(f, 1)));
    let shutter = rational(Tag::ExposureTime)
        .filter(|t| t.is_finite() && *t > 0.0)
        .map(format_shutter);
    let iso = uint(Tag::PhotographicSensitivity).map(|v| v.to_string());
    let focal = rational(Tag::FocalLength)
        .filter(|f| *f > 0.0)
        .map(|f| format!("{}mm", trim_float(f, 1)));
    let focal_35 = uint(Tag::FocalLengthIn35mmFilm)
        .filter(|v| *v > 0)
        .map(|v| format!("{v}mm"));
    let bias = rational(Tag::ExposureBiasValue)
        .filter(|b| b.is_finite())
        .map(|b| {
            if b.abs() < 0.005 {
                "0 EV".to_string()
            } else {
                format!("{:+.1} EV", b)
            }
        });
    let flash = uint(Tag::Flash).map(|v| if v & 1 == 1 { "FIRED" } else { "OFF" }.to_string());

    let taken = ascii(Tag::DateTimeOriginal)
        .or_else(|| ascii(Tag::DateTime))
        .map(|s| {
            // "2026:06:30 12:23:01" -> "2026-06-30 12:23:01"
            let mut s = s;
            if s.len() >= 10 && s.as_bytes()[4] == b':' && s.as_bytes()[7] == b':' {
                s.replace_range(4..5, "-");
                s.replace_range(7..8, "-");
            }
            s
        });

    let gps = gps_coord(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, 'S').zip(gps_coord(
        exif,
        Tag::GPSLongitude,
        Tag::GPSLongitudeRef,
        'W',
    ));
    let altitude = rational(Tag::GPSAltitude).map(|a| {
        let below = uint(Tag::GPSAltitudeRef) == Some(1);
        format!("{}{:.0}m", if below { "-" } else { "" }, a)
    });

    ExifInfo {
        make: ascii(Tag::Make),
        model: ascii(Tag::Model),
        lens: ascii(Tag::LensModel),
        taken,
        aperture,
        shutter,
        iso,
        focal,
        focal_35,
        bias,
        flash,
        gps,
        altitude,
        orientation: uint(Tag::Orientation).unwrap_or(1),
    }
}

fn gps_coord(exif: &exif::Exif, tag: Tag, ref_tag: Tag, negative: char) -> Option<f64> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let Value::Rational(v) = &field.value else {
        return None;
    };
    if v.len() < 3 {
        return None;
    }
    let deg = v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
    if !deg.is_finite() {
        return None;
    }
    let sign = match exif.get_field(ref_tag, In::PRIMARY).map(|f| &f.value) {
        Some(Value::Ascii(r)) if r.first().and_then(|b| b.first()) == Some(&(negative as u8)) => {
            -1.0
        }
        _ => 1.0,
    };
    Some(sign * deg)
}

pub fn format_shutter(t: f64) -> String {
    if t >= 1.0 {
        format!("{}s", trim_float(t, 1))
    } else {
        format!("1/{}", (1.0 / t).round() as u64)
    }
}

pub fn trim_float(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutter_speeds() {
        assert_eq!(format_shutter(1.0 / 125.0), "1/125");
        assert_eq!(format_shutter(0.5), "1/2");
        assert_eq!(format_shutter(2.0), "2s");
        assert_eq!(format_shutter(1.5), "1.5s");
    }

    #[test]
    fn trims_floats() {
        assert_eq!(trim_float(1.80, 1), "1.8");
        assert_eq!(trim_float(4.0, 1), "4");
        assert_eq!(trim_float(26.0, 1), "26");
    }

    #[test]
    fn camera_name_is_not_repeated() {
        let e = ExifInfo {
            make: Some("Apple".into()),
            model: Some("Apple iPhone 15".into()),
            ..Default::default()
        };
        assert_eq!(e.camera().as_deref(), Some("Apple iPhone 15"));
        let e = ExifInfo {
            make: Some("SONY".into()),
            model: Some("ILCE-7M4".into()),
            ..Default::default()
        };
        assert_eq!(e.camera().as_deref(), Some("SONY ILCE-7M4"));
    }

    #[test]
    fn garbage_is_none() {
        assert!(read(b"not an image").is_none());
    }
}
