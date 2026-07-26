//! Integration tests: GPS map pins (`docs/adr/0040-gps-map-view.md`).

use leyline_catalog::{Catalog, Metadata, NewAsset, RegisteredAsset};
use leyline_core::MediaType;
use leyline_core::Settings;

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Map").unwrap()
}

fn add(catalog: &mut Catalog, filename: &str, checksum: u8) -> RegisteredAsset {
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: filename.to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [checksum; 32],
        width: None,
        height: None,
        capture_date: None,
        capture_offset_minutes: None,
    };
    catalog.add_asset(&new, &Settings::default()).unwrap()
}

#[test]
fn an_asset_without_gps_has_no_pin() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    add(&mut catalog, "a.CR3", 1);
    assert_eq!(catalog.map_pins().unwrap(), vec![]);
}

#[test]
fn an_asset_with_gps_produces_one_pin() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = add(&mut catalog, "a.CR3", 1);
    catalog
        .set_metadata(
            registered.asset,
            &Metadata {
                gps_latitude: Some(48.8584),
                gps_longitude: Some(2.2945),
                ..Metadata::default()
            },
        )
        .unwrap();

    let pins = catalog.map_pins().unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].asset_id, registered.asset);
    assert_eq!(pins[0].version_id, registered.version);
    assert_eq!(pins[0].latitude, 48.8584);
    assert_eq!(pins[0].longitude, 2.2945);
}

#[test]
fn latitude_without_longitude_produces_no_pin() {
    // Malformed/partial metadata should never surface a half-valid pin.
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = add(&mut catalog, "a.CR3", 1);
    catalog
        .set_metadata(
            registered.asset,
            &Metadata {
                gps_latitude: Some(48.8584),
                gps_longitude: None,
                ..Metadata::default()
            },
        )
        .unwrap();
    assert_eq!(catalog.map_pins().unwrap(), vec![]);
}

#[test]
fn only_the_current_version_is_pinned() {
    // A second, non-current version of the same asset must not double the
    // pin count — map_pins joins through develop_current, same scoping as
    // the grid's uncollected view.
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = add(&mut catalog, "a.CR3", 1);
    catalog
        .set_metadata(
            registered.asset,
            &Metadata {
                gps_latitude: Some(10.0),
                gps_longitude: Some(20.0),
                ..Metadata::default()
            },
        )
        .unwrap();
    catalog
        .create_version(registered.version, "Virtual copy", None)
        .unwrap();

    assert_eq!(catalog.map_pins().unwrap().len(), 1);
}
