//! Integration tests: revision commits, amendment guards, undo and redo
//! (`docs/catalog.md` §16, §17, §18).

use leyline_catalog::{Catalog, NewAsset, NewPreview, RegisteredAsset};
use leyline_core::{LeylineError, MediaType, PreviewKind, Settings, VersionId};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Revisions").unwrap()
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

fn exposure(ev: f64) -> Settings {
    Settings {
        exposure: ev,
        ..Settings::default()
    }
}

#[test]
fn commit_advances_the_head_and_chains_parents() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    let r1 = catalog
        .commit_revision(reg.version, &exposure(0.5))
        .unwrap();
    let r2 = catalog
        .commit_revision(reg.version, &exposure(1.0))
        .unwrap();

    assert_eq!(catalog.version_head(reg.version).unwrap(), r2);
    let row = catalog.revision(r2).unwrap();
    assert_eq!(row.parent, Some(r1));
    assert_eq!(row.asset, reg.asset);
    assert_eq!(Settings::parse(&row.settings_json).unwrap(), exposure(1.0));
    assert_eq!(catalog.revision(r1).unwrap().parent, Some(reg.revision));

    // Head first, back to the initial revision.
    let history = catalog.version_history(reg.version).unwrap();
    let ids: Vec<_> = history.iter().map(|r| r.revision).collect();
    assert_eq!(ids, vec![r2, r1, reg.revision]);
}

#[test]
fn commit_validates_settings_and_the_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    assert!(matches!(
        catalog.commit_revision(reg.version, &exposure(f64::NAN)),
        Err(LeylineError::InvalidSettings(_))
    ));
    assert!(matches!(
        catalog.commit_revision(VersionId::new(999), &exposure(0.5)),
        Err(LeylineError::VersionMissing(id)) if id.get() == 999
    ));
}

#[test]
fn amend_rewrites_the_head_in_place_and_invalidates_previews() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    let r1 = catalog
        .commit_revision(reg.version, &exposure(0.5))
        .unwrap();
    catalog
        .record_preview(&NewPreview {
            asset: reg.asset,
            revision: r1,
            kind: PreviewKind::Thumbnail,
            width: 320,
            height: 213,
            relative_path: "ab/cd.png".to_owned(),
        })
        .unwrap();

    let amendment = catalog
        .try_amend_head(reg.version, &exposure(0.6))
        .unwrap()
        .expect("head should be amendable");
    assert_eq!(amendment.revision, r1);
    assert_eq!(amendment.removed_previews, vec!["ab/cd.png".to_owned()]);

    // Same revision id, new settings, no new row.
    assert_eq!(catalog.version_head(reg.version).unwrap(), r1);
    let row = catalog.revision(r1).unwrap();
    assert_eq!(Settings::parse(&row.settings_json).unwrap(), exposure(0.6));
    assert_eq!(catalog.version_history(reg.version).unwrap().len(), 2);
    assert_eq!(
        catalog
            .valid_preview(reg.asset, PreviewKind::Thumbnail)
            .unwrap(),
        None
    );
}

#[test]
fn amend_refuses_the_initial_revision() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    // §17: the initial revision is never amended.
    assert_eq!(
        catalog.try_amend_head(reg.version, &exposure(0.5)).unwrap(),
        None
    );
    assert_eq!(
        Settings::parse(&catalog.revision(reg.revision).unwrap().settings_json).unwrap(),
        Settings::default()
    );
}

#[test]
fn amend_refuses_a_head_with_children() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    let r1 = catalog
        .commit_revision(reg.version, &exposure(0.5))
        .unwrap();
    catalog
        .commit_revision(reg.version, &exposure(1.0))
        .unwrap();
    catalog.undo_version(reg.version).unwrap();

    // Back on r1, which now has a child: amendment must refuse.
    assert_eq!(catalog.version_head(reg.version).unwrap(), r1);
    assert_eq!(
        catalog.try_amend_head(reg.version, &exposure(0.6)).unwrap(),
        None
    );
}

#[test]
fn amend_refuses_a_head_shared_with_another_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    let r1 = catalog
        .commit_revision(reg.version, &exposure(0.5))
        .unwrap();

    // A virtual version branching from the same head (§18).
    catalog
        .connection()
        .execute(
            "INSERT INTO develop_versions (uuid, asset_id, name, head_revision_id, created_at)
             VALUES ('bw-uuid', ?1, 'Noir & Blanc', ?2, 0)",
            rusqlite::params![reg.asset.get(), r1.get()],
        )
        .unwrap();

    assert_eq!(
        catalog.try_amend_head(reg.version, &exposure(0.6)).unwrap(),
        None
    );
}

#[test]
fn amend_never_overwrites_a_newer_engine_revision() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    let r1 = catalog
        .commit_revision(reg.version, &exposure(0.5))
        .unwrap();
    catalog
        .connection()
        .execute(
            "UPDATE develop_revisions SET settings_json = '{\"schema\":99,\"process\":1}'
             WHERE id = ?1",
            [r1.get()],
        )
        .unwrap();

    // §3.4: an old engine never modifies what a newer one wrote.
    assert!(matches!(
        catalog.try_amend_head(reg.version, &exposure(0.6)),
        Err(LeylineError::NewerSettings { schema: 99, .. })
    ));
}

