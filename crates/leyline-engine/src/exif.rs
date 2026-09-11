//! EXIF of the files LibRaw does not read (ADR 0056).
//!
//! A JPEG, a TIFF or a PNG carries the same facts a RAW does — body, lens,
//! exposure, position, and above all the moment the shutter opened — and
//! until this module existed the catalog stored none of them, so those
//! photos sorted together at the end of every by-date view.
//!
//! The decision that shapes the module is one of precedence: **LibRaw comes
//! first** (ADR 0056 §1). The reader below runs only on the files LibRaw
//! could not identify, so the decoder that renders a photo stays the one
//! that names it. What it produces is the same [`Metadata`] row the RAW path
//! produces — the source depends on the file, the destination never does.
//!
//! Best-effort throughout, like the XMP sidecar next to it (ADR 0047, ADR
//! 0056 §5): a missing, truncated or nonsensical EXIF block yields `None`
//! and the photo still imports. Nothing here can fail an import, and nothing
//! here writes to the source file — reading is the only direction (§6).

use std::path::Path;

// `::exif` is the crate `kamadak-exif` publishes; the leading `::` tells it
// apart from this module, which shares its name.
use ::exif::{DateTime, Exif, In, Tag, Value};
use leyline_catalog::{CameraInfo, LensInfo, Metadata, Rational};

/// What one file's EXIF block says, split by destination.
///
/// The two halves land in different rows: [`ExifFacts::metadata`] in the
/// `metadata` table, the capture instant in the asset itself — which is why
/// the reader has to run before the asset is created.
pub(crate) struct ExifFacts {
    /// The metadata row to record, or `None` when the file said nothing this
    /// catalog stores. An empty row is not written: absent stays absent
    /// (ADR 0056 §3).
    pub metadata: Option<Metadata>,
    /// Capture instant in milliseconds since the Unix epoch.
    pub capture_date: Option<i64>,
    /// Time-zone offset in minutes, known only when the file states one
    /// (ADR 0056 §4).
    pub capture_offset_minutes: Option<i32>,
}

/// Reads the EXIF block of `path`, or `None` when there is nothing usable.
///
/// `None` covers every failure alike — an unreadable file, a container
/// without EXIF, a malformed block, a well-formed one carrying none of the
/// fields we store. None of them is the caller's problem (ADR 0056 §5).
pub(crate) fn read_exif(path: &Path) -> Option<ExifFacts> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let exif = ::exif::Reader::new()
        .read_from_container(&mut reader)
        .ok()?;
    facts(&exif)
}

/// Maps a parsed EXIF block onto the catalog's fields.
///
/// `None` when it yields nothing at all, so that an empty metadata row is
/// never written for a file that had nothing to say.
fn facts(exif: &Exif) -> Option<ExifFacts> {
    let fix = gps_fix(exif);
    let metadata = Metadata {
        camera: camera(exif),
        lens: lens(exif),
        orientation: uint(exif, Tag::Orientation)
            .map(|value| value as u16)
            .filter(|value| (1..=8).contains(value)),
        // `PhotographicSensitivity` is the Exif 2.3 name of the tag older
        // bodies wrote as `ISOSpeedRatings`; same tag, same number.
        iso: uint(exif, Tag::PhotographicSensitivity).or_else(|| uint(exif, Tag::ISOSpeed)),
        // Unlike LibRaw, which reports decimals the RAW path has to turn
        // back into rationals, EXIF gives the exact rationals the catalog
        // stores (`docs/catalog.md` §13): they pass through untouched.
        shutter: rational(exif, Tag::ExposureTime),
        aperture: rational(exif, Tag::FNumber),
        focal_length: rational(exif, Tag::FocalLength),
        exposure_bias: srational(exif, Tag::ExposureBiasValue),
        // Bit 0 of `Flash` is the only one that answers "did it fire"; the
        // rest describe the mode and the return detection.
        flash: uint(exif, Tag::Flash).map(|value| value & 1 == 1),
        white_balance_mode: uint(exif, Tag::WhiteBalance)
            .filter(|value| *value <= 1)
            .map(|value| value as u16),
        color_space: color_space(exif),
        gps_latitude: fix.map(|(latitude, _)| latitude),
        gps_longitude: fix.map(|(_, longitude)| longitude),
        gps_altitude: altitude(exif),
        artist: text(exif, Tag::Artist),
        copyright: text(exif, Tag::Copyright),
    };
    let (capture_date, capture_offset_minutes) = capture_instant(exif);

    let empty = metadata == Metadata::default();
    (!empty || capture_date.is_some()).then_some(ExifFacts {
        metadata: (!empty).then_some(metadata),
        capture_date,
        capture_offset_minutes,
    })
}

