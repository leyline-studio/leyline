//! Integration tests: folder tree and asset registration.

use leyline_catalog::{Catalog, NewAsset};
use leyline_core::{LeylineError, MediaType, Settings};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Write").unwrap()
}

fn sample_asset(catalog: &mut Catalog, filename: &str) -> NewAsset {
    NewAsset {
        folder: catalog.ensure_folder("Photos/Wildlife").unwrap(),
        filename: filename.to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 32_000_000,
        checksum: [0xAB; 32],
        width: Some(6000),
        height: Some(4000),
        capture_date: Some(1_784_000_000_000),
        capture_offset_minutes: Some(120),
    }
}

#[test]
fn ensure_folder_creates_the_whole_chain_idempotently() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let wildlife = catalog.ensure_folder("Photos/Wildlife").unwrap();
    let again = catalog.ensure_folder("Photos/Wildlife").unwrap();
    assert_eq!(wildlife, again);

    // The ancestor exists as its own row, parent of the leaf.
    let (leaf_parent, root_parent): (i64, Option<i64>) = catalog
        .connection()
        .query_row(
            "SELECT leaf.parent_id, root.parent_id
             FROM folders leaf JOIN folders root ON root.id = leaf.parent_id
             WHERE leaf.relative_path = 'Photos/Wildlife'
               AND root.relative_path = 'Photos'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(leaf_parent > 0);
    assert_eq!(root_parent, None);

    // A sibling reuses the existing ancestor.
    catalog.ensure_folder("Photos/Macro").unwrap();
    let folder_count: i64 = catalog
        .connection()
        .query_row("SELECT COUNT(*) FROM folders", [], |r| r.get(0))
        .unwrap();
    assert_eq!(folder_count, 3);
}

#[test]
fn ensure_folder_rejects_non_portable_paths() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    for bad in [
        "",
        "/absolute",
        "Photos\\Wildlife",
        "C:/Photos",
        "Photos/",
        "Photos//Wildlife",
        "Photos/../secret",
    ] {
        assert!(
            matches!(catalog.ensure_folder(bad), Err(LeylineError::Io(_))),
            "path {bad:?} should be rejected"
        );
    }
}

#[test]
fn add_asset_creates_the_mandatory_develop_trio() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let new = sample_asset(&mut catalog, "IMG_0001.CR3");
    let registered = catalog.add_asset(&new, &Settings::default()).unwrap();

    // §18: initial revision is neutral, parentless, and the head of `Default`.
    let (settings_json, parent): (String, Option<i64>) = catalog
        .connection()
        .query_row(
            "SELECT settings_json, parent_revision_id FROM develop_revisions WHERE id = ?1",
            [registered.revision.get()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(parent, None);
    assert_eq!(
        Settings::parse(&settings_json).unwrap(),
        Settings::default()
    );

    let (name, head): (String, i64) = catalog
        .connection()
        .query_row(
            "SELECT name, head_revision_id FROM develop_versions WHERE id = ?1",
            [registered.version.get()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name, "Default");
    assert_eq!(head, registered.revision.get());

    let current: i64 = catalog
        .connection()
        .query_row(
            "SELECT version_id FROM develop_current WHERE asset_id = ?1",
            [registered.asset.get()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(current, registered.version.get());

    // §9: the path is derived from the folder, never stored.
    assert_eq!(
        catalog.asset_relative_path(registered.asset).unwrap(),
        "Photos/Wildlife/IMG_0001.CR3"
    );
}

#[test]
fn add_asset_rejects_duplicates_in_the_same_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let new = sample_asset(&mut catalog, "IMG_0001.CR3");
    catalog.add_asset(&new, &Settings::default()).unwrap();
    assert!(matches!(
        catalog.add_asset(&new, &Settings::default()),
        Err(LeylineError::Db(_))
    ));

    // The failed transaction left nothing behind.
    let assets: i64 = catalog
        .connection()
        .query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(assets, 1);
}

#[test]
fn asset_relative_path_reports_missing_assets() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = new_catalog(&dir);
    assert!(matches!(
        catalog.asset_relative_path(leyline_core::AssetId::new(999)),
        Err(LeylineError::AssetMissing(id)) if id.get() == 999
    ));
}

#[test]
fn read_only_handles_refuse_writes_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.db");
    drop(Catalog::create(&path, "RO").unwrap());

    let mut catalog = Catalog::open_read_only(&path).unwrap();
    assert!(matches!(
        catalog.ensure_folder("Photos"),
        Err(LeylineError::Db(_))
    ));
}
