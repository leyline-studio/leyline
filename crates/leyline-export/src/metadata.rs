//! What an exported file says about the photograph (ADR 0157).
//!
//! The engine gathers the facts into a [`PhotoMetadata`]; the recipe's
//! [`ExportMetadata`] policy decides which of them leave the machine; the
//! two writers below turn what is left into the bytes each format carries.
//! Nothing here reads a clock or a random source (ADR 0157 §3), so the same
//! facts give the same bytes.

use serde::{Deserialize, Serialize};

pub(crate) mod exif_block;
pub(crate) mod xmp_packet;

/// Which part of a [`PhotoMetadata`] an export writes (ADR 0157 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExportMetadata {
    /// Everything, the position included.
    Full,
    /// Everything but the GPS position and the city, state and country —
    /// the default of a new export: publishing where one lives is a choice
    /// one makes, not one a default makes for them.
    WithoutLocation,
    /// Nothing but the ICC profile: what every export wrote before ADR 0157.
    None,
}

impl ExportMetadata {
    /// What a recipe written before ADR 0157 reads as. Anything but `None`
    /// would change an old preset's bytes under a new build, which §5.1 of
    /// `docs/pipeline.md` forbids.
    pub(crate) fn before_adr_0157() -> Self {
        ExportMetadata::None
    }
}

/// A capture time as the camera's clock showed it, with the offset to UTC
/// when the file recorded one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureTime {
    /// Year, four digits.
    pub year: u16,
    /// Month, 1-12.
    pub month: u8,
    /// Day of the month, 1-31.
    pub day: u8,
    /// Hour, 0-23.
    pub hour: u8,
    /// Minute, 0-59.
    pub minute: u8,
    /// Second, 0-59.
    pub second: u8,
    /// Offset from UTC in minutes, when known.
    pub offset_minutes: Option<i16>,
}

/// An unsigned EXIF rational: `1/3200` s, `56/10` for f/5.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ratio {
    /// Numerator.
    pub numerator: u32,
    /// Denominator; a zero makes the value absent rather than infinite.
    pub denominator: u32,
}

/// A GPS position in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpsPosition {
    /// Latitude in [-90, 90], north positive.
    pub latitude: f64,
    /// Longitude in [-180, 180], east positive.
    pub longitude: f64,
    /// Altitude in metres above sea level, when known.
    pub altitude: Option<f64>,
}

/// Everything an export may write about the photograph (ADR 0157 §2), already
/// resolved by the engine: an authored value has won over the file's own
/// (ADR 0099 §2) before it reaches this crate. Every field is optional; an
/// empty value still writes the orientation and the software name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhotoMetadata {
    /// When the photograph was taken.
    pub captured: Option<CaptureTime>,
    /// Camera maker.
    pub make: Option<String>,
    /// Camera model.
    pub model: Option<String>,
    /// Lens model.
    pub lens: Option<String>,
    /// ISO sensitivity.
    pub iso: Option<u32>,
    /// Shutter speed in seconds.
    pub shutter: Option<Ratio>,
    /// Aperture f-number.
    pub aperture: Option<Ratio>,
    /// Focal length in millimetres.
    pub focal_length: Option<Ratio>,
    /// Exposure bias in EV.
    pub exposure_bias: Option<f64>,
    /// Whether the flash fired.
    pub flash: Option<bool>,
    /// Creator.
    pub creator: Option<String>,
    /// Copyright notice.
    pub copyright: Option<String>,
    /// Credit line.
    pub credit: Option<String>,
    /// Title.
    pub title: Option<String>,
    /// Caption.
    pub caption: Option<String>,
    /// Flat keywords (`dc:subject`), in the order they are written.
    pub keywords: Vec<String>,
    /// Keyword paths joined by `|` (`lr:hierarchicalSubject`), in the order
    /// they are written.
    pub keyword_paths: Vec<String>,
    /// Where it was taken. Location: dropped under `WithoutLocation`.
    pub position: Option<GpsPosition>,
    /// City. Location.
    pub city: Option<String>,
    /// State or province. Location.
    pub state: Option<String>,
    /// Country. Location.
    pub country: Option<String>,
}

impl PhotoMetadata {
    /// What `policy` lets leave the machine, or `None` when nothing does.
    pub(crate) fn under(&self, policy: ExportMetadata) -> Option<PhotoMetadata> {
        match policy {
            ExportMetadata::None => None,
            ExportMetadata::Full => Some(self.clone()),
            ExportMetadata::WithoutLocation => Some(PhotoMetadata {
                position: None,
                city: None,
                state: None,
                country: None,
                ..self.clone()
            }),
        }
    }
}

/// The software name both blocks carry — without a version number, so that
/// replaying a recipe on the next build gives the same bytes (ADR 0157 §2).
pub(crate) const SOFTWARE: &str = "Leyline";

/// A ratio that can be written: a zero denominator is no value at all.
pub(crate) fn usable(ratio: Option<Ratio>) -> Option<Ratio> {
    ratio.filter(|r| r.denominator != 0)
}

/// An exposure bias as an EXIF signed rational, in hundredths of an EV.
pub(crate) fn bias_hundredths(bias: Option<f64>) -> Option<i32> {
    let hundredths = (bias? * 100.0).round();
    // Bounded well inside i32: no camera biases by more than a few EV.
    (hundredths.is_finite() && hundredths.abs() <= 100_000.0).then_some(hundredths as i32)
}

/// A position that can be written: finite and inside the globe.
pub(crate) fn usable_position(position: Option<GpsPosition>) -> Option<GpsPosition> {
    position.filter(|p| {
        p.latitude.is_finite()
            && p.longitude.is_finite()
            && p.latitude.abs() <= 90.0
            && p.longitude.abs() <= 180.0
    })
}

/// An altitude that can be written, as (below sea level, centimetres).
pub(crate) fn altitude_centimetres(altitude: Option<f64>) -> Option<(bool, u32)> {
    let altitude = altitude?;
    let centimetres = (altitude.abs() * 100.0).round();
    (centimetres.is_finite() && centimetres <= f64::from(u32::MAX))
        .then_some((altitude < 0.0, centimetres as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_location_drops_the_four_location_fields_only() {
        let photo = PhotoMetadata {
            make: Some("Canon".into()),
            position: Some(GpsPosition {
                latitude: 48.0,
                longitude: 2.0,
                altitude: None,
            }),
            city: Some("Paris".into()),
            state: Some("IdF".into()),
            country: Some("France".into()),
            ..PhotoMetadata::default()
        };
        let written = photo
            .under(ExportMetadata::WithoutLocation)
            .expect("something is written");
        assert_eq!(written.make.as_deref(), Some("Canon"));
        assert_eq!(
            (
                written.position,
                written.city,
                written.state,
                written.country
            ),
            (None, None, None, None)
        );
        assert!(photo.under(ExportMetadata::None).is_none());
        assert_eq!(photo.under(ExportMetadata::Full), Some(photo));
    }

    #[test]
    fn values_that_cannot_be_written_are_absent() {
        assert_eq!(bias_hundredths(Some(f64::NAN)), None);
        assert_eq!(bias_hundredths(Some(-2.0 / 3.0)), Some(-67));
        assert_eq!(
            usable(Some(Ratio {
                numerator: 1,
                denominator: 0
            })),
            None
        );
        let off_the_globe = GpsPosition {
            latitude: 91.0,
            longitude: 0.0,
            altitude: None,
        };
        assert_eq!(usable_position(Some(off_the_globe)), None);
        assert_eq!(altitude_centimetres(Some(-1.234)), Some((true, 123)));
    }
}
