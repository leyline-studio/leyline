//! Integration tests: develop presets (`docs/presets.md` §4).

use leyline_catalog::Catalog;
use leyline_core::{LeylineError, PresetId};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Presets").unwrap()
}

#[test]
fn presets_store_and_list_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let bw = catalog
        .create_preset(
            "Noir & Blanc",
            r#"{"schema":1,"groups":["Tone"],"contrast":20}"#,
        )
        .unwrap();
    catalog
        .create_preset("Chaud", r#"{"schema":1,"groups":["WhiteBalance"]}"#)
        .unwrap();

    let presets = catalog.presets().unwrap();
    let names: Vec<_> = presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Chaud", "Noir & Blanc"]);
    assert_eq!(presets[1].preset, bw);
    assert!(presets[1].preset_json.contains("\"contrast\":20"));

    assert_eq!(catalog.preset(bw).unwrap(), presets[1]);
    assert!(matches!(
        catalog.preset(PresetId::new(999)),
        Err(LeylineError::PresetMissing(_))
    ));
}

#[test]
fn rename_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let preset = catalog
        .create_preset("Draft", r#"{"schema":1,"groups":[]}"#)
        .unwrap();

    catalog.rename_preset(preset, "Final").unwrap();
    assert_eq!(catalog.preset(preset).unwrap().name, "Final");

    catalog.delete_preset(preset).unwrap();
    assert!(matches!(
        catalog.preset(preset),
        Err(LeylineError::PresetMissing(_))
    ));
    assert!(matches!(
        catalog.rename_preset(preset, "Ghost"),
        Err(LeylineError::PresetMissing(_))
    ));
    assert!(matches!(
        catalog.delete_preset(preset),
        Err(LeylineError::PresetMissing(_))
    ));
}
