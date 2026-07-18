//! Integration tests: smart collections (`docs/catalog.md` §26).

use leyline_catalog::{
    CameraInfo, Catalog, GridQuery, Metadata, NewAsset, RatingRule, RegisteredAsset, SmartRules,
    Sort,
};
use leyline_core::{CollectionType, LeylineError, MediaType, PickState, VersionId};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Smart").unwrap()
}

fn add(catalog: &mut Catalog, filename: &str) -> RegisteredAsset {
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

fn versions(items: &[leyline_catalog::GridItem]) -> Vec<VersionId> {
    items.iter().map(|i| i.version_id).collect()
}

/// The full §26 example: rating ≥ 4, Canon EOS R5, Nature/Birds, picked.
fn spec_rules() -> SmartRules {
    SmartRules::parse(
        r#"{
            "rating": { "gte": 4 },
            "camera": "Canon EOS R5",
            "keywords": ["Nature/Birds"],
            "pick": true
        }"#,
    )
    .unwrap()
}

#[test]
fn the_spec_example_selects_exactly_the_matching_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    // The winner matches every criterion.
    let winner = add(&mut catalog, "winner.CR3");
    // Each other asset misses exactly one.
    let unrated = add(&mut catalog, "unrated.CR3");
    let wrong_camera = add(&mut catalog, "wrong_camera.CR3");
    let untagged = add(&mut catalog, "untagged.CR3");
    let unpicked = add(&mut catalog, "unpicked.CR3");

    let r5 = Metadata {
        camera: Some(CameraInfo {
            manufacturer: "Canon".to_owned(),
            model: "EOS R5".to_owned(),
        }),
        ..Metadata::default()
    };
    let fuji = Metadata {
        camera: Some(CameraInfo {
            manufacturer: "Fujifilm".to_owned(),
            model: "X-T5".to_owned(),
        }),
        ..Metadata::default()
    };
    for reg in [&winner, &unrated, &untagged, &unpicked] {
        catalog.set_metadata(reg.asset, &r5).unwrap();
    }
    catalog.set_metadata(wrong_camera.asset, &fuji).unwrap();

    let nature = catalog.create_keyword(None, "Nature").unwrap();
    let birds = catalog.create_keyword(Some(nature), "Birds").unwrap();
    let heron = catalog.create_keyword(Some(birds), "Heron").unwrap();
    for reg in [&winner, &unrated, &wrong_camera, &unpicked] {
        // The winner is tagged with a *descendant* of Nature/Birds.
        catalog.add_keyword(&[reg.asset], heron).unwrap();
    }

    for reg in [&winner, &wrong_camera, &untagged, &unpicked] {
        if reg.version != unrated.version {
            catalog.set_rating(&[reg.version], Some(4)).unwrap();
        }
    }
    for reg in [&winner, &unrated, &wrong_camera, &untagged] {
        catalog.set_pick(&[reg.version], PickState::Pick).unwrap();
    }

    let smart = catalog
        .create_smart_collection(None, "Best birds", &spec_rules())
        .unwrap();

    // §26: the client queries it like any collection.
    let query = GridQuery {
        collection: Some(smart),
        sort: Sort::Filename { ascending: true },
        ..GridQuery::default()
    };
    assert_eq!(catalog.count(&query).unwrap(), 1);
    assert_eq!(
        versions(&catalog.grid(&query).unwrap()),
        vec![winner.version]
    );

    // "manufacturer model" also matches the camera rule.
    let by_full_name = SmartRules {
        camera: Some("Canon EOS R5".to_owned()),
        ..SmartRules::default()
    };
    let full = catalog
        .create_smart_collection(None, "R5 shots", &by_full_name)
        .unwrap();
    assert_eq!(
        catalog
            .count(&GridQuery {
                collection: Some(full),
                ..GridQuery::default()
            })
            .unwrap(),
        4
    );
}

#[test]
fn pick_false_means_not_picked() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let picked = add(&mut catalog, "picked.CR3");
    let rejected = add(&mut catalog, "rejected.CR3");
    let plain = add(&mut catalog, "plain.CR3");

    catalog
        .set_pick(&[picked.version], PickState::Pick)
        .unwrap();
    catalog
        .set_pick(&[rejected.version], PickState::Reject)
        .unwrap();

    let not_picked = catalog
        .create_smart_collection(
            None,
            "Not picked",
            &SmartRules {
                pick: Some(false),
                ..SmartRules::default()
            },
        )
        .unwrap();
    let query = GridQuery {
        collection: Some(not_picked),
        sort: Sort::Filename { ascending: true },
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&query).unwrap()),
        vec![plain.version, rejected.version]
    );
}

#[test]
fn smart_collections_appear_in_the_tree_and_refuse_user_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    add(&mut catalog, "IMG_0001.CR3");

    let smart = catalog
        .create_smart_collection(None, "Rated", &spec_rules())
        .unwrap();
    let tree = catalog.collections().unwrap();
    assert_eq!(tree[0].collection_type, CollectionType::Smart);

    // No user-defined order to sort by.
    let query = GridQuery {
        collection: Some(smart),
        sort: Sort::CollectionOrder,
        ..GridQuery::default()
    };
    assert!(matches!(catalog.grid(&query), Err(LeylineError::Db(_))));
}

#[test]
fn unknown_criteria_and_bad_ranges_are_refused() {
    // A newer engine's rules must never be evaluated partially.
    assert!(matches!(
        SmartRules::parse(r#"{ "rating": { "gte": 4 }, "iso": { "lte": 800 } }"#),
        Err(LeylineError::InvalidSettings(_))
    ));
    assert!(matches!(
        SmartRules::parse(r#"{ "rating": { "gte": 9 } }"#),
        Err(LeylineError::InvalidSettings(_))
    ));

    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    assert!(matches!(
        catalog.create_smart_collection(
            None,
            "Bad",
            &SmartRules {
                rating: Some(RatingRule { gte: 0 }),
                ..SmartRules::default()
            }
        ),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn rules_round_trip_through_json() {
    let rules = spec_rules();
    assert_eq!(SmartRules::parse(&rules.to_json()).unwrap(), rules);
    assert_eq!(rules.keywords, vec!["Nature/Birds".to_owned()]);
    assert_eq!(rules.rating, Some(RatingRule { gte: 4 }));
}
