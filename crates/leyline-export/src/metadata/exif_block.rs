//! The EXIF block: a small little-endian TIFF structure of a dozen tags
//! (ADR 0157 §5), written here rather than by a metadata crate so its byte
//! order and its tag order are ours, and fixed.
//!
//! Layout: the 8-byte header, IFD 0, the Exif IFD, then the GPS IFD when
//! there is a position. Each IFD is followed by the values too long to sit
//! in its entries. Tags are sorted inside each IFD, as TIFF requires.

use super::{
    CaptureTime, GpsPosition, PhotoMetadata, Ratio, SOFTWARE, altitude_centimetres,
    bias_hundredths, usable, usable_position,
};

/// The prefix a JPEG APP1 segment puts before the TIFF structure; PNG's
/// `eXIf` chunk and WebP's `EXIF` chunk take the structure alone.
pub(crate) const JPEG_PREFIX: &[u8] = b"Exif\0\0";

/// One TIFF field value, by type.
enum Value {
    Byte(Vec<u8>),
    Ascii(String),
    Short(u16),
    Long(u32),
    Rational(Vec<Ratio>),
    SRational(i32, i32),
    Undefined(Vec<u8>),
}

impl Value {
    fn type_code(&self) -> u16 {
        match self {
            Value::Byte(_) => 1,
            Value::Ascii(_) => 2,
            Value::Short(_) => 3,
            Value::Long(_) => 4,
            Value::Rational(_) => 5,
            Value::Undefined(_) => 7,
            Value::SRational(..) => 10,
        }
    }

    /// The value's bytes and its TIFF count.
    fn encoded(&self) -> (Vec<u8>, u32) {
        match self {
            Value::Byte(bytes) | Value::Undefined(bytes) => (bytes.clone(), len_u32(bytes.len())),
            Value::Ascii(text) => {
                // A NUL inside an ASCII field would end it early for every
                // reader; it is dropped rather than truncating the rest.
                let mut bytes: Vec<u8> = text.bytes().filter(|b| *b != 0).collect();
                bytes.push(0);
                let count = len_u32(bytes.len());
                (bytes, count)
            }
            Value::Short(v) => (v.to_le_bytes().to_vec(), 1),
            Value::Long(v) => (v.to_le_bytes().to_vec(), 1),
            Value::Rational(ratios) => {
                let bytes = ratios
                    .iter()
                    .flat_map(|r| [r.numerator.to_le_bytes(), r.denominator.to_le_bytes()])
                    .flatten()
                    .collect();
                (bytes, len_u32(ratios.len()))
            }
            Value::SRational(n, d) => ([n.to_le_bytes(), d.to_le_bytes()].concat(), 1),
        }
    }
}

/// A length as a TIFF count. Saturates rather than wraps: a field that long
/// is refused later by the carrier's own size limit.
fn len_u32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The bytes one IFD takes, entries and out-of-line values together.
fn ifd_size(entries: &[(u16, Value)]) -> usize {
    let data: usize = entries
        .iter()
        .map(|(_, value)| value.encoded().0.len())
        .filter(|len| *len > 4)
        .map(|len| len + len % 2)
        .sum();
    2 + 12 * entries.len() + 4 + data
}

/// Appends one IFD at the end of `out`, which is where its offsets point.
fn write_ifd(out: &mut Vec<u8>, entries: &[(u16, Value)]) {
    let start = out.len();
    let mut data_offset = start + 2 + 12 * entries.len() + 4;
    let mut data = Vec::new();
    out.extend_from_slice(
        &u16::try_from(entries.len())
            .unwrap_or(u16::MAX)
            .to_le_bytes(),
    );
    for (tag, value) in entries {
        let (bytes, count) = value.encoded();
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&value.type_code().to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        if bytes.len() <= 4 {
            let mut inline = bytes;
            inline.resize(4, 0);
            out.extend_from_slice(&inline);
        } else {
            out.extend_from_slice(&len_u32(data_offset).to_le_bytes());
            data_offset += bytes.len() + bytes.len() % 2;
            data.extend_from_slice(&bytes);
            if bytes.len() % 2 == 1 {
                data.push(0);
            }
        }
    }
    // No next IFD: there is no thumbnail.
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&data);
}

/// `YYYY:MM:DD HH:MM:SS`, EXIF's date form.
pub(crate) fn exif_date(time: &CaptureTime) -> String {
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        time.year, time.month, time.day, time.hour, time.minute, time.second
    )
}

/// `+02:00`, the form of EXIF's `OffsetTime*` and of XMP dates.
pub(crate) fn offset_text(minutes: i16) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let minutes = minutes.unsigned_abs();
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Degrees, minutes and seconds as three rationals, the seconds to a
/// ten-thousandth. Counted in whole ten-thousandths of a second first, so a
/// rounding can never write 60 seconds.
fn dms(degrees: f64) -> Vec<Ratio> {
    const PER_SECOND: u64 = 10_000;
    let total = (degrees.abs() * 3600.0 * PER_SECOND as f64).round() as u64;
    let whole = |v: u64| Ratio {
        numerator: u32::try_from(v).unwrap_or(u32::MAX),
        denominator: 1,
    };
    vec![
        whole(total / (3600 * PER_SECOND)),
        whole(total / (60 * PER_SECOND) % 60),
        Ratio {
            numerator: u32::try_from(total % (60 * PER_SECOND)).unwrap_or(u32::MAX),
            denominator: PER_SECOND as u32,
        },
    ]
}

