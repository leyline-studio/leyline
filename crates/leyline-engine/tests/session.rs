//! Integration tests: edit sessions and the coalescence policy
//! (`docs/engine-api.md` §10.1, `docs/catalog.md` §17).

use std::time::Duration;

use leyline_catalog::{CHECKSUM_LEN, Catalog, NewAsset, RegisteredAsset};
use leyline_core::{LeylineError, MediaType, Settings, VersionId};
use leyline_engine::{EditSession, Param, Value};

fn catalog_with_asset(dir: &tempfile::TempDir) -> (Catalog, RegisteredAsset) {
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Session").unwrap();
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: "IMG_0001.CR3".to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 32_000_000,
        checksum: [0xAB; CHECKSUM_LEN],
        width: Some(6000),
        height: Some(4000),
        capture_date: None,
        capture_offset_minutes: None,
    };
    let registered = catalog.add_asset(&new).unwrap();
    (catalog, registered)
}

#[test]
fn set_updates_memory_only_and_commit_persists() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    // A drag: many set() calls, zero catalog writes.
    for step in 1..=100 {
        session
            .set(Param::Exposure, Value::Float(step as f64 / 100.0))
            .unwrap();
    }
    assert_eq!(session.settings().exposure, 1.0);
    assert_eq!(session.history().unwrap().len(), 1); // still only the initial

    // Release: one commit point, one revision.
    let head = session.commit().unwrap();
    let history = session.history().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].revision, head);
    assert_eq!(
        Settings::parse(&history[0].settings_json).unwrap().exposure,
        1.0
    );
}

#[test]
fn same_param_within_window_amends_the_head() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    session.set_amend_window(Duration::from_secs(3600));

    session.set(Param::Contrast, Value::Int(10)).unwrap();
    let first = session.commit().unwrap();
    session.set(Param::Contrast, Value::Int(20)).unwrap();
    let second = session.commit().unwrap();

    // One intention, one revision: the head was amended in place.
    assert_eq!(first, second);
    assert_eq!(session.history().unwrap().len(), 2);
    assert_eq!(session.settings().contrast, 20);
}

#[test]
fn window_expiry_or_param_change_creates_new_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();

    // Expired window: same parameter still gets a new revision.
    session.set_amend_window(Duration::ZERO);
    session.set(Param::Contrast, Value::Int(10)).unwrap();
    let first = session.commit().unwrap();
    std::thread::sleep(Duration::from_millis(10));
    session.set(Param::Contrast, Value::Int(20)).unwrap();
    let second = session.commit().unwrap();
    assert_ne!(first, second);

    // Different parameter: new revision even inside a huge window.
    session.set_amend_window(Duration::from_secs(3600));
    session.set(Param::Vibrance, Value::Int(5)).unwrap();
    let third = session.commit().unwrap();
    assert_ne!(second, third);
    session.set(Param::Contrast, Value::Int(30)).unwrap();
    let fourth = session.commit().unwrap();
    assert_ne!(third, fourth);

    assert_eq!(session.history().unwrap().len(), 5);
}

#[test]
fn multi_param_commits_never_amend() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    session.set_amend_window(Duration::from_secs(3600));

    session.set(Param::Contrast, Value::Int(10)).unwrap();
    session.commit().unwrap();
    // Two parameters between commit points: a new intention.
    session.set(Param::Contrast, Value::Int(20)).unwrap();
    session.set(Param::Shadows, Value::Int(15)).unwrap();
    session.commit().unwrap();

    assert_eq!(session.history().unwrap().len(), 3);
}

#[test]
fn empty_commit_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    assert_eq!(session.commit().unwrap(), reg.revision);
    assert_eq!(session.history().unwrap().len(), 1);
}

#[test]
fn undo_and_redo_reload_the_session_state() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    session.set(Param::Exposure, Value::Float(0.5)).unwrap();
    let r1 = session.commit().unwrap();

    // A pending change is committed before undoing: nothing lost.
    session.set(Param::Contrast, Value::Int(25)).unwrap();
    let undone_to = session.undo().unwrap().unwrap();
    assert_eq!(undone_to, r1);
    assert_eq!(session.settings().exposure, 0.5);
    assert_eq!(session.settings().contrast, 0);
    assert_eq!(session.history().unwrap().len(), 2);

    let redone_to = session.redo().unwrap().unwrap();
    assert_eq!(session.settings().contrast, 25);
    assert_ne!(redone_to, r1);
    assert_eq!(session.redo().unwrap(), None);

    // Undo broke the amendment chain: after undo/redo, the next same-param
    // commit is a fresh revision, not an amendment of a shared head.
    session.set_amend_window(Duration::from_secs(3600));
    session.set(Param::Contrast, Value::Int(30)).unwrap();
    session.commit().unwrap();
    assert_eq!(session.history().unwrap().len(), 4);
}

#[test]
fn dropping_the_session_commits_pending_state() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    {
        let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
        session.set(Param::Exposure, Value::Float(1.5)).unwrap();
    } // dropped without an explicit commit

    let head = catalog.version_head(reg.version).unwrap();
    assert_ne!(head, reg.revision);
    let settings = Settings::parse(&catalog.revision(head).unwrap().settings_json).unwrap();
    assert_eq!(settings.exposure, 1.5);
}

#[test]
fn invalid_values_leave_the_state_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    session.set(Param::Contrast, Value::Int(10)).unwrap();

    // Out of range.
    assert!(matches!(
        session.set(Param::Contrast, Value::Int(999)),
        Err(LeylineError::InvalidSettings(_))
    ));
    // Type mismatch.
    assert!(matches!(
        session.set(Param::Exposure, Value::Int(1)),
        Err(LeylineError::InvalidSettings(_))
    ));
    assert_eq!(session.settings().contrast, 10);
    assert_eq!(session.settings().exposure, 0.0);
}

#[test]
fn newer_engine_heads_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    catalog
        .connection()
        .execute(
            "UPDATE develop_revisions SET settings_json = '{\"schema\":99,\"process\":1}'
             WHERE id = ?1",
            [reg.revision.get()],
        )
        .unwrap();

    // §3.4: never edit what a newer engine wrote.
    assert!(matches!(
        EditSession::open(&mut catalog, reg.version),
        Err(LeylineError::NewerSettings { schema: 99, .. })
    ));
}

#[test]
fn opening_a_missing_version_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, _) = catalog_with_asset(&dir);
    assert!(matches!(
        EditSession::open(&mut catalog, VersionId::new(999)),
        Err(LeylineError::VersionMissing(id)) if id.get() == 999
    ));
}
