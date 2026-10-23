//! Integration test: a library left by Leyline 0.1.0 opens in this build
//! (`docs/catalog.md` §34, ADR 0077 §4).
//!
//! `catalog.rs` proves the snapshot mechanism on an empty database at
//! version 0. What it cannot prove is the path a real user takes: a populated
//! catalog at the schema the public release shipped, migrated forward with its
//! versions, history, collections and exports intact. The fixture is that
//! catalog, frozen as SQL text.

use std::path::Path;

use leyline_catalog::Catalog;
use leyline_core::{AssetId, CollectionId, RevisionId, Settings, VersionId};

/// The schema version tagged `v0.1.0`.
const RELEASE_0_1_0_SCHEMA: u32 = 13;

const FIXTURE: &str = include_str!("fixtures/catalog-0.1.0.sql");

/// Writes the fixture as `<library>/catalog.db`, closed, as an installed
/// 0.1.0 leaves it on disk.
fn library_from_0_1_0(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("catalog.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(FIXTURE).unwrap();
    path
}

fn user_version(path: &Path) -> u32 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_0_1_0_library_migrates_with_its_work_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = library_from_0_1_0(dir.path());
    assert_eq!(user_version(&path), RELEASE_0_1_0_SCHEMA);

    let catalog = Catalog::open(&path).unwrap();
    assert_eq!(catalog.user_version().unwrap(), Catalog::SCHEMA_VERSION);

    let raw = AssetId::new(1);
    let versions = catalog.versions(raw).unwrap();
    let names: Vec<&str> = versions.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["Master", "Copy"]);
    assert_eq!(versions[0].head, RevisionId::new(2));
    assert_eq!(versions[0].rating, Some(4));
    assert_eq!(catalog.current_version(raw).unwrap(), VersionId::new(1));

    // History comes through verbatim, and a revision written before origins
    // existed (ADR 0154 §6) declares none rather than inventing one.
    let history = catalog.version_history(VersionId::new(1)).unwrap();
    let chain: Vec<RevisionId> = history.iter().map(|r| r.revision).collect();
    assert_eq!(chain, [RevisionId::new(2), RevisionId::new(1)]);
    assert!(history.iter().all(|r| r.origin.is_none()));
    assert!(history.iter().all(|r| r.settings_json == "{}"));

    assert_eq!(
        catalog.collection_versions(CollectionId::new(1)).unwrap(),
        [VersionId::new(1)]
    );

    // An export recorded before provenance (ADR 0153) keeps what it had and
    // claims nothing it never measured.
    let exports = catalog.export_history(raw).unwrap();
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].destination, "/exports/IMG_0001.jpg");
    assert!(exports[0].revision.is_none());
    assert!(exports[0].revision_fingerprint.is_none());
    assert!(exports[0].output_checksum.is_none());
}

#[test]
fn a_migrated_0_1_0_library_keeps_a_restorable_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = library_from_0_1_0(dir.path());
    drop(Catalog::open(&path).unwrap());

    let snapshot = dir
        .path()
        .join(format!("Backups/catalog-schema-{RELEASE_0_1_0_SCHEMA}.db"));
    assert_eq!(user_version(&snapshot), RELEASE_0_1_0_SCHEMA);

    // Restoring means 0.1.0 can open it again: the very rows it wrote.
    let restored = rusqlite::Connection::open(&snapshot).unwrap();
    let revisions: i64 = restored
        .query_row("SELECT COUNT(*) FROM develop_revisions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(revisions, 3);
}

#[test]
fn a_migrated_0_1_0_library_takes_new_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = library_from_0_1_0(dir.path());
    let mut catalog = Catalog::open(&path).unwrap();

    let settings = Settings {
        exposure: 0.5,
        ..Settings::default()
    };
    let revision = catalog
        .commit_revision(VersionId::new(1), &settings)
        .unwrap();

    let history = catalog.version_history(VersionId::new(1)).unwrap();
    assert_eq!(history[0].revision, revision);
    assert_eq!(history[0].parent, Some(RevisionId::new(2)));
}
