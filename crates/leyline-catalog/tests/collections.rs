//! Integration tests: manual collections (`docs/catalog.md` §24, §25).

use leyline_catalog::{Catalog, NewAsset};
use leyline_core::Settings;
use leyline_core::{CollectionId, CollectionType, LeylineError, MediaType, VersionId};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Collections").unwrap()
}

/// Registers three assets and returns their Default versions.
fn three_versions(catalog: &mut Catalog) -> Vec<VersionId> {
    (1..=3)
        .map(|n| {
            let new = NewAsset {
                folder: catalog.ensure_folder("Photos").unwrap(),
                filename: format!("IMG_000{n}.CR3"),
                extension: "CR3".to_owned(),
                media_type: MediaType::Raw,
                file_size: 1,
                checksum: [n as u8; 32],
                width: None,
                height: None,
                capture_date: None,
                capture_offset_minutes: None,
            };
            catalog
                .add_asset(&new, &Settings::default())
                .unwrap()
                .version
        })
        .collect()
}

#[test]
fn collections_form_a_tree_ordered_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let trips = catalog.create_collection(None, "Trips").unwrap();
    catalog.create_collection(Some(trips), "Norway").unwrap();
    catalog.create_collection(Some(trips), "Iceland").unwrap();
    catalog.create_collection(None, "Best of").unwrap();

    let tree = catalog.collections().unwrap();
    let names: Vec<_> = tree.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Best of", "Trips"]);
    let children: Vec<_> = tree[1].children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(children, ["Iceland", "Norway"]);
    assert_eq!(tree[1].collection_type, CollectionType::Manual);

    assert!(matches!(
        catalog.create_collection(Some(CollectionId::new(999)), "Orphan"),
        Err(LeylineError::CollectionMissing(_))
    ));
}

#[test]
fn membership_appends_in_order_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let versions = three_versions(&mut catalog);
    let album = catalog.create_collection(None, "Album").unwrap();

    catalog.add_to_collection(album, &versions[..2]).unwrap();
    catalog
        .add_to_collection(album, &[versions[0], versions[2]]) // [0] already in
        .unwrap();

    // Insertion order preserved, duplicates kept their position.
    assert_eq!(
        catalog.collection_versions(album).unwrap(),
        vec![versions[0], versions[1], versions[2]]
    );

    // A version can live in several collections at once (§24).
    let other = catalog.create_collection(None, "Other").unwrap();
    catalog.add_to_collection(other, &versions[..1]).unwrap();
    assert_eq!(
        catalog.collection_versions(other).unwrap(),
        vec![versions[0]]
    );

    // A missing version rolls the whole batch back.
    assert!(matches!(
        catalog.add_to_collection(other, &[versions[1], VersionId::new(999)]),
        Err(LeylineError::VersionMissing(_))
    ));
    assert_eq!(
        catalog.collection_versions(other).unwrap(),
        vec![versions[0]]
    );
}

#[test]
fn removal_keeps_relative_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let versions = three_versions(&mut catalog);
    let album = catalog.create_collection(None, "Album").unwrap();
    catalog.add_to_collection(album, &versions).unwrap();

    catalog
        .remove_from_collection(album, &versions[1..2])
        .unwrap();
    catalog
        .remove_from_collection(album, &versions[1..2])
        .unwrap(); // no-op
    assert_eq!(
        catalog.collection_versions(album).unwrap(),
        vec![versions[0], versions[2]]
    );

    // Appending after a removal still lands at the end.
    catalog.add_to_collection(album, &versions[1..2]).unwrap();
    assert_eq!(
        catalog.collection_versions(album).unwrap(),
        vec![versions[0], versions[2], versions[1]]
    );
}

#[test]
fn reorder_requires_the_exact_membership() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let versions = three_versions(&mut catalog);
    let album = catalog.create_collection(None, "Album").unwrap();
    catalog.add_to_collection(album, &versions).unwrap();

    catalog
        .reorder_collection(album, &[versions[2], versions[0], versions[1]])
        .unwrap();
    assert_eq!(
        catalog.collection_versions(album).unwrap(),
        vec![versions[2], versions[0], versions[1]]
    );

    // Incomplete, duplicated, or foreign lists are refused atomically.
    for bad in [
        vec![versions[0], versions[1]],
        vec![versions[0], versions[0], versions[1]],
        vec![versions[0], versions[1], VersionId::new(999)],
    ] {
        assert!(catalog.reorder_collection(album, &bad).is_err());
    }
    assert_eq!(
        catalog.collection_versions(album).unwrap(),
        vec![versions[2], versions[0], versions[1]]
    );
}

