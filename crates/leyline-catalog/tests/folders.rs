//! Integration tests: the folder tree as a sidebar reads it
//! (`docs/catalog.md` §8, ADR 0055 §2).

use leyline_catalog::{Catalog, NewAsset};
use leyline_core::{MediaType, Settings};

/// Registers one asset in `folder`, named after `n` so names stay unique.
fn add_photo(catalog: &mut Catalog, folder: leyline_core::FolderId, n: u8) {
    catalog
        .add_asset(
            &NewAsset {
                folder,
                filename: format!("IMG_{n:04}.CR3"),
                extension: "CR3".to_owned(),
                media_type: MediaType::Raw,
                file_size: 1_000,
                checksum: [n; 32],
                width: None,
                height: None,
                capture_date: None,
                capture_offset_minutes: None,
            },
            &Settings::default(),
        )
        .unwrap();
}

#[test]
fn folders_come_back_depth_first_with_their_own_photo_counts() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Folders").unwrap();

    // Created out of order on purpose: the listing must not depend on it.
    let wildlife = catalog.ensure_folder("Photos/Wildlife").unwrap();
    let archive = catalog.ensure_folder("Archive").unwrap();
    let birds = catalog.ensure_folder("Photos/Wildlife/Birds").unwrap();
    let photos = catalog.ensure_folder("Photos").unwrap();

    add_photo(&mut catalog, birds, 1);
    add_photo(&mut catalog, birds, 2);
    add_photo(&mut catalog, photos, 3);

    let rows = catalog.folders().unwrap();
    let shape: Vec<(&str, u32)> = rows
        .iter()
        .map(|row| (row.relative_path.as_str(), row.photo_count))
        .collect();
    assert_eq!(
        shape,
        vec![
            ("Archive", 0),
            ("Photos", 1),
            ("Photos/Wildlife", 0),
            ("Photos/Wildlife/Birds", 2),
        ],
        "path order is depth-first order, and a count is that folder's own \
         photos — `Photos` holds one, not the three under it"
    );

    // The parent links are the ones `ensure_folder` created on the way down,
    // so a caller can indent from either the path or the chain.
    let parent_of = |path: &str| {
        rows.iter()
            .find(|row| row.relative_path == path)
            .and_then(|row| row.parent)
    };
    assert_eq!(parent_of("Archive"), None);
    assert_eq!(parent_of("Photos"), None);
    assert_eq!(parent_of("Photos/Wildlife"), Some(photos));
    assert_eq!(parent_of("Photos/Wildlife/Birds"), Some(wildlife));
    assert!(rows.iter().any(|row| row.folder == archive));
}

#[test]
fn an_empty_library_lists_no_folder_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::create(&dir.path().join("catalog.db"), "Empty").unwrap();
    assert!(catalog.folders().unwrap().is_empty());
}

/// The library root is a folder like any other once something sits in it
/// (§8): one row, no parent, an empty path — and an asset path that does not
/// grow a leading separator from it (§9).
#[test]
fn the_library_root_has_a_folder_row_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Root").unwrap();

    let root = catalog.ensure_folder("").unwrap();
    assert_eq!(catalog.ensure_folder("").unwrap(), root, "idempotent");

    add_photo(&mut catalog, root, 1);
    let asset = catalog.grid(&Default::default()).unwrap()[0].asset_id;
    assert_eq!(catalog.asset_relative_path(asset).unwrap(), "IMG_0001.CR3");

    let rows = catalog.folders().unwrap();
    let row = rows
        .iter()
        .find(|node| node.folder == root)
        .expect("the root row is listed");
    assert_eq!((row.parent, row.photo_count), (None, 1));
    // It sorts first, being the shortest path there is.
    assert_eq!(rows[0].folder, root);
}

/// Every other path stays validated: the empty one is the root, not a licence
/// to store anything.
#[test]
fn the_root_exception_does_not_relax_any_other_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Root").unwrap();

    for refused in [
        "/Photos",
        "Photos/",
        "Photos//Wildlife",
        "../Photos",
        "C:/Photos",
    ] {
        assert!(
            catalog.ensure_folder(refused).is_err(),
            "{refused:?} must stay refused"
        );
    }
}
