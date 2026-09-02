//! Integration tests: contact-sheet presets (ADR 0110 §8), parallel to print
//! presets (`docs/catalog.md` §42/§43).

use leyline_catalog::Catalog;
use leyline_core::LeylineError;

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Sheets").unwrap()
}

#[test]
fn presets_store_and_list_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let index = catalog
        .create_contact_sheet_preset("Index 4x5", r#"{"columns":4,"rows":5}"#)
        .unwrap();
    catalog
        .create_contact_sheet_preset("Big 2x2", r#"{"columns":2,"rows":2}"#)
        .unwrap();

    let presets = catalog.contact_sheet_presets().unwrap();
    let names: Vec<_> = presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Big 2x2", "Index 4x5"]);
    assert_eq!(presets[1].preset, index);
    assert!(presets[1].settings_json.contains("\"rows\":5"));

    assert_eq!(catalog.contact_sheet_preset(index).unwrap(), presets[1]);
    assert!(matches!(
        catalog.contact_sheet_preset(leyline_core::ContactSheetPresetId::new(999)),
        Err(LeylineError::ContactSheetPresetMissing(_))
    ));
}

/// A contact-sheet recipe and a print recipe live in different tables, and
/// neither listing sees the other's rows (ADR 0110 §8).
#[test]
fn sheet_presets_never_land_among_the_print_presets() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    catalog
        .create_print_preset("Baryta A4", r#"{"paper":"a4","dpi":300}"#)
        .unwrap();
    catalog
        .create_contact_sheet_preset("Index 4x5", r#"{"columns":4,"rows":5}"#)
        .unwrap();

    assert_eq!(catalog.print_presets().unwrap().len(), 1);
    assert_eq!(catalog.contact_sheet_presets().unwrap().len(), 1);
}
