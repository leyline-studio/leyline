//! Integration tests: print presets (ADR 0036), parallel to export presets
//! (`docs/catalog.md` §27).

use leyline_catalog::Catalog;
use leyline_core::LeylineError;

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Prints").unwrap()
}

#[test]
fn presets_store_and_list_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let baryta = catalog
        .create_print_preset("Baryta A4", r#"{"paper":"a4","dpi":300}"#)
        .unwrap();
    catalog
        .create_print_preset("4x6 glossy", r#"{"paper":"custom"}"#)
        .unwrap();

    let presets = catalog.print_presets().unwrap();
    let names: Vec<_> = presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["4x6 glossy", "Baryta A4"]);
    assert_eq!(presets[1].preset, baryta);
    assert!(presets[1].settings_json.contains("300"));

    assert_eq!(catalog.print_preset(baryta).unwrap(), presets[1]);
    assert!(matches!(
        catalog.print_preset(leyline_core::PrintPresetId::new(999)),
        Err(LeylineError::PrintPresetMissing(_))
    ));
}
