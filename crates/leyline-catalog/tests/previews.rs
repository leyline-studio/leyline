//! Integration tests: preview metadata and §20 validity.

use leyline_catalog::{Catalog, NewAsset, NewPreview, RegisteredAsset};
use leyline_core::Settings;
use leyline_core::{LeylineError, MediaType, PreviewKind, PreviewOrigin, RevisionId};

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
    catalog.add_asset(&new, &Settings::default()).unwrap()
}

fn thumbnail(registered: &RegisteredAsset, revision: RevisionId) -> NewPreview {
    NewPreview {
        asset: registered.asset,
        revision,
        kind: PreviewKind::Thumbnail,
        width: 256,
        height: 171,
        relative_path: format!("thumbnails/{}/{}.png", registered.asset, revision),
        origin: PreviewOrigin::Rendered,
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
fn record_preview_if_current_writes_when_settings_still_match() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);
    let settings_json = catalog.revision(registered.revision).unwrap().settings_json;

    let wrote = catalog
        .record_preview_if_current(&thumbnail(&registered, registered.revision), &settings_json)
        .unwrap();

    assert!(wrote);
    assert!(
        catalog
            .valid_preview(registered.asset, PreviewKind::Thumbnail)
            .unwrap()
            .is_some()
    );
}

#[test]
fn record_preview_if_current_skips_a_revision_amended_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);
    // The settings captured before the (simulated) concurrent render started.
    let stale_settings_json = catalog.revision(registered.revision).unwrap().settings_json;

    // Same revision id rewritten in place — the §17 amendment path, here
    // done directly over SQL the way `edit()` above simulates a commit,
    // since `try_amend_head` itself lives in `leyline-engine`.
    catalog
        .connection()
        .execute(
            "UPDATE develop_revisions SET settings_json = '{\"schema\":1,\"exposure\":1.0}' WHERE id = ?1",
            [registered.revision.get()],
        )
        .unwrap();

    let wrote = catalog
        .record_preview_if_current(
            &thumbnail(&registered, registered.revision),
            &stale_settings_json,
        )
        .unwrap();

    assert!(!wrote, "a stale render must never be recorded as valid");
    assert_eq!(
        catalog
            .valid_preview(registered.asset, PreviewKind::Thumbnail)
            .unwrap(),
        None
    );
}

#[test]
fn record_preview_if_current_writes_across_a_plain_commit() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);
    let rendered_revision = registered.revision;
    let settings_json = catalog.revision(rendered_revision).unwrap().settings_json;

    // A plain commit (not an amendment): a new revision, head moves on, the
    // rendered revision's own `settings_json` is untouched.
    edit(&catalog, &registered);

    let wrote = catalog
        .record_preview_if_current(&thumbnail(&registered, rendered_revision), &settings_json)
        .unwrap();

    assert!(wrote, "a plain commit must never trip the guard");
    // Not the head anymore, so not valid yet — but an undo back onto it
    // revalidates for free, same as `record_preview` today.
    assert_eq!(
        catalog
            .valid_preview(registered.asset, PreviewKind::Thumbnail)
            .unwrap(),
        None
    );
    catalog
        .connection()
        .execute(
            "UPDATE develop_versions SET head_revision_id = ?1 WHERE id = ?2",
            rusqlite::params![rendered_revision.get(), registered.version.get()],
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
fn record_preview_if_current_skips_a_missing_revision() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);

    let wrote = catalog
        .record_preview_if_current(
            &thumbnail(&registered, RevisionId::new(999_999)),
            "{\"schema\":1}",
        )
        .unwrap();

    assert!(!wrote);
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

/// The window of ADR 0075: the three most recent revisions survive, the rest
/// are dropped and their files named so the caller can unlink them.
#[test]
fn retain_previews_keeps_the_window_and_names_what_it_drops() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);

    // Six revisions, each one previewed as it became the head.
    let mut revisions = vec![registered.revision];
    catalog
        .record_preview(&thumbnail(&registered, registered.revision))
        .unwrap();
    for _ in 0..5 {
        let revision = edit(&catalog, &registered);
        catalog
            .record_preview(&thumbnail(&registered, revision))
            .unwrap();
        revisions.push(revision);
    }

    let dropped = catalog.retain_previews(registered.asset, 3).unwrap();

    // The three oldest go, and the caller is told which files to unlink.
    assert_eq!(dropped.len(), 3, "{dropped:?}");
    for revision in &revisions[..3] {
        assert!(
            dropped
                .iter()
                .any(|path| path.ends_with(&format!("{revision}.png"))),
            "revision {revision} should have been dropped: {dropped:?}"
        );
    }
    // The head is still served, which is the point of keeping a window at all.
    assert!(
        catalog
            .valid_preview(registered.asset, PreviewKind::Thumbnail)
            .unwrap()
            .is_some()
    );
    // Idempotent: a second pass has nothing left to do.
    assert!(
        catalog
            .retain_previews(registered.asset, 3)
            .unwrap()
            .is_empty()
    );
}

