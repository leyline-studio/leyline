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

#[test]
fn delete_assets_takes_the_whole_graph_and_frees_the_checksum() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let asset = sample_asset(&mut catalog, "IMG_0001.CR3");
    let checksum = asset.checksum;
    let registered = catalog.add_asset(&asset, &Settings::default()).unwrap();

    // A cached preview: its row cascades, but its file on disk is the
    // caller's to unlink, so the path must come back out.
    catalog
        .record_preview(&leyline_catalog::NewPreview {
            asset: registered.asset,
            revision: registered.revision,
            kind: leyline_core::PreviewKind::Small,
            width: 1024,
            height: 683,
            relative_path: "ab/cd/preview.png".to_owned(),
        })
        .unwrap();

    let deleted = catalog.delete_assets(&[registered.asset]).unwrap();
    assert_eq!(deleted.assets, vec![registered.asset]);
    assert_eq!(deleted.file_paths, vec!["Photos/Wildlife/IMG_0001.CR3"]);
    assert_eq!(deleted.preview_paths, vec!["ab/cd/preview.png"]);

    // Every table hanging off the asset is empty — the six that cascade,
    // plus `search_index`, which is an FTS5 virtual table where foreign
    // keys do not apply and which would otherwise keep answering searches
    // with a photo that no longer exists.
    for table in [
        "assets",
        "metadata",
        "develop_revisions",
        "develop_versions",
        "develop_current",
        "previews",
        "asset_keywords",
        "export_history",
        "search_index",
    ] {
        let count: i64 = catalog
            .connection()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "{table} still holds rows after the delete");
    }

    // The point of the whole feature (ADR 0060, ADR 0043 §5): the
    // checksum is free again, so the file re-imports instead of being
    // skipped as a duplicate.
    assert_eq!(catalog.find_asset_by_checksum(&checksum).unwrap(), None);
    let again = sample_asset(&mut catalog, "IMG_0001.CR3");
    catalog.add_asset(&again, &Settings::default()).unwrap();
}

#[test]
fn delete_assets_ignores_unknown_ids_and_an_empty_batch() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let asset = sample_asset(&mut catalog, "IMG_0002.CR3");
    let registered = catalog.add_asset(&asset, &Settings::default()).unwrap();

    assert_eq!(catalog.delete_assets(&[]).unwrap().assets, vec![]);

    // Removing what is already gone is the caller's intent either way:
    // the unknown id contributes nothing and does not fail the batch.
    let ghost = leyline_core::AssetId::new(registered.asset.get() + 999);
    let deleted = catalog.delete_assets(&[ghost, registered.asset]).unwrap();
    assert_eq!(deleted.assets, vec![registered.asset]);
    assert_eq!(deleted.file_paths.len(), 1);
}
