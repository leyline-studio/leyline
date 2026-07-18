//! Integration tests: export presets and history (`docs/catalog.md` §27, §28).

use leyline_catalog::{Catalog, NewAsset};
use leyline_core::{AssetId, LeylineError, MediaType};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Exports").unwrap()
}

fn add(catalog: &mut Catalog) -> AssetId {
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: "IMG_0001.CR3".to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xAB; 32],
        width: None,
        height: None,
        capture_date: None,
        capture_offset_minutes: None,
    };
    catalog.add_asset(&new).unwrap().asset
}

#[test]
fn presets_store_and_list_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let web = catalog
        .create_export_preset("Web", r#"{"format":"jpeg","quality":80,"max_edge":2048}"#)
        .unwrap();
    catalog
        .create_export_preset("Archive", r#"{"format":"png"}"#)
        .unwrap();

    let presets = catalog.export_presets().unwrap();
    let names: Vec<_> = presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Archive", "Web"]);
    assert_eq!(presets[1].preset, web);
    assert!(presets[1].settings_json.contains("2048"));
}

#[test]
fn history_journals_newest_first_and_survives_preset_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let asset = add(&mut catalog);
    let preset = catalog
        .create_export_preset("Web", r#"{"format":"jpeg"}"#)
        .unwrap();

    catalog
        .record_export(asset, Some(preset), "jpg", "/out/IMG_0001.jpg")
        .unwrap();
    catalog
        .record_export(asset, None, "png", "/out/IMG_0001.png")
        .unwrap();

    let history = catalog.export_history(asset).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].format, "png");
    assert_eq!(history[0].preset, None);
    assert_eq!(history[1].preset, Some(preset));
    assert_eq!(history[1].destination, "/out/IMG_0001.jpg");

    // §28: deleting the preset keeps the history (ON DELETE SET NULL).
    catalog
        .connection()
        .execute("DELETE FROM export_presets WHERE id = ?1", [preset.get()])
        .unwrap();
    let history = catalog.export_history(asset).unwrap();
    assert_eq!(history[1].preset, None);

    assert!(matches!(
        catalog.record_export(AssetId::new(999), None, "jpg", "/out/x.jpg"),
        Err(LeylineError::AssetMissing(_))
    ));
}
