//! Integration tests: catalog creation, opening, configuration and schema.

use std::path::PathBuf;

use leyline_catalog::Catalog;
use leyline_core::LeylineError;

fn temp_catalog_path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("catalog.db")
}

#[test]
fn create_produces_a_configured_migrated_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_catalog_path(&dir);

    let catalog = Catalog::create(&path, "Wildlife").unwrap();

    assert!(path.is_file());
    assert_eq!(catalog.user_version().unwrap(), Catalog::SCHEMA_VERSION);
    assert!(!catalog.is_read_only());

    let library = catalog.library().unwrap();
    assert_eq!(library.name, "Wildlife");
    assert!(!library.uuid.is_empty());
    assert!(library.created_at > 0);
    assert_eq!(library.created_at, library.updated_at);

    let count: i64 = catalog
        .connection()
        .query_row("SELECT COUNT(*) FROM library", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);

    let journal_mode: String = catalog
        .connection()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal_mode, "wal");

    let foreign_keys: i64 = catalog
        .connection()
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(foreign_keys, 1);
}

#[test]
fn create_refuses_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_catalog_path(&dir);

    Catalog::create(&path, "First").unwrap();
    assert!(matches!(
        Catalog::create(&path, "Second"),
        Err(LeylineError::Io(_))
    ));
}

#[test]
fn open_requires_an_existing_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_catalog_path(&dir);

    match Catalog::open(&path) {
        Err(LeylineError::LibraryNotFound(p)) => assert_eq!(p, path),
        other => panic!("expected LibraryNotFound, got {other:?}"),
    }
}

#[test]
fn open_reopens_a_created_catalog_idempotently() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_catalog_path(&dir);

    let uuid = {
        let catalog = Catalog::create(&path, "Reopen").unwrap();
        catalog.library().unwrap().uuid
    };

    // Reopening applies no migration (already at head) and loses nothing.
    let catalog = Catalog::open(&path).unwrap();
    assert_eq!(catalog.user_version().unwrap(), Catalog::SCHEMA_VERSION);
    assert_eq!(catalog.library().unwrap().uuid, uuid);
}

#[test]
fn newer_catalog_is_refused_for_writing_but_readable() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_catalog_path(&dir);

    {
        let catalog = Catalog::create(&path, "Future").unwrap();
        catalog
            .connection()
            .pragma_update(None, "user_version", 999)
            .unwrap();
    }

    match Catalog::open(&path) {
        Err(LeylineError::NewerCatalog { found, supported }) => {
            assert_eq!(found, 999);
            assert_eq!(supported, Catalog::SCHEMA_VERSION);
        }
        other => panic!("expected NewerCatalog, got {other:?}"),
    }

    let catalog = Catalog::open_read_only(&path).unwrap();
    assert!(catalog.is_read_only());
    assert_eq!(catalog.user_version().unwrap(), 999);
    assert_eq!(catalog.library().unwrap().name, "Future");

    // The read-only handle cannot write anything.
    assert!(
        catalog
            .connection()
            .execute("UPDATE library SET name = 'hacked'", [])
            .is_err()
    );
}

#[test]
fn foreign_keys_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::create(&temp_catalog_path(&dir), "FK").unwrap();

    // An asset referencing a non-existent folder must be rejected.
    let orphan = catalog.connection().execute(
        "INSERT INTO assets (uuid, folder_id, filename, extension, media_type,
                             file_size, checksum, imported_at, modified_at)
         VALUES ('u1', 42, 'IMG_0001.CR3', 'CR3', 0, 1, x'00', 0, 0)",
        [],
    );
    assert!(orphan.is_err());
}

#[test]
fn metadata_generated_columns_are_computed() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::create(&temp_catalog_path(&dir), "Generated").unwrap();
    let conn = catalog.connection();

    conn.execute_batch(
        "INSERT INTO folders (relative_path, created_at) VALUES ('Photos', 0);
         INSERT INTO assets (uuid, folder_id, filename, extension, media_type,
                             file_size, checksum, imported_at, modified_at)
         VALUES ('u1', 1, 'IMG_0001.CR3', 'CR3', 0, 1, x'00', 0, 0);
         INSERT INTO metadata (asset_id, shutter_numerator, shutter_denominator,
                               aperture_numerator, aperture_denominator,
                               focal_length_numerator, focal_length_denominator)
         VALUES (1, 1, 3200, 56, 10, 70, 1);",
    )
    .unwrap();

    let (shutter, aperture, focal): (f64, f64, f64) = conn
        .query_row(
            "SELECT shutter_speed_s, aperture_f, focal_length_mm FROM metadata WHERE asset_id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!((shutter - 1.0 / 3200.0).abs() < 1e-9);
    assert!((aperture - 5.6).abs() < 1e-9);
    assert!((focal - 70.0).abs() < 1e-9);
}

#[test]
fn full_text_search_ignores_diacritics() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::create(&temp_catalog_path(&dir), "FTS").unwrap();
    let conn = catalog.connection();

    conn.execute(
        "INSERT INTO search_index (asset_id, filename, keywords, artist, copyright)
         VALUES (1, 'IMG_0001.CR3', 'Nature/Birds/Héron', 'Quentin', '')",
        [],
    )
    .unwrap();

    let found: i64 = conn
        .query_row(
            "SELECT asset_id FROM search_index WHERE search_index MATCH 'heron'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(found, 1);
}

#[test]
fn rating_check_constraint_rejects_zero() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::create(&temp_catalog_path(&dir), "Rating").unwrap();
    let conn = catalog.connection();

    conn.execute_batch(
        "INSERT INTO folders (relative_path, created_at) VALUES ('Photos', 0);
         INSERT INTO assets (uuid, folder_id, filename, extension, media_type,
                             file_size, checksum, imported_at, modified_at)
         VALUES ('u1', 1, 'IMG_0001.CR3', 'CR3', 0, 1, x'00', 0, 0);
         INSERT INTO develop_revisions (asset_id, settings_json, created_at)
         VALUES (1, '{}', 0);",
    )
    .unwrap();

    // rating 0 does not exist: NULL = unrated, otherwise 1..=5 (catalog.md §18).
    let zero = conn.execute(
        "INSERT INTO develop_versions (uuid, asset_id, name, head_revision_id, rating, created_at)
         VALUES ('v1', 1, 'Default', 1, 0, 0)",
        [],
    );
    assert!(zero.is_err());

    conn.execute(
        "INSERT INTO develop_versions (uuid, asset_id, name, head_revision_id, rating, created_at)
         VALUES ('v1', 1, 'Default', 1, 5, 0)",
        [],
    )
    .unwrap();
}
