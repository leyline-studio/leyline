//! Integration tests: the grid query (`docs/engine-api.md` §7).

use leyline_catalog::{Catalog, GridQuery, NewAsset, RegisteredAsset, Sort};
use leyline_core::{ColorLabel, LeylineError, MediaType, PickState, VersionId};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Grid").unwrap()
}

fn add(
    catalog: &mut Catalog,
    folder: &str,
    filename: &str,
    capture_date: Option<i64>,
) -> RegisteredAsset {
    let new = NewAsset {
        folder: catalog.ensure_folder(folder).unwrap(),
        filename: filename.to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xAB; 32],
        width: Some(6000),
        height: Some(4000),
        capture_date,
        capture_offset_minutes: None,
    };
    catalog.add_asset(&new).unwrap()
}

/// Three assets: two birds in Wildlife (rated 5 and 3), one street shot.
fn seeded(catalog: &mut Catalog) -> [RegisteredAsset; 3] {
    let heron = add(catalog, "Wildlife", "heron.CR3", Some(3_000));
    let eagle = add(catalog, "Wildlife", "eagle.CR3", Some(1_000));
    let street = add(catalog, "Street", "street.CR3", Some(2_000));

    catalog.set_rating(&[heron.version], Some(5)).unwrap();
    catalog.set_rating(&[eagle.version], Some(3)).unwrap();
    catalog
        .set_color_label(&[street.version], Some(ColorLabel::Red))
        .unwrap();
    catalog.set_pick(&[heron.version], PickState::Pick).unwrap();

    let birds = catalog.create_keyword(None, "Birds").unwrap();
    let heron_kw = catalog.create_keyword(Some(birds), "Heron").unwrap();
    catalog.add_keyword(&[heron.asset], heron_kw).unwrap();
    catalog.add_keyword(&[eagle.asset], birds).unwrap();

    [heron, eagle, street]
}

fn versions(items: &[leyline_catalog::GridItem]) -> Vec<VersionId> {
    items.iter().map(|i| i.version_id).collect()
}

#[test]
fn default_query_lists_current_versions_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, street] = seeded(&mut catalog);

    let query = GridQuery::default();
    assert_eq!(catalog.count(&query).unwrap(), 3);
    let items = catalog.grid(&query).unwrap();
    assert_eq!(
        versions(&items),
        vec![heron.version, street.version, eagle.version]
    );

    let heron_item = &items[0];
    assert_eq!(heron_item.filename, "heron.CR3");
    assert_eq!(heron_item.rating, Some(5));
    assert_eq!(heron_item.pick, PickState::Pick);
    assert_eq!(heron_item.width, Some(6000));
}

#[test]
fn filters_compose_with_and_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, street] = seeded(&mut catalog);

    let wildlife = catalog.ensure_folder("Wildlife").unwrap();
    let by_folder = GridQuery {
        folder: Some(wildlife),
        ..GridQuery::default()
    };
    assert_eq!(catalog.count(&by_folder).unwrap(), 2);

    let rated_in_folder = GridQuery {
        rating_at_least: Some(4),
        ..by_folder.clone()
    };
    assert_eq!(
        versions(&catalog.grid(&rated_in_folder).unwrap()),
        vec![heron.version]
    );

    let labeled = GridQuery {
        color_label: Some(ColorLabel::Red),
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&labeled).unwrap()),
        vec![street.version]
    );

    let picked = GridQuery {
        pick: Some(PickState::Pick),
        ..GridQuery::default()
    };
    assert_eq!(catalog.count(&picked).unwrap(), 1);

    let captured_early = GridQuery {
        capture_range: Some((0, 2_000)),
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&captured_early).unwrap()),
        vec![street.version, eagle.version]
    );
}

#[test]
fn keyword_filter_includes_descendants() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, _] = seeded(&mut catalog);

    // "Birds" matches the eagle (tagged Birds) and the heron (tagged
    // Birds/Heron, a descendant).
    let birds = catalog.keyword_tree().unwrap()[0].keyword;
    let by_keyword = GridQuery {
        keywords: vec![birds],
        sort: Sort::Filename { ascending: true },
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&by_keyword).unwrap()),
        vec![eagle.version, heron.version]
    );
}

#[test]
fn text_filter_uses_the_search_index() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, ..] = seeded(&mut catalog);

    let by_text = GridQuery {
        text: Some("her".to_owned()),
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&by_text).unwrap()),
        vec![heron.version]
    );

    // Blank text filters nothing.
    let blank = GridQuery {
        text: Some("   ".to_owned()),
        ..GridQuery::default()
    };
    assert_eq!(catalog.count(&blank).unwrap(), 3);
}

#[test]
fn collection_queries_enumerate_members_in_user_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, street] = seeded(&mut catalog);

    let album = catalog.create_collection(None, "Album").unwrap();
    catalog
        .add_to_collection(album, &[street.version, heron.version, eagle.version])
        .unwrap();

    let in_album = GridQuery {
        collection: Some(album),
        sort: Sort::CollectionOrder,
        ..GridQuery::default()
    };
    assert_eq!(catalog.count(&in_album).unwrap(), 3);
    assert_eq!(
        versions(&catalog.grid(&in_album).unwrap()),
        vec![street.version, heron.version, eagle.version]
    );

    // Collection order without a collection is meaningless.
    let bad = GridQuery {
        sort: Sort::CollectionOrder,
        ..GridQuery::default()
    };
    assert!(matches!(catalog.grid(&bad), Err(LeylineError::Db(_))));
}

#[test]
fn sorts_are_stable_and_handle_null_last() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, street] = seeded(&mut catalog);
    let undated = add(&mut catalog, "Street", "undated.CR3", None);

    let by_date = GridQuery {
        sort: Sort::CaptureDate { ascending: true },
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&by_date).unwrap()),
        vec![
            eagle.version,
            street.version,
            heron.version,
            undated.version
        ]
    );

    // Unrated versions come last in both directions.
    let by_rating = GridQuery {
        sort: Sort::Rating { ascending: false },
        ..GridQuery::default()
    };
    let items = versions(&catalog.grid(&by_rating).unwrap());
    assert_eq!(items[0], heron.version);
    assert_eq!(items[1], eagle.version);
    assert_eq!(items.len(), 4);
}

#[test]
fn range_paginates_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, _, street] = seeded(&mut catalog);

    let window = GridQuery {
        range: 0..2,
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&window).unwrap()),
        vec![heron.version, street.version]
    );
    // The count ignores the window.
    assert_eq!(catalog.count(&window).unwrap(), 3);

    let next = GridQuery {
        range: 2..4,
        ..GridQuery::default()
    };
    assert_eq!(catalog.grid(&next).unwrap().len(), 1);

    let empty = GridQuery {
        range: 2..2,
        ..GridQuery::default()
    };
    assert_eq!(catalog.grid(&empty).unwrap(), vec![]);
}

#[test]
fn grid_shows_the_current_version_not_all_versions() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, ..] = seeded(&mut catalog);

    let bw = catalog
        .create_version(heron.version, "Noir & Blanc", None)
        .unwrap();
    assert_eq!(catalog.count(&GridQuery::default()).unwrap(), 3);

    catalog.set_current_version(heron.asset, bw).unwrap();
    let items = catalog.grid(&GridQuery::default()).unwrap();
    assert!(versions(&items).contains(&bw));
    assert!(!versions(&items).contains(&heron.version));
}
