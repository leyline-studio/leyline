//! Integration tests: full-text search maintenance (`docs/catalog.md` §30).

use leyline_catalog::{Catalog, NewAsset, RegisteredAsset};
use leyline_core::MediaType;

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Search").unwrap()
}

fn registered_asset(catalog: &mut Catalog, filename: &str) -> RegisteredAsset {
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: filename.to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xAB; 32],
        width: None,
        height: None,
        capture_date: None,
        capture_offset_minutes: None,
    };
    catalog.add_asset(&new).unwrap()
}

#[test]
fn registration_indexes_the_filename() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let a = registered_asset(&mut catalog, "heron-sunrise.CR3");
    registered_asset(&mut catalog, "IMG_0002.CR3");

    assert_eq!(catalog.search_assets("heron").unwrap(), vec![a.asset]);
    // Prefix match while typing.
    assert_eq!(catalog.search_assets("sunri").unwrap(), vec![a.asset]);
    assert_eq!(catalog.search_assets("owl").unwrap(), vec![]);
    assert_eq!(catalog.search_assets("   ").unwrap(), vec![]);
}

#[test]
fn keyword_changes_keep_the_index_in_sync() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let a = registered_asset(&mut catalog, "IMG_0001.CR3");

    let nature = catalog.create_keyword(None, "Nature").unwrap();
    let birds = catalog.create_keyword(Some(nature), "Birds").unwrap();
    let heron = catalog.create_keyword(Some(birds), "Héron").unwrap();

    catalog.add_keyword(&[a.asset], heron).unwrap();
    // Every level of the path matches, diacritics ignored.
    for term in ["nature", "birds", "heron", "héron"] {
        assert_eq!(
            catalog.search_assets(term).unwrap(),
            vec![a.asset],
            "term {term:?} should match"
        );
    }

    catalog.remove_keyword(&[a.asset], heron).unwrap();
    assert_eq!(catalog.search_assets("heron").unwrap(), vec![]);
}

#[test]
fn operators_cannot_be_injected() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    registered_asset(&mut catalog, "IMG_0001.CR3");

    // Raw FTS syntax would error or match everything; quoted terms just
    // return nothing.
    for hostile in ["img OR heron", "NOT img", "\"img*\" AND x", "col:img"] {
        assert!(catalog.search_assets(hostile).is_ok(), "query {hostile:?}");
    }
    // Multiple terms are all required.
    assert_eq!(catalog.search_assets("img 0001").unwrap().len(), 1);
    assert_eq!(catalog.search_assets("img heron").unwrap(), vec![]);
}

#[test]
fn rebuild_regenerates_from_source_tables() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let a = registered_asset(&mut catalog, "heron.CR3");
    let heron = catalog.create_keyword(None, "Heron").unwrap();
    catalog.add_keyword(&[a.asset], heron).unwrap();

    // Wreck the cache, then rebuild it.
    catalog
        .connection()
        .execute("DELETE FROM search_index", [])
        .unwrap();
    assert_eq!(catalog.search_assets("heron").unwrap(), vec![]);

    catalog.rebuild_search_index().unwrap();
    assert_eq!(catalog.search_assets("heron").unwrap(), vec![a.asset]);
}
