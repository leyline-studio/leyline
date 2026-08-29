//! Turns catalog values into the strings Studio displays.
//!
//! Kept free of any Slint type so every rule here is unit-testable: the UI
//! shows these strings verbatim and never interprets catalog values itself.

use leyline_sdk::{Metadata, Rational};

/// The star rating as text: `★★★` for three stars, empty when unrated.
pub fn stars(rating: Option<u8>) -> String {
    "★".repeat(usize::from(rating.unwrap_or(0)))
}

/// A UTC capture instant as `YYYY-MM-DD HH:MM`.
pub fn capture_date(epoch_ms: i64) -> String {
    let secs = epoch_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        tod / 3600,
        tod % 3600 / 60
    )
}

/// Gregorian date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// A shutter speed: `1/3200 s` under a second, `2.5 s` from one second up.
pub fn shutter(value: Rational) -> String {
    let seconds = value.as_f64();
    if seconds >= 1.0 {
        format!("{} s", trim(seconds))
    } else if seconds > 0.0 {
        format!("1/{} s", (1.0 / seconds).round() as i64)
    } else {
        String::new()
    }
}

/// A body or a lens as one line: `Canon EOS 5D Mark IV`.
///
/// Either half can be missing — plenty of lenses report a model and no maker
/// — and joining them blindly then leaves a leading space that reads as a
/// bad character rather than as a missing brand.
pub fn maker_and_model(manufacturer: &str, model: &str) -> String {
    format!("{} {}", manufacturer.trim(), model.trim())
        .trim()
        .to_owned()
}

/// A sensitivity as `ISO 100`.
pub fn iso(value: u32) -> String {
    format!("ISO {value}")
}

/// An aperture as `f/5.6`.
pub fn aperture(value: Rational) -> String {
    format!("f/{}", trim(value.as_f64()))
}

/// A focal length as `70 mm`, rounded to the millimeter.
pub fn focal(value: Rational) -> String {
    format!("{} mm", value.as_f64().round() as i64)
}

/// A file size in the largest fitting decimal unit: `24.3 MB`.
pub fn file_size(bytes: u64) -> String {
    let bytes = bytes as f64;
    for (scale, unit) in [(1e9, "GB"), (1e6, "MB"), (1e3, "kB")] {
        if bytes >= scale {
            return format!("{:.1} {unit}", bytes / scale);
        }
    }
    format!("{bytes} B")
}

/// Pixel dimensions as `6000 × 4000`, or `—` while unknown.
pub fn dimensions(width: Option<u32>, height: Option<u32>) -> String {
    match (width, height) {
        (Some(w), Some(h)) => format!("{w} × {h}"),
        _ => "—".to_owned(),
    }
}

/// The one-line exposure summary: `ISO 100 · 1/3200 s · f/5.6 · 70 mm`.
///
/// Absent values are simply skipped; all absent gives an empty string.
pub fn exposure_line(meta: &Metadata) -> String {
    let mut parts = Vec::new();
    if let Some(value) = meta.iso {
        parts.push(iso(value));
    }
    if let Some(value) = meta.shutter {
        parts.push(shutter(value));
    }
    if let Some(value) = meta.aperture {
        parts.push(aperture(value));
    }
    if let Some(value) = meta.focal_length {
        parts.push(focal(value));
    }
    parts.join(" · ")
}

/// `1.8` for 1.8, `8` for 8.0 — at most one decimal, no trailing zero.
fn trim(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

/// What a root's location line says when there is no location to say.
///
/// A sentence rather than a dash: the row is the one place a user learns that
/// an unplugged volume costs them their originals and nothing else, and a
/// dash teaches nothing.
pub(crate) fn root_offline_location() -> String {
    "not plugged in — the photographs stay catalogued".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rational(numerator: i64, denominator: i64) -> Rational {
        Rational {
            numerator,
            denominator,
        }
    }

    #[test]
    fn formats_ratings_as_stars() {
        assert_eq!(stars(None), "");
        assert_eq!(stars(Some(0)), "");
        assert_eq!(stars(Some(3)), "★★★");
    }

    #[test]
    fn formats_capture_dates_in_utc() {
        assert_eq!(capture_date(0), "1970-01-01 00:00");
        // 2024-02-29 12:34:56 UTC — a leap day.
        assert_eq!(capture_date(1_709_210_096_000), "2024-02-29 12:34");
        // Before the epoch.
        assert_eq!(capture_date(-86_400_000), "1969-12-31 00:00");
    }

    #[test]
    fn formats_exposure_values() {
        assert_eq!(shutter(rational(1, 3200)), "1/3200 s");
        assert_eq!(shutter(rational(5, 2)), "2.5 s");
        assert_eq!(aperture(rational(56, 10)), "f/5.6");
        assert_eq!(aperture(rational(8, 1)), "f/8");
        assert_eq!(focal(rational(70, 1)), "70 mm");
        assert_eq!(iso(100), "ISO 100");
    }

    #[test]
    fn joins_a_maker_and_a_model_without_leaving_a_stray_space() {
        assert_eq!(
            maker_and_model("Canon", "EOS 5D Mark IV"),
            "Canon EOS 5D Mark IV"
        );
        assert_eq!(
            maker_and_model("", "EF70-200mm f/2.8L IS USM"),
            "EF70-200mm f/2.8L IS USM"
        );
        assert_eq!(maker_and_model("Leica", ""), "Leica");
        assert_eq!(maker_and_model("", ""), "");
    }

    #[test]
    fn formats_sizes_and_dimensions() {
        assert_eq!(file_size(512), "512 B");
        assert_eq!(file_size(24_300_000), "24.3 MB");
        assert_eq!(file_size(1_500_000_000), "1.5 GB");
        assert_eq!(dimensions(Some(6000), Some(4000)), "6000 × 4000");
        assert_eq!(dimensions(None, Some(4000)), "—");
    }

    #[test]
    fn joins_the_exposure_line_from_present_values() {
        let mut meta = Metadata::default();
        assert_eq!(exposure_line(&meta), "");
        meta.iso = Some(100);
        meta.shutter = Some(rational(1, 3200));
        meta.focal_length = Some(rational(70, 1));
        assert_eq!(exposure_line(&meta), "ISO 100 · 1/3200 s · 70 mm");
    }
}
