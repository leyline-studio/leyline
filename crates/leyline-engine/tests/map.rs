//! Integration tests: the GPS map facade (`docs/adr/0040-gps-map-view.md`).

use leyline_core::LeylineError;
use leyline_engine::Library;
use rusqlite::Connection;

fn open_test_library(name: &str) -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), name).unwrap();
    (dir, library)
}

/// A minimal spec-compliant MBTiles file with one tile at `z=0`.
fn sample_pack(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE metadata (name TEXT, value TEXT);
         CREATE TABLE tiles (
             zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB
         );
         INSERT INTO metadata VALUES ('attribution', '© OpenStreetMap contributors');
         INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data)
         VALUES (0, 0, 0, X'010203');",
    )
    .unwrap();
}

#[cfg(not(feature = "bundled-basemap"))]
#[test]
fn without_a_pack_the_map_facade_returns_none_not_an_error() {
    let (_dir, library) = open_test_library("MapNoPack");
    assert_eq!(library.map_pack_info().unwrap(), None);
    assert_eq!(library.map_tile(0, 0, 0).unwrap(), None);
}

#[cfg(feature = "bundled-basemap")]
#[test]
fn without_a_pack_the_embedded_world_basemap_answers() {
    // ADR 0059: a build carrying the basemap has no empty map state — the
    // world is there before anything is imported.
    let (_dir, library) = open_test_library("MapBundled");
    let info = library.map_pack_info().unwrap().unwrap();
    assert_eq!(info.name.as_deref(), Some("Leyline world basemap"));
    assert_eq!(
        info.attribution.as_deref(),
        Some("Natural Earth (public domain)")
    );
    assert_eq!(info.min_zoom, Some(0));
    assert_eq!(info.max_zoom, Some(5));
    assert_eq!(info.format.as_deref(), Some("jpg"));
    // The whole world at z0 is a single tile, and every pack ships it.
    assert!(library.map_tile(0, 0, 0).unwrap().is_some());
    // Past the pack's own depth, still `None` rather than an error.
    assert_eq!(library.map_tile(9, 0, 0).unwrap(), None);
}

#[cfg(feature = "bundled-basemap")]
#[test]
fn an_imported_pack_outranks_the_embedded_basemap() {
    // The fallback is a fallback: the moment the user brings a pack, it is
    // the one served — the two are never composed (ADR 0059).
    let (dir, library) = open_test_library("MapBundledOverridden");
    let source = dir.path().join("region.mbtiles");
    sample_pack(&source);

    library.import_map_pack(&source).unwrap();

    assert_eq!(library.map_tile(0, 0, 0).unwrap(), Some(vec![1, 2, 3]));
    let info = library.map_pack_info().unwrap().unwrap();
    assert_ne!(info.name.as_deref(), Some("Leyline world basemap"));
}

#[test]
fn an_imported_pack_serves_tiles_and_info() {
    let (dir, library) = open_test_library("MapWithPack");
    let source = dir.path().join("region.mbtiles");
    sample_pack(&source);

    library.import_map_pack(&source).unwrap();

    let info = library.map_pack_info().unwrap().unwrap();
    assert_eq!(
        info.attribution.as_deref(),
        Some("© OpenStreetMap contributors")
    );
    assert_eq!(library.map_tile(0, 0, 0).unwrap(), Some(vec![1, 2, 3]));
    assert_eq!(library.map_tile(0, 5, 5).unwrap(), None);
}

#[test]
fn reimporting_a_pack_replaces_the_cached_handle() {
    // Regression guard for the `map_pack` cache: without invalidating it on
    // re-import, this would keep serving tiles from the first pack.
    let (dir, library) = open_test_library("MapReimport");
    let first = dir.path().join("first.mbtiles");
    sample_pack(&first);
    library.import_map_pack(&first).unwrap();
    assert_eq!(library.map_tile(0, 0, 0).unwrap(), Some(vec![1, 2, 3]));

    let second = dir.path().join("second.mbtiles");
    let conn = Connection::open(&second).unwrap();
    conn.execute_batch(
        "CREATE TABLE metadata (name TEXT, value TEXT);
         CREATE TABLE tiles (
             zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB
         );
         INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data)
         VALUES (0, 0, 0, X'0A0B0C');",
    )
    .unwrap();
    drop(conn);
    library.import_map_pack(&second).unwrap();

    assert_eq!(library.map_tile(0, 0, 0).unwrap(), Some(vec![10, 11, 12]));
}

#[test]
fn reimporting_leaves_no_temporary_file_behind() {
    // The pack is published by copy-to-temp then move, because
    // `std::fs::rename` will not replace an existing destination on
    // Windows. Whatever the platform, the temp file must not survive: a
    // stray `pack.mbtiles.*.tmp` in `Map/` is both clutter and a thing a
    // later import could trip over.
    let (dir, library) = open_test_library("MapReimportTemp");
    let first = dir.path().join("first.mbtiles");
    sample_pack(&first);
    library.import_map_pack(&first).unwrap();
    // Open the pack so the cached SQLite handle is live during the second
    // import — the case the ordering fix exists for.
    assert!(library.map_pack_info().unwrap().is_some());

    let second = dir.path().join("second.mbtiles");
    sample_pack(&second);
    library.import_map_pack(&second).unwrap();

    let map_dir = library.root().join("Map");
    let leftovers: Vec<_> = std::fs::read_dir(&map_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "left behind {leftovers:?}");
    assert!(map_dir.join("pack.mbtiles").is_file());
    assert_eq!(library.map_tile(0, 0, 0).unwrap(), Some(vec![1, 2, 3]));
}

#[test]
fn map_pins_reads_through_the_catalog() {
    let (_dir, library) = open_test_library("MapPins");
    assert_eq!(library.map_pins().unwrap(), vec![]);
}

#[test]
fn a_read_only_handle_refuses_to_import_a_pack() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("RO");
    drop(Library::create(&root, "RO").unwrap());

    let library = Library::open_read_only(&root).unwrap();
    let source = dir.path().join("region.mbtiles");
    sample_pack(&source);

    assert!(matches!(
        library.import_map_pack(&source),
        Err(LeylineError::Db(_))
    ));
    assert!(!root.join("Map").join("pack.mbtiles").exists());
}
