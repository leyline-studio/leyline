//! Integration tests: the camera-profile facade (`docs/adr/0035-camera-profile-dcp.md`).

use leyline_engine::Library;

fn open_test_library(name: &str) -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), name).unwrap();
    (dir, library)
}

/// The bytes' content is irrelevant here: import and listing never parse a
/// profile, they only copy and checksum it (parsing happens at render time,
/// covered by `camera_profile::resolve_from_settings`'s own tests).
fn sample_profile(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn without_an_import_the_profile_list_is_empty() {
    let (_dir, library) = open_test_library("ProfilesNone");
    assert_eq!(library.camera_profiles().unwrap(), Vec::new());
}

#[test]
fn an_imported_profile_is_copied_under_its_own_name_and_checksummed() {
    let (dir, library) = open_test_library("ProfilesImport");
    let source = sample_profile(dir.path(), "Canon 60D.dcp", b"profile bytes");

    let imported = library.import_camera_profile(&source).unwrap();

    assert_eq!(imported.relative_path, "Profiles/Camera/Canon 60D.dcp");
    assert_eq!(
        imported.checksum,
        format!("blake3:{}", blake3::hash(b"profile bytes").to_hex())
    );
    let copied = library.root().join("Profiles/Camera/Canon 60D.dcp");
    assert_eq!(std::fs::read(&copied).unwrap(), b"profile bytes");
    // The source is referenced by copy, never moved or modified.
    assert!(source.is_file());
}

#[test]
fn reimporting_the_same_name_is_refused_not_overwritten() {
    // Overwriting would silently change what every revision already
    // referencing that path renders to — the checksum would then mismatch
    // and those revisions would fail closed.
    let (dir, library) = open_test_library("ProfilesNoOverwrite");
    let first = sample_profile(dir.path(), "Canon 60D.dcp", b"first");
    library.import_camera_profile(&first).unwrap();

    let second = sample_profile(&dir.path().join("other"), "Canon 60D.dcp", b"second");
    let error = library.import_camera_profile(&second).unwrap_err();

    assert!(
        error.to_string().contains("never overwritten"),
        "unexpected error: {error}"
    );
    let copied = library.root().join("Profiles/Camera/Canon 60D.dcp");
    assert_eq!(std::fs::read(&copied).unwrap(), b"first");
}

#[test]
fn the_profile_list_is_sorted_and_ignores_non_dcp_files() {
    let (dir, library) = open_test_library("ProfilesList");
    for name in ["Zeta.dcp", "Alpha.dcp"] {
        let source = sample_profile(dir.path(), name, name.as_bytes());
        library.import_camera_profile(&source).unwrap();
    }
    std::fs::write(
        library.root().join("Profiles/Camera/readme.txt"),
        b"not a profile",
    )
    .unwrap();

    let profiles = library.camera_profiles().unwrap();

    let paths: Vec<_> = profiles.iter().map(|p| p.relative_path.as_str()).collect();
    assert_eq!(
        paths,
        ["Profiles/Camera/Alpha.dcp", "Profiles/Camera/Zeta.dcp"]
    );
    assert_eq!(
        profiles[0].checksum,
        format!("blake3:{}", blake3::hash(b"Alpha.dcp").to_hex())
    );
}

#[test]
fn the_listed_checksum_tracks_the_file_on_disk() {
    // A profile edited behind Leyline's back must list its *current*
    // checksum, so a client re-referencing it stores the truth rather than
    // a stale value that would fail closed at render time.
    let (dir, library) = open_test_library("ProfilesChecksum");
    let source = sample_profile(dir.path(), "Canon 60D.dcp", b"before");
    library.import_camera_profile(&source).unwrap();

    std::fs::write(
        library.root().join("Profiles/Camera/Canon 60D.dcp"),
        b"after",
    )
    .unwrap();

    assert_eq!(
        library.camera_profiles().unwrap()[0].checksum,
        format!("blake3:{}", blake3::hash(b"after").to_hex())
    );
}