/// The body, when the file names one.
fn camera(exif: &Exif) -> Option<CameraInfo> {
    let manufacturer = text(exif, Tag::Make);
    let model = text(exif, Tag::Model);
    match (manufacturer, model) {
        (None, None) => None,
        (manufacturer, model) => {
            let manufacturer = manufacturer.unwrap_or_default();
            Some(CameraInfo {
                model: without_brand(&model.unwrap_or_default(), &manufacturer),
                manufacturer,
            })
        }
    }
}

/// Drops a leading brand from a model name: Canon writes `Model` as
/// `"Canon EOS 5D Mark IV"` next to a `Make` of `"Canon"`, and a panel that
/// shows the two joined then reads "Canon Canon EOS 5D Mark IV".
///
/// This is normalisation, not cleverness: LibRaw already hands the RAW path a
/// brandless model, so doing it here is what makes the same body look the
/// same whether its RAW or its JPEG was imported. A model that merely starts
/// with the same letters (`"Canonet"` under a `Make` of `"Canon"`) is left
/// alone — the brand has to be followed by a separator to count.
fn without_brand(model: &str, manufacturer: &str) -> String {
    if manufacturer.is_empty() {
        return model.to_owned();
    }
    model
        .strip_prefix(manufacturer)
        .and_then(|rest| rest.strip_prefix([' ', '-', '_']))
        .map_or_else(|| model.to_owned(), str::to_owned)
}

/// The lens, when the file names one. Compacts and older bodies report only
/// a model, which is still worth recording — the same rule the RAW path
/// applies.
fn lens(exif: &Exif) -> Option<LensInfo> {
    let manufacturer = text(exif, Tag::LensMake);
    let model = text(exif, Tag::LensModel);
    match (manufacturer, model) {
        (None, None) => None,
        (manufacturer, model) => Some(LensInfo {
            manufacturer: manufacturer.unwrap_or_default(),
            model: model.unwrap_or_default(),
            mount: None,
        }),
    }
}

/// The `ColorSpace` tag, named rather than numbered.
///
/// `1` and `0xFFFF` are the two values Exif 2.3 defines. `2` is not one of
/// them, but enough cameras write it for Adobe RGB that reading it is
/// reporting what the file says, not guessing. Any other value is left out
/// rather than invented (ADR 0056 §3).
fn color_space(exif: &Exif) -> Option<String> {
    match uint(exif, Tag::ColorSpace)? {
        1 => Some("sRGB".to_owned()),
        2 => Some("Adobe RGB".to_owned()),
        0xFFFF => Some("Uncalibrated".to_owned()),
        _ => None,
    }
}

/// The capture instant in epoch milliseconds, and the time-zone offset in
/// minutes when the file states one (ADR 0056 §4).
///
/// Each date tag is paired with *its own* offset tag, as EXIF 2.31 defines
/// them: a `DateTimeDigitized` fallback must not borrow the offset that
/// belonged to a `DateTimeOriginal` the file does not have.
fn capture_instant(exif: &Exif) -> (Option<i64>, Option<i32>) {
    const CANDIDATES: [(Tag, Tag); 3] = [
        (Tag::DateTimeOriginal, Tag::OffsetTimeOriginal),
        (Tag::DateTimeDigitized, Tag::OffsetTimeDigitized),
        (Tag::DateTime, Tag::OffsetTime),
    ];
    for (date_tag, offset_tag) in CANDIDATES {
        let Some(mut when) = ascii(exif, date_tag).and_then(|raw| DateTime::from_ascii(raw).ok())
        else {
            continue;
        };
        // Without an offset the timestamp stays as written and the zone
        // stays unknown, exactly like the RAW path: a local time recorded
        // as such is honest, a local time shifted by a guessed zone is not.
        if let Some(offset) = ascii(exif, offset_tag) {
            let _ = when.parse_offset(offset);
        }
        if let Some(instant) = epoch_ms(&when) {
            return (Some(instant), when.offset.map(i32::from));
        }
    }
    (None, None)
}

