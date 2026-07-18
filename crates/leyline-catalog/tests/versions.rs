//! Integration tests: virtual versions and classement
//! (`docs/catalog.md` §16, §18, `docs/engine-api.md` §8, §10.2).

use leyline_catalog::{Catalog, NewAsset, RegisteredAsset};
use leyline_core::{
    AssetId, ColorLabel, LeylineError, MediaType, PickState, RevisionId, Settings, VersionId,
};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Versions").unwrap()
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

fn exposure(catalog: &mut Catalog, version: VersionId, ev: f64) -> RevisionId {
    let settings = Settings {
        exposure: ev,
        ..Settings::default()
    };
    catalog.commit_revision(version, &settings).unwrap()
}

#[test]
fn create_version_branches_without_duplicating_anything() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);
    let r1 = exposure(&mut catalog, reg.version, 0.5);

    // Branch from the head of Default.
    let bw = catalog
        .create_version(reg.version, "Noir & Blanc", None)
        .unwrap();
    // Branch from an explicit revision: the initial one.
    let soft = catalog
        .create_version(reg.version, "Soft", Some(reg.revision))
        .unwrap();

    let versions = catalog.versions(reg.asset).unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(versions[0].name, "Default");
    assert_eq!(versions[1].name, "Noir & Blanc");
    assert_eq!(versions[1].head, r1);
    assert_eq!(versions[2].head, reg.revision);
    // A fresh branch starts unclassed.
    assert_eq!(versions[1].rating, None);
    assert_eq!(versions[1].pick, PickState::None);

    // No revision was duplicated: the branches share the graph.
    let revisions: i64 = catalog
        .connection()
        .query_row("SELECT COUNT(*) FROM develop_revisions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(revisions, 2);

    // Editing the branch does not move Default's head.
    exposure(&mut catalog, bw, -1.0);
    assert_eq!(catalog.version_head(reg.version).unwrap(), r1);
    assert_eq!(catalog.version_head(soft).unwrap(), reg.revision);
}

#[test]
fn create_version_rejects_foreign_revisions_and_duplicate_names() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    let other = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: "IMG_0002.CR3".to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xCD; 32],
        width: None,
        height: None,
        capture_date: None,
        capture_offset_minutes: None,
    };
    let other = catalog.add_asset(&other).unwrap();

    // A revision of another asset is not a valid branch point.
    assert!(matches!(
        catalog.create_version(reg.version, "Bad", Some(other.revision)),
        Err(LeylineError::RevisionMissing(id)) if id == other.revision
    ));
    // Names are unique per asset (`UNIQUE(asset_id, name)`).
    assert!(matches!(
        catalog.create_version(reg.version, "Default", None),
        Err(LeylineError::Db(_))
    ));
    // But reusable across assets.
    catalog
        .create_version(other.version, "Default 2", None)
        .unwrap();
}

#[test]
fn current_version_switches_and_drives_preview_validity() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);
    let r1 = exposure(&mut catalog, reg.version, 0.5);

    let bw = catalog
        .create_version(reg.version, "Noir & Blanc", Some(reg.revision))
        .unwrap();
    assert_eq!(catalog.current_version(reg.asset).unwrap(), reg.version);
    assert_eq!(catalog.current_head_revision(reg.asset).unwrap(), r1);

    catalog.set_current_version(reg.asset, bw).unwrap();
    assert_eq!(catalog.current_version(reg.asset).unwrap(), bw);
    // §20: validity follows the current version's head.
    assert_eq!(
        catalog.current_head_revision(reg.asset).unwrap(),
        reg.revision
    );

    // A version of another asset is refused.
    let foreign = registered_asset_named(&mut catalog, "IMG_0003.CR3");
    assert!(matches!(
        catalog.set_current_version(reg.asset, foreign.version),
        Err(LeylineError::VersionMissing(_))
    ));
}

fn registered_asset_named(catalog: &mut Catalog, filename: &str) -> RegisteredAsset {
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: filename.to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xEF; 32],
        width: None,
        height: None,
        capture_date: None,
        capture_offset_minutes: None,
    };
    catalog.add_asset(&new).unwrap()
}

#[test]
fn rename_version_enforces_existence() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    catalog.rename_version(reg.version, "Master").unwrap();
    assert_eq!(catalog.versions(reg.asset).unwrap()[0].name, "Master");
    assert!(matches!(
        catalog.rename_version(VersionId::new(999), "X"),
        Err(LeylineError::VersionMissing(_))
    ));
}

#[test]
fn delete_version_refuses_the_last_and_repoints_current() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);

    // §18: the last version is never deleted.
    assert!(matches!(
        catalog.delete_version(reg.version),
        Err(LeylineError::Db(_))
    ));

    let bw = catalog
        .create_version(reg.version, "Noir & Blanc", None)
        .unwrap();
    catalog.set_current_version(reg.asset, bw).unwrap();

    // Deleting the current version falls back to the oldest remaining one.
    catalog.delete_version(bw).unwrap();
    assert_eq!(catalog.current_version(reg.asset).unwrap(), reg.version);
    assert_eq!(catalog.versions(reg.asset).unwrap().len(), 1);
    // The branch's revisions belong to the asset and survive.
    let revisions: i64 = catalog
        .connection()
        .query_row("SELECT COUNT(*) FROM develop_revisions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(revisions, 1);
}

#[test]
fn classement_applies_in_batch_and_rolls_back_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let reg = registered_asset(&mut catalog);
    let bw = catalog
        .create_version(reg.version, "Noir & Blanc", None)
        .unwrap();
    let batch = [reg.version, bw];

    catalog.set_rating(&batch, Some(4)).unwrap();
    catalog
        .set_color_label(&batch, Some(ColorLabel::Blue))
        .unwrap();
    catalog.set_pick(&batch, PickState::Pick).unwrap();

    let versions = catalog.versions(reg.asset).unwrap();
    for v in &versions {
        assert_eq!(v.rating, Some(4));
        assert_eq!(v.color_label, Some(ColorLabel::Blue));
        assert_eq!(v.pick, PickState::Pick);
    }

    // Ratings are 1-5; 0 does not exist (§18).
    assert!(matches!(
        catalog.set_rating(&batch, Some(0)),
        Err(LeylineError::InvalidSettings(_))
    ));

    // One missing version rolls back the whole batch.
    assert!(matches!(
        catalog.set_rating(&[reg.version, VersionId::new(999)], None),
        Err(LeylineError::VersionMissing(id)) if id.get() == 999
    ));
    assert_eq!(catalog.versions(reg.asset).unwrap()[0].rating, Some(4));

    // Clearing works batch-wide.
    catalog.set_rating(&batch, None).unwrap();
    catalog.set_color_label(&batch, None).unwrap();
    assert_eq!(catalog.versions(reg.asset).unwrap()[1].rating, None);
    assert_eq!(catalog.versions(reg.asset).unwrap()[1].color_label, None);
}

#[test]
fn versions_of_a_missing_asset_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = new_catalog(&dir);
    assert!(matches!(
        catalog.versions(AssetId::new(999)),
        Err(LeylineError::AssetMissing(id)) if id.get() == 999
    ));
    assert!(matches!(
        catalog.current_version(AssetId::new(999)),
        Err(LeylineError::AssetMissing(_))
    ));
}
