//! Integration tests: preview metadata and §20 validity.

use leyline_catalog::{Catalog, NewAsset, NewPreview, RegisteredAsset};
use leyline_core::{LeylineError, MediaType, PreviewKind, RevisionId};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Previews").unwrap()
}

fn registered_asset(catalog: &mut Catalog) -> RegisteredAsset {
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: "IMG_0001.CR3".to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 32_000_000,
        checksum: [0xAB; 32],
        width: Some(6000),
        height: Some(4000),
        capture_date: None,
        capture_offset_minutes: None,
    };
    catalog.add_asset(&new).unwrap()
}

fn thumbnail(registered: &RegisteredAsset, revision: RevisionId) -> NewPreview {
    NewPreview {
        asset: registered.asset,
        revision,
        kind: PreviewKind::Thumbnail,
        width: 256,
        height: 171,
        relative_path: format!("thumbnails/{}/{}.png", registered.asset, revision),
    }
}

/// Adds a child revision of the current head and moves the head onto it,
/// simulating one edit.
fn edit(catalog: &Catalog, registered: &RegisteredAsset) -> RevisionId {
    let head = catalog.current_head_revision(registered.asset).unwrap();
    catalog
        .connection()
        .execute(
            "INSERT INTO develop_revisions (asset_id, parent_revision_id, settings_json, created_at)
             VALUES (?1, ?2, '{\"schema\":1}', 0)",
            rusqlite::params![registered.asset.get(), head.get()],
        )
        .unwrap();
    let revision = RevisionId::new(catalog.connection().last_insert_rowid());
    catalog
        .connection()
        .execute(
            "UPDATE develop_versions SET head_revision_id = ?1 WHERE id = ?2",
            rusqlite::params![revision.get(), registered.version.get()],
        )
        .unwrap();
    revision
}

#[test]
fn a_recorded_preview_is_valid_for_the_head_revision() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);

    assert_eq!(
        catalog.current_head_revision(registered.asset).unwrap(),
        registered.revision
    );
    assert_eq!(
        catalog
            .valid_preview(registered.asset, PreviewKind::Thumbnail)
            .unwrap(),
        None
    );

    let new = thumbnail(&registered, registered.revision);
    catalog.record_preview(&new).unwrap();

    let row = catalog
        .valid_preview(registered.asset, PreviewKind::Thumbnail)
        .unwrap()
        .unwrap();
    assert_eq!(row.revision, registered.revision);
    assert_eq!((row.width, row.height), (256, 171));
    assert_eq!(row.relative_path, new.relative_path);

    // Another kind of the same revision is a separate slot.
    assert_eq!(
        catalog
            .valid_preview(registered.asset, PreviewKind::Small)
            .unwrap(),
        None
    );
}

#[test]
fn regenerating_a_slot_replaces_the_previous_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);

    catalog
        .record_preview(&thumbnail(&registered, registered.revision))
        .unwrap();
    let mut replacement = thumbnail(&registered, registered.revision);
    replacement.width = 200;
    replacement.relative_path = "thumbnails/other.png".to_owned();
    catalog.record_preview(&replacement).unwrap();

    let count: i64 = catalog
        .connection()
        .query_row("SELECT COUNT(*) FROM previews", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let row = catalog
        .valid_preview(registered.asset, PreviewKind::Thumbnail)
        .unwrap()
        .unwrap();
    assert_eq!(
        (row.width, row.relative_path.as_str()),
        (200, "thumbnails/other.png")
    );
}

#[test]
fn an_edit_invalidates_and_an_undo_revalidates() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);
    catalog
        .record_preview(&thumbnail(&registered, registered.revision))
        .unwrap();

    // Editing moves the head: the preview of the old head is no longer valid.
    edit(&catalog, &registered);
    assert_eq!(
        catalog
            .valid_preview(registered.asset, PreviewKind::Thumbnail)
            .unwrap(),
        None
    );

    // Undo: moving the head back revalidates the old file, no regeneration.
    catalog
        .connection()
        .execute(
            "UPDATE develop_versions SET head_revision_id = ?1 WHERE id = ?2",
            rusqlite::params![registered.revision.get(), registered.version.get()],
        )
        .unwrap();
    assert!(
        catalog
            .valid_preview(registered.asset, PreviewKind::Thumbnail)
            .unwrap()
            .is_some()
    );
}

#[test]
fn remove_revision_previews_returns_the_paths_to_delete() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);

    catalog
        .record_preview(&thumbnail(&registered, registered.revision))
        .unwrap();
    let mut small = thumbnail(&registered, registered.revision);
    small.kind = PreviewKind::Small;
    small.relative_path = "previews/1/1/1.png".to_owned();
    catalog.record_preview(&small).unwrap();

    let mut paths = catalog
        .remove_revision_previews(registered.revision)
        .unwrap();
    paths.sort();
    assert_eq!(
        paths,
        vec![
            "previews/1/1/1.png".to_owned(),
            format!(
                "thumbnails/{}/{}.png",
                registered.asset, registered.revision
            ),
        ]
    );
    let count: i64 = catalog
        .connection()
        .query_row("SELECT COUNT(*) FROM previews", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);

    // Idempotent: nothing left to remove.
    assert!(
        catalog
            .remove_revision_previews(registered.revision)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn preview_queries_report_missing_assets() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = new_catalog(&dir);
    assert!(matches!(
        catalog.current_head_revision(leyline_core::AssetId::new(999)),
        Err(LeylineError::AssetMissing(id)) if id.get() == 999
    ));
}