/// An EXIF date as milliseconds since the Unix epoch, `None` when its fields
/// are not a real date.
///
/// `DateTime::from_ascii` deliberately range-checks nothing, and cameras do
/// write `0000:00:00 00:00:00` into files: the guard below is what keeps
/// that from becoming a capture date in year zero.
fn epoch_ms(when: &DateTime) -> Option<i64> {
    if !(1..=12).contains(&when.month)
        || !(1..=31).contains(&when.day)
        || when.hour > 23
        || when.minute > 59
        // 60 is a leap second, which rolls into the next minute here.
        || when.second > 60
    {
        return None;
    }
    let days = days_from_civil(i64::from(when.year), when.month, when.day);
    let seconds = days * 86_400
        + i64::from(when.hour) * 3600
        + i64::from(when.minute) * 60
        + i64::from(when.second);
    // The offset says how far ahead of UTC the written time is, so it comes
    // back off to reach the instant.
    let offset = i64::from(when.offset.unwrap_or(0)) * 60;
    Some((seconds - offset) * 1000)
}

/// Days since 1970-01-01 for a Gregorian date (Howard Hinnant's algorithm,
/// the inverse of Studio's `civil_from_days`). Written out rather than
/// pulled from a date crate: this is the whole of what we need from one.
fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    // March-based years put the leap day last, which is what makes the rest
    // of the arithmetic branch-free.
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 {
        i64::from(month) - 3
    } else {
        i64::from(month) + 9
    };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// A GPS coordinate as signed decimal degrees, the convention the catalog
/// and the RAW path share.
///
/// EXIF splits it in two: three rationals (degrees, minutes, seconds) and a
/// hemisphere letter. Without the letter the sign is unknown, so the
/// coordinate is dropped rather than assumed northern or eastern.
fn coordinate(exif: &Exif, value_tag: Tag, ref_tag: Tag, limit: f64) -> Option<f64> {
    let field = exif.get_field(value_tag, In::PRIMARY)?;
    let Value::Rational(parts) = &field.value else {
        return None;
    };
    let [degrees, minutes, seconds] = parts.get(..3)? else {
        return None;
    };
    let magnitude = degrees.to_f64() + minutes.to_f64() / 60.0 + seconds.to_f64() / 3600.0;
    let negative = match text(exif, ref_tag)?.to_ascii_uppercase().as_str() {
        "N" | "E" => false,
        "S" | "W" => true,
        _ => return None,
    };
    let value = if negative { -magnitude } else { magnitude };
    // An out-of-range coordinate is a broken file, and the catalog rejects
    // it outright; dropping it here keeps one bad tag from costing the photo
    // its whole metadata row.
    (value.is_finite() && value.abs() <= limit).then_some(value)
}

/// The position the file claims, or `None` when it claims none.
///
/// The rule itself is `leyline_catalog::gps_fix` (ADR 0150 §1), shared with
/// the RAW path: zero and zero is a body with no receiver, not the Gulf of
/// Guinea.
fn gps_fix(exif: &Exif) -> Option<(f64, f64)> {
    leyline_catalog::gps_fix(
        coordinate(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, 90.0),
        coordinate(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, 180.0),
    )
}

