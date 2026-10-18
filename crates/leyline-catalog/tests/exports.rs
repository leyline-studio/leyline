//! Integration tests: export presets and history (`docs/catalog.md` §27, §28).

use leyline_catalog::{Catalog, ExportProvenance, NewAsset, RegisteredAsset};
use leyline_core::Settings;
use leyline_core::{AssetId, LeylineError, MediaType};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Exports").unwrap()
}

fn register(catalog: &mut Catalog) -> RegisteredAsset {
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
    catalog.add_asset(&new, &Settings::default()).unwrap()
}

/// A provenance naming the asset's real version and head, as the
/// foreign keys require.
fn provenance(catalog: &Catalog, registered: &RegisteredAsset, output: u8) -> ExportProvenance {
    ExportProvenance {
        version: registered.version,
        revision: catalog.version_head(registered.version).unwrap(),
        revision_fingerprint: [0x11; 32],
        export_settings_json: r#"{"format":"jpeg"}"#.to_owned(),
        decoder: "0.22.2".to_owned(),
        output_checksum: [output; 32],
    }
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

    assert_eq!(catalog.export_preset(web).unwrap(), presets[1]);
    assert!(matches!(
        catalog.export_preset(leyline_core::ExportPresetId::new(999)),
        Err(LeylineError::ExportPresetMissing(_))
    ));
}

#[test]
fn history_journals_newest_first_and_survives_preset_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = register(&mut catalog);
    let asset = registered.asset;
    let preset = catalog
        .create_export_preset("Web", r#"{"format":"jpeg"}"#)
        .unwrap();

    let first = provenance(&catalog, &registered, 1);
    catalog
        .record_export(asset, Some(preset), "jpg", "/out/IMG_0001.jpg", &first)
        .unwrap();
    let second = provenance(&catalog, &registered, 2);
    catalog
        .record_export(asset, None, "png", "/out/IMG_0001.png", &second)
        .unwrap();

    let history = catalog.export_history(asset).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].format, "png");
    assert_eq!(history[0].preset, None);
    assert_eq!(history[1].preset, Some(preset));
    assert_eq!(history[1].destination, "/out/IMG_0001.jpg");
    // ADR 0153 §2: what the export was made from, read back as written.
    assert_eq!(history[1].version, Some(first.version));
    assert_eq!(history[1].revision, Some(first.revision));
    assert_eq!(history[1].revision_fingerprint, Some([0x11; 32]));
    assert_eq!(history[1].decoder.as_deref(), Some("0.22.2"));
    assert_eq!(history[1].output_checksum, Some([1; 32]));
    assert_eq!(history[0].output_checksum, Some([2; 32]));

    // §28: deleting the preset keeps the history (ON DELETE SET NULL).
    catalog
        .connection()
        .execute("DELETE FROM export_presets WHERE id = ?1", [preset.get()])
        .unwrap();
    let history = catalog.export_history(asset).unwrap();
    assert_eq!(history[1].preset, None);

    assert!(matches!(
        catalog.record_export(AssetId::new(999), None, "jpg", "/out/x.jpg", &first),
        Err(LeylineError::AssetMissing(_))
    ));
}

/// The journal says a file was written, which stays true once the version
/// is gone: the version is cleared, the row and its checksum remain
/// (ADR 0153 §2, `ON DELETE SET NULL`).
#[test]
fn deleting_the_version_keeps_the_row_and_its_checksum() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = register(&mut catalog);
    // A second version, so the exported one is not the asset's last (§18).
    catalog
        .create_version(registered.version, "Noir & Blanc", None)
        .unwrap();
    let written = provenance(&catalog, &registered, 9);
    catalog
        .record_export(registered.asset, None, "jpg", "/out/a.jpg", &written)
        .unwrap();

    catalog.delete_version(registered.version).unwrap();
    let history = catalog.export_history(registered.asset).unwrap();
    assert_eq!(history[0].version, None);
    assert_eq!(history[0].output_checksum, Some([9; 32]));
}