/// A virtual copy parked on an old revision keeps its preview however far the
/// other copies have moved on — otherwise the grid would re-render it on
/// every scroll (ADR 0075 §1).
#[test]
fn retain_previews_never_drops_a_version_head() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);
    let parked = registered.revision;
    catalog
        .record_preview(&thumbnail(&registered, parked))
        .unwrap();

    // A second copy, left on the original revision, while the first one is
    // edited well past the window.
    let copy = catalog
        .create_version(registered.version, "Copy", Some(parked))
        .unwrap();
    for _ in 0..5 {
        let revision = edit(&catalog, &registered);
        catalog
            .record_preview(&thumbnail(&registered, revision))
            .unwrap();
    }

    let dropped = catalog.retain_previews(registered.asset, 3).unwrap();
    assert!(
        !dropped
            .iter()
            .any(|path| path.ends_with(&format!("{parked}.png"))),
        "the parked copy lost its preview: {dropped:?}"
    );
    assert_eq!(
        catalog.version_head(copy).unwrap(),
        parked,
        "the copy under test is not the one parked"
    );
}

/// ADR 0082 §2: a preview the file carried draws fine but is not the head's
/// render. If `valid_preview` accepted it, nothing would ever replace it —
/// the grid would show the camera's JPEG for good, and believe it was
/// showing a development.
#[test]
fn an_embedded_preview_is_displayable_but_never_valid() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);
    let head = catalog.current_head_revision(registered.asset).unwrap();

    catalog
        .record_preview(&NewPreview {
            origin: PreviewOrigin::Embedded,
            ..thumbnail(&registered, head)
        })
        .unwrap();

    assert_eq!(
        catalog
            .valid_preview(registered.asset, PreviewKind::Thumbnail)
            .unwrap(),
        None,
        "an embedded preview answered for the head's render"
    );
    let shown = catalog
        .displayable_preview(registered.asset, PreviewKind::Thumbnail)
        .unwrap()
        .expect("the cell has something to draw");
    assert_eq!(shown.origin, PreviewOrigin::Embedded);
    assert_eq!(shown.revision, head);
}

/// The render lands in the same slot and takes it over: same asset, same
/// revision, same kind, and `origin` flips. That is what lets the constraint
/// stay on three columns (`docs/catalog.md` §19).
#[test]
fn a_render_replaces_the_embedded_preview_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);
    let head = catalog.current_head_revision(registered.asset).unwrap();

    catalog
        .record_preview(&NewPreview {
            origin: PreviewOrigin::Embedded,
            ..thumbnail(&registered, head)
        })
        .unwrap();
    catalog
        .record_preview(&thumbnail(&registered, head))
        .unwrap();

    let rows: i64 = catalog
        .connection()
        .query_row("SELECT COUNT(*) FROM previews", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1, "the two previews stacked instead of replacing");
    assert_eq!(
        catalog
            .displayable_preview(registered.asset, PreviewKind::Thumbnail)
            .unwrap()
            .unwrap()
            .origin,
        PreviewOrigin::Rendered
    );
    assert!(
        catalog
            .valid_preview(registered.asset, PreviewKind::Thumbnail)
            .unwrap()
            .is_some(),
        "the render did not become the head's valid preview"
    );
}

/// ADR 0082 §2: the retention window of ADR 0075 counts revisions, and an
/// embedded preview belongs to the file rather than to one. Editing past the
/// window must not evict the image the import produced.
#[test]
fn retention_never_evicts_the_file_s_own_preview() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let registered = registered_asset(&mut catalog);
    let initial = catalog.current_head_revision(registered.asset).unwrap();

    catalog
        .record_preview(&NewPreview {
            origin: PreviewOrigin::Embedded,
            ..thumbnail(&registered, initial)
        })
        .unwrap();

    // Five edits, each with its own render: the initial revision falls well
    // outside a window of three.
    for _ in 0..5 {
        let revision = edit(&catalog, &registered);
        catalog
            .record_preview(&thumbnail(&registered, revision))
            .unwrap();
        catalog.retain_previews(registered.asset, 3).unwrap();
    }

    let embedded: i64 = catalog
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM previews WHERE origin = 1 AND revision_id = ?1",
            [initial.get()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        embedded, 1,
        "the file's own preview was evicted with the history"
    );

    // And the amendment rule of §17 leaves it alone too.
    catalog.remove_revision_previews(initial).unwrap();
    let embedded: i64 = catalog
        .connection()
        .query_row("SELECT COUNT(*) FROM previews WHERE origin = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        embedded, 1,
        "an amendment took the file's own preview with it"
    );
}