/// GPS altitude in meters, negative below sea level.
///
/// `GPSAltitude` is a magnitude and `GPSAltitudeRef` the side of the sea it
/// sits on — a below-grade location imports above sea level if the reference
/// is ignored, which is the trap the RAW path documents too.
fn altitude(exif: &Exif) -> Option<f64> {
    let field = exif.get_field(Tag::GPSAltitude, In::PRIMARY)?;
    let Value::Rational(parts) = &field.value else {
        return None;
    };
    let magnitude = parts.first()?.to_f64();
    if !magnitude.is_finite() {
        return None;
    }
    let below_sea_level = uint(exif, Tag::GPSAltitudeRef) == Some(1);
    Some(if below_sea_level {
        -magnitude
    } else {
        magnitude
    })
}

/// The first ASCII value of a tag, raw bytes and all.
fn ascii(exif: &Exif, tag: Tag) -> Option<&[u8]> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Ascii(values) => values.first().map(Vec::as_slice),
        _ => None,
    }
}

/// The first ASCII value of a tag as trimmed text, `None` when it is empty.
///
/// EXIF strings are padded with spaces and NULs, and bodies that have
/// nothing to say often write the padding alone.
fn text(exif: &Exif, tag: Tag) -> Option<String> {
    let value = String::from_utf8_lossy(ascii(exif, tag)?);
    let trimmed = value.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The first value of a tag as an unsigned integer, whatever integer type it
/// was written in.
fn uint(exif: &Exif, tag: Tag) -> Option<u32> {
    exif.get_field(tag, In::PRIMARY)?.value.get_uint(0)
}

/// The first value of a tag as an exact rational, dropped when its
/// denominator is zero — which the catalog refuses, and which divides to
/// infinity anyway.
fn rational(exif: &Exif, tag: Tag) -> Option<Rational> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Rational(values) => {
            let value = values.first()?;
            (value.denom > 0).then(|| Rational {
                numerator: i64::from(value.num),
                denominator: i64::from(value.denom),
            })
        }
        _ => None,
    }
}