fn gps_entries(position: &GpsPosition) -> Vec<(u16, Value)> {
    let latitude_ref = if position.latitude < 0.0 { "S" } else { "N" };
    let longitude_ref = if position.longitude < 0.0 { "W" } else { "E" };
    let mut entries = vec![
        (0x0000, Value::Byte(vec![2, 3, 0, 0])),
        (0x0001, Value::Ascii(latitude_ref.into())),
        (0x0002, Value::Rational(dms(position.latitude))),
        (0x0003, Value::Ascii(longitude_ref.into())),
        (0x0004, Value::Rational(dms(position.longitude))),
    ];
    if let Some((below, centimetres)) = altitude_centimetres(position.altitude) {
        entries.push((0x0005, Value::Byte(vec![u8::from(below)])));
        entries.push((
            0x0006,
            Value::Rational(vec![Ratio {
                numerator: centimetres,
                denominator: 100,
            }]),
        ));
    }
    entries
}

/// The TIFF structure for `photo`, without the JPEG prefix.
pub(crate) fn exif_block(photo: &PhotoMetadata) -> Vec<u8> {
    let text =
        |tag: u16, value: &Option<String>| value.as_ref().map(|v| (tag, Value::Ascii(v.clone())));

    let mut exif: Vec<(u16, Value)> = Vec::new();
    if let Some(shutter) = usable(photo.shutter) {
        exif.push((0x829A, Value::Rational(vec![shutter])));
    }
    if let Some(aperture) = usable(photo.aperture) {
        exif.push((0x829D, Value::Rational(vec![aperture])));
    }
    // ISO above 65 535 does not fit the SHORT tag; the XMP packet carries it.
    if let Some(iso) = photo.iso.and_then(|iso| u16::try_from(iso).ok()) {
        exif.push((0x8827, Value::Short(iso)));
    }
    exif.push((0x9000, Value::Undefined(b"0232".to_vec())));
    if let Some(time) = &photo.captured {
        exif.push((0x9003, Value::Ascii(exif_date(time))));
        if let Some(offset) = time.offset_minutes {
            exif.push((0x9011, Value::Ascii(offset_text(offset))));
        }
    }
    if let Some(bias) = bias_hundredths(photo.exposure_bias) {
        exif.push((0x9204, Value::SRational(bias, 100)));
    }
    if let Some(fired) = photo.flash {
        exif.push((0x9209, Value::Short(u16::from(fired))));
    }
    if let Some(focal) = usable(photo.focal_length) {
        exif.push((0x920A, Value::Rational(vec![focal])));
    }
    exif.extend(text(0xA434, &photo.lens));

    let gps = usable_position(photo.position).map(|p| gps_entries(&p));

    // IFD 0, with placeholder pointers: an IFD's size does not depend on the
    // offsets it holds, so the layout is known before they are.
    let mut ifd0: Vec<(u16, Value)> = Vec::new();
    ifd0.extend(text(0x010F, &photo.make));
    ifd0.extend(text(0x0110, &photo.model));
    // Always 1: the pixels are already upright, and the source orientation
    // would rotate them a second time (ADR 0157 §2).
    ifd0.push((0x0112, Value::Short(1)));
    ifd0.push((0x0131, Value::Ascii(SOFTWARE.into())));
    ifd0.extend(text(0x013B, &photo.creator));
    ifd0.extend(text(0x8298, &photo.copyright));
    ifd0.push((0x8769, Value::Long(0)));
    if gps.is_some() {
        ifd0.push((0x8825, Value::Long(0)));
    }

    let exif_offset = 8 + ifd_size(&ifd0);
    let gps_offset = exif_offset + ifd_size(&exif);
    for (tag, value) in &mut ifd0 {
        match tag {
            0x8769 => *value = Value::Long(len_u32(exif_offset)),
            0x8825 => *value = Value::Long(len_u32(gps_offset)),
            _ => {}
        }
    }

    let mut out = b"II*\0".to_vec();
    out.extend_from_slice(&8u32.to_le_bytes());
    write_ifd(&mut out, &ifd0);
    write_ifd(&mut out, &exif);
    if let Some(gps) = &gps {
        write_ifd(&mut out, gps);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degrees_never_round_up_to_sixty_seconds() {
        let ratios = dms(1.0 - 1e-9);
        assert_eq!(
            ratios
                .iter()
                .map(|r| (r.numerator, r.denominator))
                .collect::<Vec<_>>(),
            vec![(1, 1), (0, 1), (0, 10_000)]
        );
    }

    #[test]
    fn offsets_are_written_with_their_sign() {
        assert_eq!(offset_text(120), "+02:00");
        assert_eq!(offset_text(-330), "-05:30");
    }
}
