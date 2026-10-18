//! What an exported file says, read back by readers that are not ours
//! (ADR 0157 §Consequences): `kamadak-exif` for the EXIF block, `roxmltree`
//! for the XMP packet.

use std::path::Path;

use leyline_export::{
    CaptureTime, ExportError, ExportFormat, ExportMetadata, ExportSettings, GpsPosition,
    PhotoMetadata, Provenance, Ratio, encode,
};

fn pixels() -> Vec<u8> {
    (0..8 * 6 * 3).map(|i| (i * 5) as u8).collect()
}

fn photo() -> PhotoMetadata {
    PhotoMetadata {
        captured: Some(CaptureTime {
            year: 2022,
            month: 4,
            day: 17,
            hour: 11,
            minute: 51,
            second: 12,
            offset_minutes: Some(120),
        }),
        make: Some("Canon".into()),
        model: Some("Canon EOS 60D".into()),
        lens: Some("EF-S 18-135mm".into()),
        iso: Some(400),
        shutter: Some(Ratio {
            numerator: 1,
            denominator: 250,
        }),
        aperture: Some(Ratio {
            numerator: 56,
            denominator: 10,
        }),
        focal_length: Some(Ratio {
            numerator: 70,
            denominator: 1,
        }),
        exposure_bias: Some(-2.0 / 3.0),
        flash: Some(false),
        creator: Some("Élise Martin".into()),
        copyright: Some("© 2022 Élise Martin".into()),
        credit: None,
        title: Some("Quai <nord> & sud".into()),
        caption: None,
        keywords: vec!["port".into(), "soir".into()],
        keyword_paths: vec!["lieux|port".into(), "moments|soir".into()],
        position: Some(GpsPosition {
            latitude: 48.8566,
            longitude: -2.3522,
            altitude: Some(35.0),
        }),
        city: Some("Paris".into()),
        state: None,
        country: Some("France".into()),
        provenance: Some(Provenance {
            revision_fingerprint: [0xAB; 32],
            stages: vec![("input".into(), 6), ("tone".into(), 2)],
            decoder: "0.22.2".into(),
            export_settings_json: r#"{"format":"jpeg","quality":90}"#.into(),
        }),
    }
}

fn export(dir: &Path, format: ExportFormat, policy: ExportMetadata) -> Vec<u8> {
    let path = dir.join(format!("{policy:?}.{}", format.extension()));
    let settings = ExportSettings {
        format,
        metadata: policy,
        ..ExportSettings::default()
    };
    encode(&path, 8, 6, &pixels(), &settings, &photo()).expect("encoded");
    std::fs::read(&path).expect("written")
}

fn exif_of(bytes: &[u8]) -> Option<exif::Exif> {
    exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()
}

fn text(exif: &exif::Exif, tag: exif::Tag, ifd: exif::In) -> Option<String> {
    exif.get_field(tag, ifd).map(|f| match &f.value {
        exif::Value::Ascii(parts) => String::from_utf8_lossy(&parts[0]).into_owned(),
        other => format!("{other:?}"),
    })
}

/// The packet, found the way any reader scanning a file for XMP finds it.
fn xmp_of(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let start = text.find("<x:xmpmeta")?;
    let end = text.find("</x:xmpmeta>")? + "</x:xmpmeta>".len();
    Some(text[start..end].to_owned())
}

fn has_gps_bytes(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.contains("GPS")
        || text.contains("Paris")
        || exif_of(bytes).is_some_and(|e| {
            e.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
                .is_some()
        })
}

const CARRIERS: [ExportFormat; 4] = [
    ExportFormat::Jpeg,
    ExportFormat::Png,
    ExportFormat::Webp,
    ExportFormat::Tiff,
];

