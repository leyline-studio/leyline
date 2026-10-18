//! What an export says about the photograph, gathered from the catalog
//! (ADR 0157 §2). The export crate decides how much of it a recipe writes;
//! this module only resolves it — authored values over the file's own
//! (ADR 0099 §2), keywords through the tree, the capture time as the camera's
//! clock showed it.

use leyline_catalog::{Catalog, Metadata, Rational};
use leyline_core::{AssetId, Result};
use leyline_export::{CaptureTime, GpsPosition, PhotoMetadata, Ratio};

/// Everything the catalog knows that an export may write about `asset`.
pub(crate) fn photo_metadata(catalog: &Catalog, asset: AssetId) -> Result<PhotoMetadata> {
    let details = catalog.asset_details(asset)?;
    let written = catalog.description(asset)?.unwrap_or_default();
    let (keywords, keyword_paths) = crate::xmp::keyword_lists(catalog, &details.keywords)?;
    let offset = catalog.capture_offset_minutes(asset)?;
    let read = details.metadata.unwrap_or_default();
    Ok(assemble(
        &read,
        written,
        details.capture_date,
        offset,
        keywords,
        keyword_paths,
    ))
}

/// The pure half of [`photo_metadata`], so the precedence and the
/// conversions are tested without a catalog.
fn assemble(
    read: &Metadata,
    written: leyline_catalog::AssetDescription,
    capture_date: Option<i64>,
    offset_minutes: Option<i32>,
    keywords: Vec<String>,
    keyword_paths: Vec<String>,
) -> PhotoMetadata {
    PhotoMetadata {
        captured: capture_date.and_then(|ms| capture_time(ms, offset_minutes)),
        make: read.camera.as_ref().map(|c| c.manufacturer.clone()),
        model: read.camera.as_ref().map(|c| c.model.clone()),
        lens: read.lens.as_ref().map(|l| l.model.clone()),
        iso: read.iso,
        shutter: read.shutter.and_then(ratio),
        aperture: read.aperture.and_then(ratio),
        focal_length: read.focal_length.and_then(ratio),
        exposure_bias: read.exposure_bias,
        flash: read.flash,
        creator: written.creator.or_else(|| read.artist.clone()),
        copyright: written.copyright.or_else(|| read.copyright.clone()),
        credit: written.credit,
        title: written.title,
        caption: written.caption,
        keywords,
        keyword_paths,
        // The same "no fix" rule the map applies (`docs/catalog.md` §13): a
        // 0,0 pair is an absence, not a position in the Gulf of Guinea.
        position: leyline_catalog::gps_fix(read.gps_latitude, read.gps_longitude).map(
            |(latitude, longitude)| GpsPosition {
                latitude,
                longitude,
                altitude: read.gps_altitude,
            },
        ),
        city: written.city,
        state: written.state,
        country: written.country,
    }
}

/// The camera's wall clock for a stored capture date (`docs/catalog.md` §9):
/// `capture_date` is the UTC instant when the offset is known, and the wall
/// clock read as UTC when it is not, so adding the offset — or nothing — gives
/// the wall clock either way, whatever zone this machine is in.
fn capture_time(capture_date: i64, offset_minutes: Option<i32>) -> Option<CaptureTime> {
    let wall = capture_date.checked_add(i64::from(offset_minutes.unwrap_or(0)) * 60_000)?;
    let (year, month, day, hour, minute, second) = crate::rename::utc_parts(wall);
    Some(CaptureTime {
        year: u16::try_from(year).ok().filter(|y| *y <= 9999)?,
        month: u8::try_from(month).ok()?,
        day: u8::try_from(day).ok()?,
        hour: u8::try_from(hour).ok()?,
        minute: u8::try_from(minute).ok()?,
        second: u8::try_from(second).ok()?,
        offset_minutes: offset_minutes.and_then(|m| i16::try_from(m).ok()),
    })
}

/// An unsigned EXIF rational, or nothing when the stored one does not fit.
fn ratio(r: Rational) -> Option<Ratio> {
    Some(Ratio {
        numerator: u32::try_from(r.numerator).ok()?,
        denominator: u32::try_from(r.denominator).ok().filter(|d| *d != 0)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use leyline_catalog::AssetDescription;

    #[test]
    fn the_wall_clock_is_the_same_reading_with_or_without_an_offset() {
        // 2022-04-17 11:51:12 on the camera's clock, from a JPEG that says
        // +02:00 (stored as the UTC instant) and from its RAW that says
        // nothing (stored as the wall clock read as UTC).
        let with_offset = capture_time(1_650_189_072_000, Some(120)).expect("a time");
        let without = capture_time(1_650_196_272_000, None).expect("a time");
        for time in [with_offset, without] {
            assert_eq!(
                (
                    time.year,
                    time.month,
                    time.day,
                    time.hour,
                    time.minute,
                    time.second
                ),
                (2022, 4, 17, 11, 51, 12)
            );
        }
        assert_eq!(with_offset.offset_minutes, Some(120));
        assert_eq!(without.offset_minutes, None);
    }

    #[test]
    fn an_authored_creator_wins_and_the_file_s_is_the_fallback() {
        let read = Metadata {
            artist: Some("camera owner".into()),
            copyright: Some("from the file".into()),
            gps_latitude: Some(0.0),
            gps_longitude: Some(0.0),
            ..Metadata::default()
        };
        let written = AssetDescription {
            creator: Some("typed".into()),
            ..AssetDescription::default()
        };
        let photo = assemble(&read, written, None, None, Vec::new(), Vec::new());
        assert_eq!(photo.creator.as_deref(), Some("typed"));
        assert_eq!(photo.copyright.as_deref(), Some("from the file"));
        assert_eq!(photo.position, None, "0,0 is no fix");
    }

    #[test]
    fn a_rational_that_does_not_fit_is_absent() {
        assert_eq!(
            ratio(Rational {
                numerator: -1,
                denominator: 3
            }),
            None
        );
        assert_eq!(
            ratio(Rational {
                numerator: 1,
                denominator: 0
            }),
            None
        );
    }
}
