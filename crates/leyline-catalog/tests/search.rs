//! Integration tests: full-text search maintenance (`docs/catalog.md` §30).

use leyline_catalog::{Catalog, NewAsset, RegisteredAsset};
use leyline_core::MediaType;
use leyline_core::Settings;

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
    catalog.add_asset(&new, &Settings::default()).unwrap()
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

/// ADR 0144 §1: what a photographer typed about a photograph is searchable.
#[test]
fn the_title_and_the_caption_are_searchable() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let a = registered_asset(&mut catalog, "IMG_0001.CR3");
    registered_asset(&mut catalog, "IMG_0002.CR3");

    catalog
        .set_description(
            a.asset,
            &leyline_catalog::AssetDescription {
                title: Some("Héron au lever".to_owned()),
                caption: Some("Marais salants, à contre-jour".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();

    // Diacritics ignored, prefix while typing, both fields indexed.
    assert_eq!(catalog.search_assets("heron").unwrap(), vec![a.asset]);
    assert_eq!(catalog.search_assets("marais").unwrap(), vec![a.asset]);
    assert_eq!(catalog.search_assets("contre").unwrap(), vec![a.asset]);
    assert_eq!(catalog.search_assets("mouette").unwrap(), vec![]);

    // Clearing the description clears the index with it: the row is a
    // cache of the tables, never a memory of what they used to hold.
    catalog
        .set_description(a.asset, &leyline_catalog::AssetDescription::default())
        .unwrap();
    assert_eq!(catalog.search_assets("heron").unwrap(), vec![]);
}

/// ADR 0144 §2: the body, the lens and the day the file carries — the three
/// facts the shot filters knew and the search box did not.
#[test]
fn the_camera_the_lens_and_the_day_are_searchable() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let folder = catalog.ensure_folder("Photos").unwrap();
    let new = NewAsset {
        folder,
        filename: "IMG_0003.CR3".to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xCD; 32],
        width: None,
        height: None,
        // 2023-05-16 12:00 UTC, shot two hours east of it.
        capture_date: Some(1_684_238_400_000),
        capture_offset_minutes: Some(120),
    };
    let a = catalog.add_asset(&new, &Settings::default()).unwrap();
    registered_asset(&mut catalog, "IMG_0004.CR3");

    catalog
        .set_metadata(
            a.asset,
            &leyline_catalog::Metadata {
                camera: Some(leyline_catalog::CameraInfo {
                    manufacturer: "Canon".to_owned(),
                    model: "EOS 60D".to_owned(),
                }),
                lens: Some(leyline_catalog::LensInfo {
                    manufacturer: "Canon".to_owned(),
                    model: "EF-S18-55mm f/3.5-5.6 IS II".to_owned(),
                    mount: None,
                }),
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(catalog.search_assets("canon").unwrap(), vec![a.asset]);
    assert_eq!(catalog.search_assets("60D").unwrap(), vec![a.asset]);
    // The lens as its own name is tokenised: « EF-S18-55mm » gives `ef`,
    // `s18`, `55mm`. So « 55mm » finds it and « 18 » does not — the shot
    // filter's chips are what one picks a lens from (ADR 0064), and the
    // search box is for the word one remembers.
    assert_eq!(catalog.search_assets("55mm").unwrap(), vec![a.asset]);
    assert_eq!(catalog.search_assets("18").unwrap(), vec![]);
    // The day, as the photographer would type it — local, offset included.
    assert_eq!(catalog.search_assets("2023").unwrap(), vec![a.asset]);
    assert_eq!(catalog.search_assets("2023-05-16").unwrap(), vec![a.asset]);
    assert_eq!(catalog.search_assets("nikon").unwrap(), vec![]);
}
