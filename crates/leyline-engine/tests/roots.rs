//! A catalog can span several volumes ([ADR 0085](../../../docs/adr/0085-named-roots.md)).
//!
//! A second root cannot be a second temporary *volume* in a test, so what is
//! simulated is the thing that actually matters: a folder outside the library
//! tree, reached only through its marker and a hint — which is exactly the
//! mechanism a second disk would use, minus the disk.

use std::path::Path;

use leyline_engine::{ImportOptions, Library};

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn reference() -> ImportOptions {
    ImportOptions {
        copy_files: false,
        recursive: true,
        pair_companions: true,
        thumbnails: false,
    }
}

#[test]
fn a_new_library_is_root_one_and_carries_its_own_marker() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let library = Library::create(&root, "Test").unwrap();

    let roots = library.roots().unwrap();
    assert_eq!(roots.len(), 1, "a fresh library has exactly one root");
    assert_eq!(roots[0].root.id, 1);
    assert!(roots[0].is_online());
    assert_eq!(roots[0].location.as_deref(), Some(root.as_path()));

    // The marker is on disk and states the root's identity, so the folder can
    // be recognised from elsewhere — that is what makes it a root at all.
    let marker = std::fs::read_to_string(root.join(".leyline-root")).unwrap();
    assert_eq!(marker.trim(), roots[0].root.uuid);
}

#[test]
fn a_library_that_lost_its_marker_gets_it_back_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let uuid = {
        let library = Library::create(&root, "Test").unwrap();
        let uuid = library.roots().unwrap()[0].root.uuid.clone();
        library.close().unwrap();
        uuid
    };

    // A backup that dropped the dotfile, or a catalog from before ADR 0085.
    std::fs::remove_file(root.join(".leyline-root")).unwrap();

    let library = Library::open(&root).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(".leyline-root"))
            .unwrap()
            .trim(),
        uuid,
        "root 1's identity is derivable from the catalog, so it is rewritten"
    );
    assert!(library.roots().unwrap()[0].is_online());
}

#[test]
fn a_second_root_holds_photographs_the_library_folder_never_contains() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let archive = dir.path().join("Archive2019");
    write(&archive.join("Iceland/glacier.png"), b"outside the library");

    let library = Library::create(&root, "Test").unwrap();

    // Before the root exists, this is exactly the ADR 0010 refusal.
    let refused = library.import(&archive, &reference(), |_, _| {}).unwrap();
    assert!(refused.imported.is_empty());
    assert!(refused.skipped[0].reason.contains("outside every root"));

    let added = library.add_root(&archive, "Archive 2019").unwrap();
    assert_ne!(added.id, 1, "the library keeps root 1");

    let report = library.import(&archive, &reference(), |_, _| {}).unwrap();
    assert_eq!(report.imported.len(), 1);
    assert_eq!(report.imported[0].relative_path, "Iceland/glacier.png");

    // Nothing was copied: the file is still only in the archive.
    assert!(!root.join("Photos/Iceland/glacier.png").exists());
    assert_eq!(
        library.locate(report.imported[0].registered.asset).unwrap(),
        archive.canonicalize().unwrap().join("Iceland/glacier.png")
    );
}

#[test]
fn an_unplugged_root_is_offline_and_that_is_not_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let archive = dir.path().join("Archive2019");
    write(&archive.join("glacier.png"), b"outside the library");

    let library = Library::create(&root, "Test").unwrap();
    library.add_root(&archive, "Archive 2019").unwrap();
    let report = library.import(&archive, &reference(), |_, _| {}).unwrap();
    let asset = report.imported[0].registered.asset;

    // Unplug it: the marker goes away with the volume.
    std::fs::remove_file(archive.join(".leyline-root")).unwrap();

    let statuses = library.roots().unwrap();
    let offline = statuses.iter().find(|s| s.root.id != 1).unwrap();
    assert!(!offline.is_online());

    // Pixels are refused, and the error names the root rather than the file.
    let error = library.locate(asset).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("Archive 2019"),
        "the error must name the root, got: {message}"
    );
    assert!(matches!(
        error,
        leyline_core::LeylineError::RootOffline { .. }
    ));

    // The photograph is still catalogued, still browsable, still searchable:
    // an unplugged disk is not an edit (ADR 0085 §5).
    let catalog = library.catalog();
    assert_eq!(catalog.grid(&Default::default()).unwrap().len(), 1);
    // `folders()` counts only assets with `is_missing = 0`, so a non-zero
    // count here is the guarantee of §5 asserted through the public API:
    // going offline did not mark anything missing.
    let counted: u32 = catalog
        .folders()
        .unwrap()
        .iter()
        .map(|folder| folder.photo_count)
        .sum();
    assert_eq!(counted, 1, "offline is not missing");
}

#[test]
fn a_hint_is_never_trusted_without_its_marker() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let archive = dir.path().join("Archive2019");
    let impostor = dir.path().join("SomethingElse");
    std::fs::create_dir_all(&archive).unwrap();
    std::fs::create_dir_all(&impostor).unwrap();

    let library = Library::create(&root, "Test").unwrap();
    let added = library.add_root(&archive, "Archive 2019").unwrap();

    // Pointing the library at the wrong folder is refused, not remembered: a
    // remembered wrong answer resolves silently to someone else's photographs.
    let wrong = library.locate_root(&added.uuid, &impostor).unwrap_err();
    assert!(wrong.to_string().contains("is not that root"));

    // The real folder still verifies, wherever it has moved to.
    let moved = dir.path().join("Archive2019-moved");
    std::fs::rename(&archive, &moved).unwrap();
    library.locate_root(&added.uuid, &moved).unwrap();
    let status = library.roots().unwrap();
    let external = status.iter().find(|s| s.root.id == added.id).unwrap();
    assert_eq!(
        external.location.as_deref(),
        Some(moved.canonicalize().unwrap().as_path())
    );
}

#[test]
fn a_root_still_holding_photographs_cannot_be_forgotten() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let archive = dir.path().join("Archive2019");
    write(&archive.join("glacier.png"), b"outside the library");

    let library = Library::create(&root, "Test").unwrap();
    let added = library.add_root(&archive, "Archive 2019").unwrap();
    library.import(&archive, &reference(), |_, _| {}).unwrap();

    let refused = library.forget_root(&added.uuid).unwrap_err();
    assert!(
        refused.to_string().contains("still holds"),
        "unexpected: {refused}"
    );

    // The library's own root is never forgettable either.
    let library_uuid = library.roots().unwrap()[0].root.uuid.clone();
    assert!(library.forget_root(&library_uuid).is_err());
}

#[test]
fn adding_a_folder_that_is_already_a_root_keeps_its_identity() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("Archive2019");
    std::fs::create_dir_all(&archive).unwrap();

    let first = Library::create(&dir.path().join("A"), "A").unwrap();
    let added = first.add_root(&archive, "Archive 2019").unwrap();

    // A second library referencing the same archive must see the same root:
    // the marker belongs to the folder, and neither library owns it.
    let second = Library::create(&dir.path().join("B"), "B").unwrap();
    let again = second.add_root(&archive, "Whatever B calls it").unwrap();
    assert_eq!(again.uuid, added.uuid);
}
