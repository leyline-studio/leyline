//! Integration tests: EXIF metadata, cameras and lenses
//! (`docs/catalog.md` §13, §14, §15).

use leyline_catalog::{CameraInfo, Catalog, LensInfo, Metadata, NewAsset, Rational};
use leyline_core::Settings;
use leyline_core::{AssetId, LeylineError, MediaType};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Metadata").unwrap()
}

fn add(catalog: &mut Catalog, filename: &str) -> AssetId {
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: filename.to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xAB; 32],
        width: None,
        height: None,
        capture_date: None,
        capture_offset_minutes: None,
    };
    catalog.add_asset(&new, &Settings::default()).unwrap().asset
}

fn r5_sample() -> Metadata {
    Metadata {
        camera: Some(CameraInfo {
            manufacturer: "Canon".to_owned(),
            model: "EOS R5".to_owned(),
        }),
        lens: Some(LensInfo {
            manufacturer: "Canon".to_owned(),
            model: "RF 70-200mm F2.8".to_owned(),
            mount: Some("RF".to_owned()),
        }),
        orientation: Some(1),
        iso: Some(800),
        shutter: Some(Rational {
            numerator: 1,
            denominator: 3200,
        }),
        aperture: Some(Rational {
            numerator: 56,
            denominator: 10,
        }),
        focal_length: Some(Rational {
            numerator: 70,
            denominator: 1,
        }),
        exposure_bias: Some(-0.33),
        flash: Some(false),
        white_balance_mode: Some(0),
        color_space: Some("sRGB".to_owned()),
        gps_latitude: Some(59.91),
        gps_longitude: Some(10.75),
        gps_altitude: Some(12.0),
        artist: Some("Ansel".to_owned()),
        copyright: Some("CC BY-NC".to_owned()),
    }
}

#[test]
fn metadata_round_trips_completely() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let asset = add(&mut catalog, "IMG_0001.CR3");

    assert_eq!(catalog.metadata(asset).unwrap(), None);
    let meta = r5_sample();
    catalog.set_metadata(asset, &meta).unwrap();
    assert_eq!(catalog.metadata(asset).unwrap(), Some(meta));

    // Replacing is an upsert, not an error.
    let sparse = Metadata {
        iso: Some(100),
        ..Metadata::default()
    };
    catalog.set_metadata(asset, &sparse).unwrap();
    assert_eq!(catalog.metadata(asset).unwrap(), Some(sparse));
}

#[test]
fn generated_columns_derive_decimal_values() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let asset = add(&mut catalog, "IMG_0001.CR3");
    catalog.set_metadata(asset, &r5_sample()).unwrap();

    // §13: rationals are the reference, SQLite derives the searchable
    // decimals itself.
    let (shutter, aperture, focal): (f64, f64, f64) = catalog
        .connection()
        .query_row(
            "SELECT shutter_speed_s, aperture_f, focal_length_mm
             FROM metadata WHERE asset_id = ?1",
            [asset.get()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!((shutter - 1.0 / 3200.0).abs() < 1e-12);
    assert!((aperture - 5.6).abs() < 1e-12);
    assert!((focal - 70.0).abs() < 1e-12);
    assert!(
        (Rational {
            numerator: 56,
            denominator: 10
        }
        .as_f64()
            - 5.6)
            .abs()
            < 1e-12
    );
}

#[test]
fn cameras_and_lenses_are_deduplicated() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let a = add(&mut catalog, "IMG_0001.CR3");
    let b = add(&mut catalog, "IMG_0002.CR3");

    catalog.set_metadata(a, &r5_sample()).unwrap();
    // Same body and glass, mount unknown this time: still one row each,
    // and the known mount is kept.
    let mut second = r5_sample();
    second.lens.as_mut().unwrap().mount = None;
    catalog.set_metadata(b, &second).unwrap();

    let (cameras, lenses): (i64, i64) = catalog
        .connection()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM cameras), (SELECT COUNT(*) FROM lenses)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((cameras, lenses), (1, 1));
    assert_eq!(
        catalog.metadata(b).unwrap().unwrap().lens.unwrap().mount,
        Some("RF".to_owned())
    );
}

#[test]
fn artist_and_copyright_feed_the_search_index() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let asset = add(&mut catalog, "IMG_0001.CR3");

    catalog.set_metadata(asset, &r5_sample()).unwrap();
    assert_eq!(catalog.search_assets("ansel").unwrap(), vec![asset]);

    // A rebuild keeps them: the index regenerates from metadata too.
    catalog.rebuild_search_index().unwrap();
    assert_eq!(catalog.search_assets("ansel").unwrap(), vec![asset]);
}

#[test]
fn invalid_rationals_gps_and_assets_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let asset = add(&mut catalog, "IMG_0001.CR3");

    let bad_denominator = Metadata {
        shutter: Some(Rational {
            numerator: 1,
            denominator: 0,
        }),
        ..Metadata::default()
    };
    assert!(matches!(
        catalog.set_metadata(asset, &bad_denominator),
        Err(LeylineError::Io(_))
    ));

    let bad_gps = Metadata {
        gps_latitude: Some(120.0),
        ..Metadata::default()
    };
    assert!(matches!(
        catalog.set_metadata(asset, &bad_gps),
        Err(LeylineError::Io(_))
    ));

    assert!(matches!(
        catalog.set_metadata(AssetId::new(999), &Metadata::default()),
        Err(LeylineError::AssetMissing(_))
    ));
    assert!(matches!(
        catalog.metadata(AssetId::new(999)),
        Err(LeylineError::AssetMissing(_))
    ));
}
