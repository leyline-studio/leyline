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

/// ADR 0077 §4: a migration is irreversible and `open` refuses a catalog
/// newer than the build, so an update that migrates a library locks the
/// previous Leyline out of it. The snapshot in `Backups/` is the way back.
#[test]
fn a_pending_migration_snapshots_the_catalog_into_backups_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_catalog_path(&dir);

    // A database at `user_version` 0: every migration is still pending, which
    // is exactly the shape an older library has when a newer build opens it.
    //
    // The writer is deliberately **kept open** across `Catalog::open`. Closing
    // it would checkpoint the WAL into `catalog.db` and make a naive file copy
    // look correct; held open, the canary lives only in `catalog.db-wal`, so
    // this test fails for any backup that copies the one file.
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE canary (note TEXT);
             INSERT INTO canary VALUES ('here');",
        )
        .unwrap();
    let marker = "here";

    let catalog = Catalog::open(&path).unwrap();
    drop(writer);
    assert_eq!(catalog.user_version().unwrap(), Catalog::SCHEMA_VERSION);

    let snapshot = dir.path().join("Backups/catalog-schema-0.db");
    assert!(
        snapshot.is_file(),
        "no snapshot was written before migrating"
    );

    // It has to be a usable database still at the old version — not an empty
    // file, and not the migrated one.
    let restored = rusqlite::Connection::open(&snapshot).unwrap();
    assert_eq!(
        restored
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0,
        "the snapshot was taken after migrating, not before"
    );
    assert_eq!(
        restored
            .query_row("SELECT note FROM canary", [], |r| r.get::<_, String>(0))
            .unwrap(),
        marker,
        "the snapshot lost the content it exists to preserve"
    );
}

/// Opening an up-to-date library is the common case, and it must not drop a
/// copy of the catalog on every launch.
#[test]
fn opening_an_up_to_date_catalog_writes_no_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_catalog_path(&dir);
    drop(Catalog::create(&path, "Current").unwrap());

    drop(Catalog::open(&path).unwrap());

    let backups = dir.path().join("Backups");
    let count = std::fs::read_dir(&backups)
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(count, 0, "an up-to-date open left something in Backups/");
}

/// The one rule that makes the snapshot worth anything: if it cannot be
/// written, the migration does not happen either.
#[test]
fn a_failed_snapshot_refuses_the_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = temp_catalog_path(&dir);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE canary (note TEXT);")
            .unwrap();
    }
    // `Backups` occupied by a regular file: the directory cannot be created.
    std::fs::write(dir.path().join("Backups"), b"not a directory").unwrap();

    assert!(
        Catalog::open(&path).is_err(),
        "migrated irreversibly without a snapshot"
    );
    // And nothing was migrated behind the refusal.
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0
    );
}

/// Every `ON DELETE CASCADE` key must be indexed **on the referencing side**
/// (§32). SQLite enforces a cascade by looking up the children that point at
/// the deleted parent: with no index on that column, the lookup scans the
/// whole child table once per deleted row. Nothing fails, nothing warns — the
/// delete just goes quadratic, which is how `develop_current(version_id)` and
/// `export_history(asset_id)` stayed uncovered until they were measured.
///
/// Written generically on purpose: the next cascading key added to the schema
/// is checked by this test the day it appears.
#[test]
fn every_cascading_foreign_key_is_indexed() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::create(&temp_catalog_path(&dir), "Cascades").unwrap();
    let conn = catalog.connection();

    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master
              WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    let mut checked = 0;
    let mut uncovered = Vec::new();
    for table in &tables {
        // (id, seq, referencing column, on_delete): a composite key spans
        // several rows sharing an id, and only its first column can lead an
        // index — so seq 0 is the one that decides.
        let keys: Vec<(i64, i64, String, String)> = conn
            .prepare(&format!("PRAGMA foreign_key_list({table})"))
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(3)?, row.get(6)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        for (_, _, column, _) in keys
            .iter()
            .filter(|(_, seq, _, on_delete)| *seq == 0 && on_delete.eq_ignore_ascii_case("CASCADE"))
        {
            checked += 1;
            if !leads_an_index(conn, table, column) && !is_rowid_alias(conn, table, column) {
                uncovered.push(format!("{table}({column})"));
            }
        }
    }
    assert!(
        uncovered.is_empty(),
        "these cascading keys lead no index, so deleting a parent row scans \
         their table whole: {}",
        uncovered.join(", ")
    );
    // A guard that checked nothing would pass just as quietly.
    assert!(checked >= 10, "only {checked} cascading keys found");
}

/// Whether `column` is the first column of some index on `table` — including
/// the automatic index behind a `UNIQUE` constraint.
fn leads_an_index(conn: &rusqlite::Connection, table: &str, column: &str) -> bool {
    let indexes: Vec<String> = conn
        .prepare(&format!("PRAGMA index_list({table})"))
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    indexes.iter().any(|index| {
        conn.query_row(
            &format!("SELECT name FROM pragma_index_info('{index}') WHERE seqno = 0"),
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|first| first == column)
        .unwrap_or(false)
    })
}

/// Whether `column` is the `INTEGER PRIMARY KEY`, i.e. the rowid itself: it
/// needs no index because the table *is* that b-tree.
fn is_rowid_alias(conn: &rusqlite::Connection, table: &str, column: &str) -> bool {
    let pk: Vec<(String, String)> = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .unwrap()
        .filter_map(|row| {
            let (name, kind, pk) = row.unwrap();
            (pk == 1).then_some((name, kind))
        })
        .collect();

    matches!(pk.as_slice(), [(name, kind)] if name == column && kind.eq_ignore_ascii_case("INTEGER"))
}
