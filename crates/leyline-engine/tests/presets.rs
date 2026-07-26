//! Integration tests: preset capture and application
//! (`docs/engine-api.md` §10.3, `docs/presets.md`).

use leyline_catalog::{CHECKSUM_LEN, Catalog, NewAsset, RegisteredAsset};
use leyline_core::{CURRENT_SCHEMA, MediaType, PresetSettings, Settings, SettingsGroup, VersionId};
use leyline_engine::{EditSession, Param, Value, apply_batch, capture};

fn catalog_with_asset(dir: &tempfile::TempDir, name: &str) -> (Catalog, RegisteredAsset) {
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Presets").unwrap();
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: name.to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xAB; CHECKSUM_LEN],
        width: None,
        height: None,
        capture_date: None,
        capture_offset_minutes: None,
    };
    let registered = catalog
        .add_asset(&new, &leyline_engine::neutral_settings())
        .unwrap();
    (catalog, registered)
}

#[test]
fn capture_then_apply_reproduces_only_the_included_groups() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, source) = catalog_with_asset(&dir, "IMG_0001.CR3");
    {
        let mut session = EditSession::open(&mut catalog, source.version).unwrap();
        session.set(Param::Contrast, Value::Int(30)).unwrap();
        session.set(Param::Rotation, Value::Float(5.0)).unwrap();
        session.commit().unwrap();
    }

    let preset = capture(&catalog, source.version, &[SettingsGroup::Tone]).unwrap();
    assert_eq!(preset.contrast, Some(30));
    assert_eq!(preset.rotation, None); // Geometry not requested.

    let folder = catalog.ensure_folder("Photos").unwrap();
    let target = catalog
        .add_asset(
            &NewAsset {
                folder,
                filename: "IMG_0002.CR3".to_owned(),
                extension: "CR3".to_owned(),
                media_type: MediaType::Raw,
                file_size: 1,
                checksum: [0xCD; CHECKSUM_LEN],
                width: None,
                height: None,
                capture_date: None,
                capture_offset_minutes: None,
            },
            &Settings::default(),
        )
        .unwrap()
        .version;

    let report = apply_batch(&mut catalog, &preset, &[target], |_, _| {});
    assert_eq!(report.applied, [target]);
    assert!(report.failed.is_empty());

    let head = catalog.version_head(target).unwrap();
    let settings = Settings::parse(&catalog.revision(head).unwrap().settings_json).unwrap();
    assert_eq!(settings.contrast, 30);
    assert_eq!(settings.rotation, 0.0); // Untouched: not in the preset.
}

#[test]
fn apply_always_creates_a_new_revision_never_an_amendment() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, target) = catalog_with_asset(&dir, "IMG_0001.CR3");
    let initial_head = catalog.version_head(target.version).unwrap();

    let preset = PresetSettings {
        schema: CURRENT_SCHEMA,
        groups: vec![SettingsGroup::Tone],
        exposure: Some(0.5),
        contrast: Some(10),
        highlights: Some(0),
        shadows: Some(0),
        whites: Some(0),
        blacks: Some(0),
        ..PresetSettings::default()
    };

    apply_batch(&mut catalog, &preset, &[target.version], |_, _| {});
    apply_batch(&mut catalog, &preset, &[target.version], |_, _| {});

    let history = catalog.version_history(target.version).unwrap();
    // Initial revision + two applications: three, none amended into another.
    assert_eq!(history.len(), 3);
    assert_ne!(history[0].revision, initial_head);
}

#[test]
fn one_failure_does_not_stop_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, ok) = catalog_with_asset(&dir, "IMG_0001.CR3");
    let missing = VersionId::new(999);

    let preset = PresetSettings {
        schema: CURRENT_SCHEMA,
        groups: vec![SettingsGroup::WhiteBalance],
        white_balance: Some(None),
        ..PresetSettings::default()
    };

    let report = apply_batch(&mut catalog, &preset, &[missing, ok.version], |_, _| {});
    assert_eq!(report.applied, [ok.version]);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].version, missing);
}