#[test]
fn smart_collections_refuse_explicit_membership() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let versions = three_versions(&mut catalog);

    catalog
        .connection()
        .execute(
            "INSERT INTO collections (uuid, name, collection_type, rules_json, created_at)
             VALUES ('smart-uuid', 'Four stars', 1, '{\"rating\":{\"gte\":4}}', 0)",
            [],
        )
        .unwrap();
    let smart = CollectionId::new(catalog.connection().last_insert_rowid());

    assert!(matches!(
        catalog.add_to_collection(smart, &versions[..1]),
        Err(LeylineError::Db(_))
    ));
    assert!(matches!(
        catalog.reorder_collection(smart, &[]),
        Err(LeylineError::Db(_))
    ));
    assert_eq!(
        catalog.collections().unwrap()[0].collection_type,
        CollectionType::Smart
    );
}

#[test]
fn missing_collections_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = new_catalog(&dir);
    assert!(matches!(
        catalog.collection_versions(CollectionId::new(999)),
        Err(LeylineError::CollectionMissing(id)) if id.get() == 999
    ));
}

#[test]
fn renaming_changes_the_name_and_refuses_a_blank_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let album = catalog.create_collection(None, "Portrait").unwrap();

    catalog.rename_collection(album, "  Portraits  ").unwrap();
    let name = catalog
        .collections()
        .unwrap()
        .into_iter()
        .find(|node| node.collection == album)
        .map(|node| node.name);
    assert_eq!(name.as_deref(), Some("Portraits"), "the name is trimmed");

    assert!(catalog.rename_collection(album, "   ").is_err());
    assert!(
        catalog
            .rename_collection(CollectionId::new(9_999), "Ghost")
            .is_err()
    );
}

#[test]
fn moving_reparents_and_refuses_to_swallow_its_own_subtree() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let travel = catalog.create_collection(None, "Travel").unwrap();
    let iceland = catalog.create_collection(Some(travel), "Iceland").unwrap();
    let reykjavik = catalog
        .create_collection(Some(iceland), "Reykjavik")
        .unwrap();
    let portfolio = catalog.create_collection(None, "Portfolio").unwrap();

    // Down into another tree, then back up to the root.
    catalog.move_collection(portfolio, Some(iceland)).unwrap();
    catalog.move_collection(portfolio, None).unwrap();

    // A collection cannot become its own descendant, at any depth: the rows
    // would stay valid and the subtree would vanish from every read.
    assert!(catalog.move_collection(travel, Some(reykjavik)).is_err());
    assert!(catalog.move_collection(travel, Some(travel)).is_err());
    // Moving a child under a sibling of its parent stays legitimate.
    catalog.move_collection(reykjavik, Some(portfolio)).unwrap();
}

#[test]
fn deleting_takes_the_subtree_and_the_memberships_but_no_photo() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let versions = three_versions(&mut catalog);
    let travel = catalog.create_collection(None, "Travel").unwrap();
    let iceland = catalog.create_collection(Some(travel), "Iceland").unwrap();
    let _deep = catalog
        .create_collection(Some(iceland), "Reykjavik")
        .unwrap();
    let keep = catalog.create_collection(None, "Portfolio").unwrap();
    catalog.add_to_collection(iceland, &versions).unwrap();
    catalog.add_to_collection(keep, &versions).unwrap();

    let gone = catalog.delete_collection(travel).unwrap();
    assert_eq!(gone, 3, "the collection and its two descendants");

    let left: Vec<String> = catalog
        .collections()
        .unwrap()
        .into_iter()
        .map(|node| node.name)
        .collect();
    assert_eq!(left, vec!["Portfolio".to_owned()]);

    // The invariant of §29: memberships go, versions stay — and the photos
    // are still in the collection that was not deleted.
    assert_eq!(catalog.collection_versions(keep).unwrap(), versions);
    assert!(matches!(
        catalog.collection_versions(iceland),
        Err(LeylineError::CollectionMissing(_))
    ));
    assert_eq!(catalog.grid(&Default::default()).unwrap().len(), 3);
}