#[test]
fn undo_and_redo_move_the_head_without_deleting_anything() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    let r1 = catalog
        .commit_revision(reg.version, &exposure(0.5))
        .unwrap();
    let r2 = catalog
        .commit_revision(reg.version, &exposure(1.0))
        .unwrap();

    assert_eq!(catalog.undo_version(reg.version).unwrap(), Some(r1));
    assert_eq!(
        catalog.undo_version(reg.version).unwrap(),
        Some(reg.revision)
    );
    // At the initial revision: nothing left to undo.
    assert_eq!(catalog.undo_version(reg.version).unwrap(), None);

    assert_eq!(catalog.redo_version(reg.version).unwrap(), Some(r1));
    assert_eq!(catalog.redo_version(reg.version).unwrap(), Some(r2));
    assert_eq!(catalog.redo_version(reg.version).unwrap(), None);

    // The full chain survived the round trip.
    assert_eq!(catalog.version_history(reg.version).unwrap().len(), 3);
}

#[test]
fn checkout_revision_jumps_directly_without_stepping() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    let r1 = catalog
        .commit_revision(reg.version, &exposure(0.5))
        .unwrap();
    let r2 = catalog
        .commit_revision(reg.version, &exposure(1.0))
        .unwrap();
    catalog
        .commit_revision(reg.version, &exposure(1.5))
        .unwrap();

    catalog.checkout_revision(reg.version, r1).unwrap();
    assert_eq!(catalog.version_head(reg.version).unwrap(), r1);

    // Jumping forward again to a revision ahead of the current head works
    // too — unlike `version_history`, which only walks backward from
    // wherever the head currently sits.
    catalog.checkout_revision(reg.version, r2).unwrap();
    assert_eq!(catalog.version_head(reg.version).unwrap(), r2);

    // Nothing was created or deleted: r2's own backward chain still has its
    // 3 entries (initial, r1, r2) — the 4th commit is a forward descendant
    // of r2, correctly excluded by `version_history`'s backward walk.
    assert_eq!(catalog.version_history(reg.version).unwrap().len(), 3);
}

#[test]
fn checkout_revision_refuses_a_revision_of_a_different_asset() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);
    let folder = catalog.ensure_folder("Photos").unwrap();
    let other = catalog
        .add_asset(&NewAsset {
            folder,
            filename: "IMG_0002.CR3".to_owned(),
            extension: "CR3".to_owned(),
            media_type: MediaType::Raw,
            file_size: 32_000_000,
            checksum: [0xCD; 32],
            width: Some(6000),
            height: Some(4000),
            capture_date: None,
            capture_offset_minutes: None,
        })
        .unwrap();

    let foreign = catalog
        .commit_revision(other.version, &exposure(0.5))
        .unwrap();

    assert!(matches!(
        catalog.checkout_revision(reg.version, foreign),
        Err(LeylineError::RevisionMissing(id)) if id == foreign
    ));
}

#[test]
fn redo_follows_the_most_recent_branch() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    let r1 = catalog
        .commit_revision(reg.version, &exposure(0.5))
        .unwrap();
    catalog.undo_version(reg.version).unwrap();
    let r2 = catalog
        .commit_revision(reg.version, &exposure(-0.5))
        .unwrap();
    catalog.undo_version(reg.version).unwrap();

    // The initial revision now has two children; redo picks the newest.
    assert_eq!(catalog.redo_version(reg.version).unwrap(), Some(r2));
    // r1 stays reachable in the graph.
    assert_eq!(catalog.revision(r1).unwrap().parent, Some(reg.revision));
}

#[test]
fn undo_revalidates_previews_of_the_previous_head() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    catalog
        .record_preview(&NewPreview {
            asset: reg.asset,
            revision: reg.revision,
            kind: PreviewKind::Thumbnail,
            width: 320,
            height: 213,
            relative_path: "ab/initial.png".to_owned(),
        })
        .unwrap();
    catalog
        .commit_revision(reg.version, &exposure(0.5))
        .unwrap();
    assert_eq!(
        catalog
            .valid_preview(reg.asset, PreviewKind::Thumbnail)
            .unwrap(),
        None
    );

    // §20: validity is a pure revision-id comparison — undo restores it.
    catalog.undo_version(reg.version).unwrap();
    let preview = catalog
        .valid_preview(reg.asset, PreviewKind::Thumbnail)
        .unwrap()
        .expect("initial preview should be valid again");
    assert_eq!(preview.relative_path, "ab/initial.png");
}

#[test]
fn missing_ids_are_reported_precisely() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = new_catalog(&dir);

    assert!(matches!(
        catalog.version_head(VersionId::new(7)),
        Err(LeylineError::VersionMissing(id)) if id.get() == 7
    ));
    assert!(matches!(
        catalog.revision(leyline_core::RevisionId::new(7)),
        Err(LeylineError::RevisionMissing(id)) if id.get() == 7
    ));
}

#[test]
fn read_only_handles_refuse_revision_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.db");
    let version = {
        let mut catalog = Catalog::create(&path, "RO").unwrap();
        registered_asset(&mut catalog).version
    };

    let mut catalog = Catalog::open_read_only(&path).unwrap();
    assert!(matches!(
        catalog.commit_revision(version, &exposure(0.5)),
        Err(LeylineError::Db(_))
    ));
    assert!(matches!(
        catalog.undo_version(version),
        Err(LeylineError::Db(_))
    ));
}