#[test]
fn every_carrier_says_what_was_written() {
    let dir = tempfile::tempdir().expect("temporary directory");
    for format in CARRIERS {
        let bytes = export(dir.path(), format, ExportMetadata::Full);
        let exif = exif_of(&bytes).unwrap_or_else(|| panic!("{format:?}: EXIF readable"));
        assert_eq!(
            text(&exif, exif::Tag::Make, exif::In::PRIMARY).as_deref(),
            Some("Canon"),
            "{format:?}"
        );
        let orientation = exif
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0));
        assert_eq!(orientation, Some(1), "{format:?}: always upright");

        let packet = xmp_of(&bytes).unwrap_or_else(|| panic!("{format:?}: XMP present"));
        let document = roxmltree::Document::parse(&packet).expect("the packet is XML");
        let subjects: Vec<&str> = document
            .descendants()
            .filter(|n| n.tag_name().name() == "subject")
            .flat_map(|n| n.descendants().filter(|li| li.tag_name().name() == "li"))
            .filter_map(|li| li.text())
            .collect();
        assert_eq!(subjects, ["port", "soir"], "{format:?}");
        let title = document
            .descendants()
            .find(|n| n.tag_name().name() == "title")
            .and_then(|n| n.descendants().find(|li| li.tag_name().name() == "li"))
            .and_then(|li| li.text());
        assert_eq!(title, Some("Quai <nord> & sud"), "{format:?}");
        assert!(packet.contains("48,51.396000N"), "{format:?}");

        // ADR 0153 §3: the block leads back to the revision, and its recipe
        // survives the escaping intact.
        let leyline = |name: &str| {
            document
                .descendants()
                .find(|n| {
                    n.tag_name().name() == name
                        && n.tag_name().namespace()
                            == Some("https://leyline.photo/ns/provenance/1.0/")
                })
                .and_then(|n| n.text())
                .map(str::to_owned)
        };
        assert_eq!(leyline("Revision"), Some("ab".repeat(32)), "{format:?}");
        assert_eq!(leyline("Stages").as_deref(), Some("input=6;tone=2"));
        assert_eq!(
            leyline("ExportSettings").as_deref(),
            Some(r#"{"format":"jpeg","quality":90}"#)
        );
        assert!(packet.contains("2,21.132000W"), "{format:?}");
    }
}

#[test]
fn the_exif_block_carries_the_capture_and_the_position() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let bytes = export(dir.path(), ExportFormat::Jpeg, ExportMetadata::Full);
    let exif = exif_of(&bytes).expect("EXIF readable");
    assert_eq!(
        text(&exif, exif::Tag::DateTimeOriginal, exif::In::PRIMARY).as_deref(),
        Some("2022:04:17 11:51:12")
    );
    assert_eq!(
        text(&exif, exif::Tag::OffsetTimeOriginal, exif::In::PRIMARY).as_deref(),
        Some("+02:00")
    );
    assert_eq!(
        text(&exif, exif::Tag::GPSLongitudeRef, exif::In::PRIMARY).as_deref(),
        Some("W")
    );
    let iso = exif
        .get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0));
    assert_eq!(iso, Some(400));
}

#[test]
fn neither_policy_short_of_full_leaves_a_location_byte() {
    let dir = tempfile::tempdir().expect("temporary directory");
    for format in CARRIERS {
        for policy in [ExportMetadata::WithoutLocation, ExportMetadata::None] {
            let bytes = export(dir.path(), format, policy);
            assert!(!has_gps_bytes(&bytes), "{format:?} under {policy:?}");
        }
        let none = export(dir.path(), format, ExportMetadata::None);
        assert!(xmp_of(&none).is_none(), "{format:?}: no packet under None");
        assert!(
            !String::from_utf8_lossy(&none).contains("Canon"),
            "{format:?}: nothing under None"
        );
    }
}

#[test]
fn two_exports_of_the_same_facts_are_the_same_bytes() {
    let dir = tempfile::tempdir().expect("temporary directory");
    for format in CARRIERS {
        let first = export(dir.path(), format, ExportMetadata::Full);
        let second = export(dir.path(), format, ExportMetadata::Full);
        assert_eq!(first, second, "{format:?}");
    }
}

#[test]
fn an_oversized_packet_is_refused_never_truncated() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let crowded = PhotoMetadata {
        keywords: (0..8000).map(|n| format!("keyword-{n}")).collect(),
        ..photo()
    };
    let path = dir.path().join("crowded.jpg");
    let result = encode(&path, 8, 6, &pixels(), &ExportSettings::default(), &crowded);
    assert!(matches!(result, Err(ExportError::MetadataTooLarge { .. })));
    assert!(!path.exists(), "no half-written file");
}

#[test]
fn avif_says_it_carries_nothing() {
    assert!(!ExportFormat::Avif.carries_metadata());
    assert!(CARRIERS.iter().all(|f| f.carries_metadata()));
}
