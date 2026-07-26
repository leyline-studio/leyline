//! Integration tests: edit sessions and the coalescence policy
//! (`docs/engine-api.md` §10.1, `docs/catalog.md` §17).

use std::time::Duration;

use leyline_catalog::{CHECKSUM_LEN, Catalog, NewAsset, RegisteredAsset};
use leyline_core::{
    ColorGrading, HslBand, LeylineError, LocalAdjustment, LocalAdjustmentValues, Mask, MediaType,
    Settings, VersionId,
};
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
    let registered = catalog
        .add_asset(&new, &leyline_engine::neutral_settings())
        .unwrap();
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
fn checkout_jumps_directly_to_a_revision_and_commits_pending_state_first() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    session.set(Param::Exposure, Value::Float(0.5)).unwrap();
    let r1 = session.commit().unwrap();
    session.set(Param::Contrast, Value::Int(25)).unwrap();
    let r2 = session.commit().unwrap();

    // A pending change is committed before jumping: nothing lost.
    session.set(Param::Vibrance, Value::Int(10)).unwrap();
    session.checkout(r1).unwrap();
    // history() walks backward from the head: r2 and the committed vibrance
    // edit are forward descendants of r1, correctly excluded.
    assert_eq!(session.history().unwrap().len(), 2); // initial, r1
    assert_eq!(session.settings().exposure, 0.5);
    assert_eq!(session.settings().contrast, 0);

    // Jumping forward again to r2 works too, unlike undo/redo's one-step
    // movement — `checkout` isn't limited to adjacent revisions.
    let jumped = session.checkout(r2).unwrap();
    assert_eq!(jumped, r2);
    assert_eq!(session.settings().contrast, 25);
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
fn reprocess_migrates_an_old_process_to_a_new_revision() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);
    catalog
        .connection()
        .execute(
            "UPDATE develop_revisions SET settings_json = '{\"schema\":1,\"exposure\":0.4}'
             WHERE id = ?1",
            [reg.revision.get()],
        )
        .unwrap();

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    assert!(
        session.settings().stages.is_empty(),
        "the stored revision records no stage version"
    );

    let head = session.reprocess().unwrap();
    assert_ne!(head, reg.revision, "reprocessing writes a new revision");
    assert_eq!(session.settings().stages.get("gains"), Some(&1));
    assert_eq!(
        session.settings().exposure,
        0.4,
        "parameter values are untouched"
    );
    assert_eq!(session.history().unwrap().len(), 2);
}

#[test]
fn reprocess_on_a_head_already_at_the_current_stage_versions_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    let head = session.reprocess().unwrap();
    assert_eq!(head, reg.revision, "already current: no new revision");
    assert_eq!(session.history().unwrap().len(), 1);
}

#[test]
fn reprocess_commits_pending_state_first() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);
    catalog
        .connection()
        .execute(
            "UPDATE develop_revisions SET settings_json = '{\"schema\":1,\"exposure\":0.4}'
             WHERE id = ?1",
            [reg.revision.get()],
        )
        .unwrap();

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    session.set(Param::Contrast, Value::Int(20)).unwrap();
    session.reprocess().unwrap();

    assert_eq!(session.settings().contrast, 20);
    assert_eq!(session.settings().stages.get("contrast"), Some(&1));
    // The pending contrast edit is committed on its own revision, and that
    // commit records the current stage versions — so the reprocess right
    // behind it finds nothing left to migrate and writes nothing.
    assert_eq!(session.history().unwrap().len(), 2);
}

fn a_radial_adjustment() -> LocalAdjustment {
    LocalAdjustment {
        mask: Mask::Radial {
            cx: 0.5,
            cy: 0.5,
            rx: 0.2,
            ry: 0.2,
            angle: 0.0,
            feather: 0.3,
            inverted: false,
        },
        opacity: 1.0,
        adjustments: LocalAdjustmentValues {
            exposure: Some(0.5),
            ..LocalAdjustmentValues::default()
        },
    }
}

#[test]
fn local_adjustment_appends_at_the_current_length_and_commits_as_one_tool() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    session
        .set(
            Param::LocalAdjustment(0),
            Value::LocalAdjustment(Some(a_radial_adjustment())),
        )
        .unwrap();
    assert_eq!(session.settings().local_adjustments.len(), 1);
    session.commit().unwrap();
    assert_eq!(session.history().unwrap().len(), 2);
}

#[test]
fn local_adjustment_replaces_the_entry_at_an_existing_index() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    session
        .set(
            Param::LocalAdjustment(0),
            Value::LocalAdjustment(Some(a_radial_adjustment())),
        )
        .unwrap();
    let mut replacement = a_radial_adjustment();
    replacement.opacity = 0.4;
    session
        .set(
            Param::LocalAdjustment(0),
            Value::LocalAdjustment(Some(replacement)),
        )
        .unwrap();
    assert_eq!(session.settings().local_adjustments.len(), 1);
    assert_eq!(session.settings().local_adjustments[0].opacity, 0.4);
}

#[test]
fn local_adjustment_none_removes_the_entry() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    session
        .set(
            Param::LocalAdjustment(0),
            Value::LocalAdjustment(Some(a_radial_adjustment())),
        )
        .unwrap();
    session
        .set(Param::LocalAdjustment(0), Value::LocalAdjustment(None))
        .unwrap();
    assert!(session.settings().local_adjustments.is_empty());
}

#[test]
fn local_adjustment_out_of_bounds_index_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    assert!(matches!(
        session.set(
            Param::LocalAdjustment(1),
            Value::LocalAdjustment(Some(a_radial_adjustment())),
        ),
        Err(LeylineError::InvalidSettings(_))
    ));
    assert!(session.settings().local_adjustments.is_empty());
}

#[test]
fn hsl_band_out_of_bounds_index_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    assert!(matches!(
        session.set(
            Param::HslBand(8),
            Value::HslBand(HslBand {
                hue: 10,
                saturation: 0,
                luminance: 0,
            }),
        ),
        Err(LeylineError::InvalidSettings(_))
    ));
    assert_eq!(session.settings().hsl, [HslBand::default(); 8]);
}

#[test]
fn hsl_band_commits_at_its_index_only() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    session
        .set(
            Param::HslBand(3),
            Value::HslBand(HslBand {
                hue: 0,
                saturation: 40,
                luminance: 0,
            }),
        )
        .unwrap();
    session.commit().unwrap();

    assert_eq!(session.settings().hsl[3].saturation, 40);
    for (i, band) in session.settings().hsl.iter().enumerate() {
        if i != 3 {
            assert_eq!(*band, HslBand::default());
        }
    }
}

#[test]
fn color_grading_commits_as_one_tool() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, reg) = catalog_with_asset(&dir);

    let mut session = EditSession::open(&mut catalog, reg.version).unwrap();
    let grading = ColorGrading {
        balance: 20,
        blending: 60,
        ..ColorGrading::default()
    };
    session
        .set(Param::ColorGrading, Value::ColorGrading(grading))
        .unwrap();
    session.commit().unwrap();

    assert_eq!(session.settings().color_grading, grading);
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