/// The first value of a tag as a decimal, from a signed rational.
fn srational(exif: &Exif, tag: Tag) -> Option<f64> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::SRational(values) => {
            let value = values.first()?;
            (value.denom != 0).then(|| value.to_f64())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag value in the encodings the fixtures below need.
    enum TestValue {
        Ascii(&'static str),
        Short(u16),
        Byte(u8),
        Rational(&'static [(u32, u32)]),
        SRational(i32, i32),
    }

    impl TestValue {
        /// `(type code, component count)` as the IFD entry spells them.
        fn header(&self) -> (u16, u32) {
            match self {
                TestValue::Ascii(text) => (2, text.len() as u32 + 1),
                TestValue::Short(_) => (3, 1),
                TestValue::Byte(_) => (1, 1),
                TestValue::Rational(parts) => (5, parts.len() as u32),
                TestValue::SRational(..) => (10, 1),
            }
        }

        /// The value bytes, little-endian, before the inline/offset choice.
        fn bytes(&self) -> Vec<u8> {
            match self {
                TestValue::Ascii(text) => {
                    let mut bytes = text.as_bytes().to_vec();
                    bytes.push(0);
                    bytes
                }
                TestValue::Short(value) => value.to_le_bytes().to_vec(),
                TestValue::Byte(value) => vec![*value],
                TestValue::Rational(parts) => parts
                    .iter()
                    .flat_map(|(num, denom)| {
                        let mut bytes = num.to_le_bytes().to_vec();
                        bytes.extend(denom.to_le_bytes());
                        bytes
                    })
                    .collect(),
                TestValue::SRational(num, denom) => {
                    let mut bytes = num.to_le_bytes().to_vec();
                    bytes.extend(denom.to_le_bytes());
                    bytes
                }
            }
        }
    }

    /// Builds a little-endian TIFF block: IFD0, an Exif sub-IFD and a GPS
    /// IFD, each linked from IFD0 by its pointer tag.
    ///
    /// Hand-assembled rather than taken from a real photo, so that every
    /// expected value sits next to the assertion that reads it and the tests
    /// need no fixture file.
    fn tiff(
        ifd0: &[(u16, TestValue)],
        sub: &[(u16, TestValue)],
        gps: &[(u16, TestValue)],
    ) -> Vec<u8> {
        let ifd_size = |entries: usize| {
            if entries == 0 {
                0
            } else {
                2 + 12 * entries + 4
            }
        };
        let mut pointers = Vec::new();
        let ifd0_offset = 8usize;
        let ifd0_entries = ifd0.len() + usize::from(!sub.is_empty()) + usize::from(!gps.is_empty());
        let sub_offset = ifd0_offset + ifd_size(ifd0_entries);
        let gps_offset = sub_offset + ifd_size(sub.len());
        // Values wider than four bytes live past the last IFD.
        let data_offset = gps_offset + ifd_size(gps.len());
        if !sub.is_empty() {
            pointers.push((0x8769u16, sub_offset as u32));
        }
        if !gps.is_empty() {
            pointers.push((0x8825u16, gps_offset as u32));
        }

        let mut data = Vec::new();
        let mut ifd = |entries: &[(u16, TestValue)], extra: &[(u16, u32)]| -> Vec<u8> {
            let count = entries.len() + extra.len();
            if count == 0 {
                return Vec::new();
            }
            let mut out = (count as u16).to_le_bytes().to_vec();
            for (tag, value) in entries {
                let (kind, components) = value.header();
                out.extend(tag.to_le_bytes());
                out.extend(kind.to_le_bytes());
                out.extend(components.to_le_bytes());
                let mut bytes = value.bytes();
                if bytes.len() <= 4 {
                    bytes.resize(4, 0);
                    out.extend(bytes);
                } else {
                    out.extend(((data_offset + data.len()) as u32).to_le_bytes());
                    data.extend(bytes);
                }
            }
            for (tag, offset) in extra {
                out.extend(tag.to_le_bytes());
                out.extend(4u16.to_le_bytes());
                out.extend(1u32.to_le_bytes());
                out.extend(offset.to_le_bytes());
            }
            out.extend(0u32.to_le_bytes());
            out
        };

        let ifd0_bytes = ifd(ifd0, &pointers);
        let sub_bytes = ifd(sub, &[]);
        let gps_bytes = ifd(gps, &[]);

        let mut out = b"II".to_vec();
        out.extend(42u16.to_le_bytes());
        out.extend((ifd0_offset as u32).to_le_bytes());
        out.extend(ifd0_bytes);
        out.extend(sub_bytes);
        out.extend(gps_bytes);
        out.extend(data);
        out
    }

    /// The block above, parsed back through the real reader.
    fn read(ifd0: &[(u16, TestValue)], sub: &[(u16, TestValue)], gps: &[(u16, TestValue)]) -> Exif {
        ::exif::Reader::new()
            .read_raw(tiff(ifd0, sub, gps))
            .expect("the hand-built TIFF block parses")
    }

    #[test]
    fn a_date_without_offset_is_read_as_written() {
        // No OffsetTimeOriginal: the time is taken as-is (UTC convention),
        // and the zone stays unknown (ADR 0056 §4).
        let exif = read(
            &[],
            &[(0x9003, TestValue::Ascii("2024:03:17 14:05:09"))],
            &[],
        );
        let (instant, offset) = capture_instant(&exif);
        assert_eq!(instant, Some(1_710_684_309_000));
        assert_eq!(offset, None);
    }

    #[test]
    fn an_offset_moves_the_instant_and_is_kept() {
        // +02:00 means the written wall clock is two hours ahead of UTC, so
        // the instant is two hours earlier than the naive reading.
        let exif = read(
            &[],
            &[
                (0x9003, TestValue::Ascii("2024:03:17 14:05:09")),
                (0x9011, TestValue::Ascii("+02:00")),
            ],
            &[],
        );
        let (instant, offset) = capture_instant(&exif);
        assert_eq!(instant, Some(1_710_684_309_000 - 2 * 3600 * 1000));
        assert_eq!(offset, Some(120));
    }

    #[test]
    fn a_negative_offset_moves_the_instant_forward() {
        let exif = read(
            &[],
            &[
                (0x9003, TestValue::Ascii("2024:03:17 14:05:09")),
                (0x9011, TestValue::Ascii("-05:30")),
            ],
            &[],
        );
        let (instant, offset) = capture_instant(&exif);
        assert_eq!(instant, Some(1_710_684_309_000 + (5 * 60 + 30) * 60 * 1000));
        assert_eq!(offset, Some(-330));
    }

    #[test]
    fn the_epoch_itself_and_a_date_before_it_convert() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        // A leap day, the case the March-based year exists for.
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
    }

    #[test]
    fn the_fallback_chain_prefers_the_original_then_the_digitized() {
        let both = read(
            &[(0x0132, TestValue::Ascii("2001:01:01 00:00:00"))],
            &[
                (0x9003, TestValue::Ascii("2024:03:17 14:05:09")),
                (0x9004, TestValue::Ascii("2010:01:01 00:00:00")),
            ],
            &[],
        );
        assert_eq!(capture_instant(&both).0, Some(1_710_684_309_000));

        let digitized = read(
            &[(0x0132, TestValue::Ascii("2001:01:01 00:00:00"))],
            &[(0x9004, TestValue::Ascii("2010:01:01 00:00:00"))],
            &[],
        );
        assert_eq!(capture_instant(&digitized).0, Some(1_262_304_000_000));

        // Only DateTime left: the file modification date of the EXIF world.
        let modified = read(
            &[(0x0132, TestValue::Ascii("2001:01:01 00:00:00"))],
            &[],
            &[],
        );
        assert_eq!(capture_instant(&modified).0, Some(978_307_200_000));
    }

    #[test]
    fn an_offset_never_leaks_onto_the_date_it_does_not_belong_to() {
        // OffsetTimeOriginal with no DateTimeOriginal: the digitized
        // fallback must not adopt it.
        let exif = read(
            &[],
            &[
                (0x9004, TestValue::Ascii("2010:01:01 00:00:00")),
                (0x9011, TestValue::Ascii("+09:00")),
            ],
            &[],
        );
        let (instant, offset) = capture_instant(&exif);
        assert_eq!(instant, Some(1_262_304_000_000));
        assert_eq!(offset, None);
    }

    #[test]
    fn an_impossible_date_yields_nothing() {
        // "0000:00:00 00:00:00" is what cameras write when they have no
        // clock set, and it must not become a year-zero capture date.
        let exif = read(
            &[],
            &[(0x9003, TestValue::Ascii("0000:00:00 00:00:00"))],
            &[],
        );
        assert_eq!(capture_instant(&exif), (None, None));
    }

    #[test]
    fn a_southern_western_coordinate_becomes_signed_degrees() {
        // Sydney: 33°51'54"S, 151°12'32"E.
        let exif = read(
            &[],
            &[],
            &[
                (0x0001, TestValue::Ascii("S")),
                (0x0002, TestValue::Rational(&[(33, 1), (51, 1), (54, 1)])),
                (0x0003, TestValue::Ascii("W")),
                (0x0004, TestValue::Rational(&[(151, 1), (12, 1), (32, 1)])),
            ],
        );
        let latitude = coordinate(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, 90.0);
        let longitude = coordinate(&exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, 180.0);
        assert!((latitude.expect("latitude") + 33.865).abs() < 1e-9);
        assert!((longitude.expect("longitude") + 151.208_888_888_9).abs() < 1e-9);
    }

    #[test]
    fn a_coordinate_without_its_hemisphere_is_dropped() {
        let exif = read(
            &[],
            &[],
            &[(0x0002, TestValue::Rational(&[(33, 1), (51, 1), (54, 1)]))],
        );
        assert_eq!(
            coordinate(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, 90.0),
            None
        );
    }

    #[test]
    fn an_out_of_range_coordinate_is_dropped() {
        let exif = read(
            &[],
            &[],
            &[
                (0x0001, TestValue::Ascii("N")),
                (0x0002, TestValue::Rational(&[(133, 1), (0, 1), (0, 1)])),
            ],
        );
        assert_eq!(
            coordinate(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, 90.0),
            None
        );
    }

    /// ADR 0150 §1: a body with no receiver writes a zeroed GPS block, and
    /// read literally that is a fix in the Gulf of Guinea. 2 997 rows of the
    /// reference library sat there.
    #[test]
    fn a_zeroed_gps_block_is_no_position_at_all() {
        let exif = read(
            &[],
            &[],
            &[
                (0x0001, TestValue::Ascii("N")),
                (0x0002, TestValue::Rational(&[(0, 1), (0, 1), (0, 1)])),
                (0x0003, TestValue::Ascii("E")),
                (0x0004, TestValue::Rational(&[(0, 1), (0, 1), (0, 1)])),
            ],
        );
        assert_eq!(gps_fix(&exif), None);
    }

    /// And only the pair: a photograph on the equator keeps its position,
    /// which is the distinction the measurement supports — not one of those
    /// 2 997 rows zeroed a single coordinate.
    #[test]
    fn a_photograph_on_the_equator_keeps_its_position() {
        let exif = read(
            &[],
            &[],
            &[
                (0x0001, TestValue::Ascii("N")),
                (0x0002, TestValue::Rational(&[(0, 1), (0, 1), (0, 1)])),
                (0x0003, TestValue::Ascii("E")),
                (0x0004, TestValue::Rational(&[(32, 1), (30, 1), (0, 1)])),
            ],
        );
        let (latitude, longitude) = gps_fix(&exif).expect("a fix on the equator");
        assert_eq!(latitude, 0.0);
        assert!((longitude - 32.5).abs() < 1e-9);
    }

    #[test]
    fn an_altitude_below_sea_level_is_negative() {
        let above = read(&[], &[], &[(0x0006, TestValue::Rational(&[(120, 1)]))]);
        assert_eq!(altitude(&above), Some(120.0));

        let below = read(
            &[],
            &[],
            &[
                (0x0005, TestValue::Byte(1)),
                (0x0006, TestValue::Rational(&[(86, 1)])),
            ],
        );
        assert_eq!(altitude(&below), Some(-86.0));
    }

    #[test]
    fn a_model_repeating_its_brand_is_stored_without_it() {
        assert_eq!(
            without_brand("Canon EOS 5D Mark IV", "Canon"),
            "EOS 5D Mark IV"
        );
        assert_eq!(
            without_brand("NIKON D850", "NIKON CORPORATION"),
            "NIKON D850"
        );
        assert_eq!(without_brand("X-T4", "FUJIFILM"), "X-T4");
        // A model that only begins with the same letters keeps them: the
        // brand has to be followed by a separator.
        assert_eq!(without_brand("Canonet QL17", "Canon"), "Canonet QL17");
        assert_eq!(without_brand("EOS R5", ""), "EOS R5");
    }

    #[test]
    fn a_full_block_fills_the_metadata_row() {
        let exif = read(
            &[
                (0x010F, TestValue::Ascii("Canon")),
                (0x0110, TestValue::Ascii("Canon EOS 5D Mark IV")),
                (0x0112, TestValue::Short(6)),
                (0x013B, TestValue::Ascii("Ansel")),
                (0x8298, TestValue::Ascii("(c) Ansel")),
            ],
            &[
                (0x829A, TestValue::Rational(&[(1, 320)])),
                (0x829D, TestValue::Rational(&[(56, 10)])),
                (0x8827, TestValue::Short(800)),
                (0x9003, TestValue::Ascii("2024:03:17 14:05:09")),
                (0x9204, TestValue::SRational(-1, 3)),
                (0x9209, TestValue::Short(0x0009)),
                (0x920A, TestValue::Rational(&[(70, 1)])),
                (0xA001, TestValue::Short(1)),
                (0xA403, TestValue::Short(1)),
                (0xA433, TestValue::Ascii("Canon")),
                (0xA434, TestValue::Ascii("EF 24-70mm f/2.8L II USM")),
            ],
            &[],
        );
        let facts = facts(&exif).expect("the block says plenty");
        let metadata = facts.metadata.expect("a metadata row");

        assert_eq!(
            metadata.camera,
            Some(CameraInfo {
                manufacturer: "Canon".to_owned(),
                // The brand the file repeats in `Model` is dropped, so this
                // body reads the same as it does through the RAW path.
                model: "EOS 5D Mark IV".to_owned(),
            })
        );
        assert_eq!(
            metadata.lens,
            Some(LensInfo {
                manufacturer: "Canon".to_owned(),
                model: "EF 24-70mm f/2.8L II USM".to_owned(),
                mount: None,
            })
        );
        assert_eq!(metadata.orientation, Some(6));
        assert_eq!(metadata.iso, Some(800));
        // The rationals are the file's own, not a decimal round trip.
        assert_eq!(
            metadata.shutter,
            Some(Rational {
                numerator: 1,
                denominator: 320
            })
        );
        assert_eq!(
            metadata.aperture,
            Some(Rational {
                numerator: 56,
                denominator: 10
            })
        );
        assert_eq!(
            metadata.focal_length,
            Some(Rational {
                numerator: 70,
                denominator: 1
            })
        );
        assert_eq!(metadata.exposure_bias, Some(-1.0 / 3.0));
        assert_eq!(metadata.flash, Some(true));
        assert_eq!(metadata.white_balance_mode, Some(1));
        assert_eq!(metadata.color_space, Some("sRGB".to_owned()));
        assert_eq!(metadata.artist, Some("Ansel".to_owned()));
        assert_eq!(metadata.copyright, Some("(c) Ansel".to_owned()));
        assert_eq!(facts.capture_date, Some(1_710_684_309_000));
        assert_eq!(facts.capture_offset_minutes, None);
    }

    #[test]
    fn absent_tags_stay_absent() {
        // One tag we do not store, and nothing else: no invented values, no
        // metadata row at all (ADR 0056 §3).
        let exif = read(&[(0x011A, TestValue::Rational(&[(72, 1)]))], &[], &[]);
        assert!(facts(&exif).is_none());
    }

    #[test]
    fn a_date_alone_writes_no_metadata_row() {
        let exif = read(
            &[],
            &[(0x9003, TestValue::Ascii("2024:03:17 14:05:09"))],
            &[],
        );
        let facts = facts(&exif).expect("a date is enough to matter");
        assert!(facts.metadata.is_none());
        assert_eq!(facts.capture_date, Some(1_710_684_309_000));
    }

    #[test]
    fn a_file_without_exif_reads_as_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("plain.jpg");
        std::fs::write(&path, b"not a jpeg at all").expect("write");
        assert!(read_exif(&path).is_none());
        assert!(read_exif(&dir.path().join("missing.jpg")).is_none());
    }

    #[test]
    fn a_jpeg_on_disk_reads_end_to_end() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("shot.jpg");
        std::fs::write(
            &path,
            jpeg_with_exif(&tiff(
                &[(0x010F, TestValue::Ascii("Nikon"))],
                &[(0x9003, TestValue::Ascii("2024:03:17 14:05:09"))],
                &[],
            )),
        )
        .expect("write");

        let facts = read_exif(&path).expect("the JPEG carries EXIF");
        assert_eq!(facts.capture_date, Some(1_710_684_309_000));
        assert_eq!(
            facts
                .metadata
                .and_then(|m| m.camera)
                .map(|c| c.manufacturer),
            Some("Nikon".to_owned())
        );
    }

    /// Wraps a TIFF block in the smallest JPEG that can carry it: start of
    /// image, the `Exif` APP1 segment, end of image.
    fn jpeg_with_exif(tiff: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        let length = (tiff.len() + 8) as u16;
        out.extend(length.to_be_bytes());
        out.extend(b"Exif\0\0");
        out.extend(tiff);
        out.extend([0xFF, 0xD9]);
        out
    }
}
