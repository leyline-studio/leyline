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

#[test]
fn a_preset_carries_its_shelf_and_its_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Shelf").unwrap();

    let film = catalog.create_preset_folder("  Film  ").unwrap();
    assert_eq!(
        catalog
            .preset_folders()
            .unwrap()
            .first()
            .map(|f| f.name.as_str()),
        Some("Film"),
        "a folder name is trimmed"
    );
    assert!(catalog.create_preset_folder("   ").is_err());

    let gold = catalog
        .create_preset("Kodak Gold", "{\"exposure\":0.3}")
        .unwrap();
    let plain = catalog.create_preset("Neutre", "{}").unwrap();
    catalog.file_preset(gold, Some(film)).unwrap();
    catalog.favourite_preset(gold, true).unwrap();

    // Favourites first, then by name (ADR 0058 §2).
    let listed: Vec<String> = catalog
        .presets()
        .unwrap()
        .into_iter()
        .map(|preset| preset.name)
        .collect();
    assert_eq!(listed, vec!["Kodak Gold".to_owned(), "Neutre".to_owned()]);

    let stored = catalog.preset(gold).unwrap();
    assert_eq!(stored.folder, Some(film));
    assert!(stored.favourite);
    assert_eq!(stored.revision, 1, "a fresh preset is at version 1");

    // Updating bumps the version; the JSON follows.
    let bumped = catalog.update_preset(gold, "{\"exposure\":0.5}").unwrap();
    assert_eq!(bumped, 2);
    assert_eq!(
        catalog.preset(gold).unwrap().preset_json,
        "{\"exposure\":0.5}"
    );

    // Deleting a folder sends its presets back to the root, it loses none.
    catalog.delete_preset_folder(film).unwrap();
    assert_eq!(catalog.preset(gold).unwrap().folder, None);
    assert_eq!(catalog.presets().unwrap().len(), 2);
    assert!(catalog.preset(plain).is_ok());
}

#[test]
fn a_revision_remembers_which_preset_made_it_and_which_version() {
    use leyline_catalog::NewAsset;
    use leyline_core::{MediaType, Settings};

    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let folder = catalog.ensure_folder("Photos").unwrap();
    let add = |catalog: &mut Catalog, n: u8| {
        catalog
            .add_asset(
                &NewAsset {
                    folder,
                    filename: format!("IMG_{n}.CR3"),
                    extension: "CR3".to_owned(),
                    media_type: MediaType::Raw,
                    file_size: 1,
                    checksum: [n; 32],
                    width: None,
                    height: None,
                    capture_date: None,
                    capture_offset_minutes: None,
                },
                &Settings::default(),
            )
            .unwrap()
            .version
    };
    let one = add(&mut catalog, 1);
    let two = add(&mut catalog, 2);
    let by_hand = add(&mut catalog, 3);

    let gold = catalog.create_preset("Kodak Gold", "{}").unwrap();
    let warm = Settings {
        exposure: 0.3,
        ..Settings::default()
    };
    catalog
        .commit_revision_from(one, &warm, Some((gold, 1)))
        .unwrap();
    catalog
        .commit_revision_from(two, &warm, Some((gold, 1)))
        .unwrap();
    catalog.commit_revision(by_hand, &warm).unwrap();

    // The question ADR 0058 exists for: who came from this preset, and with
    // which version of it — the photo edited by hand is not in the answer.
    assert_eq!(
        catalog.versions_from_preset(gold).unwrap(),
        vec![(one, 1), (two, 1)]
    );

    // Updating the preset changes nothing already written: the two photos
    // still say version 1, which is exactly how "outdated" becomes visible.
    assert_eq!(
        catalog.update_preset(gold, "{\"exposure\":0.6}").unwrap(),
        2
    );
    assert_eq!(
        catalog.versions_from_preset(gold).unwrap(),
        vec![(one, 1), (two, 1)]
    );

    // Re-applying is an ordinary revision, and it moves that photo forward.
    catalog
        .commit_revision_from(one, &warm, Some((gold, 2)))
        .unwrap();
    assert_eq!(
        catalog.versions_from_preset(gold).unwrap(),
        vec![(one, 2), (two, 1)]
    );

    // A hand edit on top takes the photo out of the preset's flock: it is no
    // longer "a photo of this preset", and re-applying would discard work.
    catalog.commit_revision(two, &warm).unwrap();
    assert_eq!(catalog.versions_from_preset(gold).unwrap(), vec![(one, 2)]);

    // Deleting the preset never touches the revisions it produced.
    catalog.delete_preset(gold).unwrap();
    assert!(catalog.versions_from_preset(gold).unwrap().is_empty());
    assert_eq!(catalog.grid(&Default::default()).unwrap().len(), 3);
}
