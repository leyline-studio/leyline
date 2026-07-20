//! Integration tests: jobs and events (`docs/engine-api.md` §3).

use std::time::Duration;

use leyline_core::{JobId, PreviewKind, VersionId};
use leyline_engine::{Event, ImportOptions, JobResult, Library, Preview};
use leyline_export::ExportSettings;

/// Events are notifications across threads: the handle must be shareable.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Library>();
    assert_send_sync::<Event>();
};

/// Collects events until `JobFinished` for `job` arrives (or times out),
/// returning everything received in order.
fn drain_until_finished(receiver: &std::sync::mpsc::Receiver<Event>, job: JobId) -> Vec<Event> {
    let mut events = Vec::new();
    loop {
        let event = receiver
            .recv_timeout(Duration::from_secs(30))
            .expect("the job must finish");
        let done = matches!(&event, Event::JobFinished { job_id, .. } if *job_id == job);
        events.push(event);
        if done {
            return events;
        }
    }
}

/// A real 4×2 PNG the import can measure and the renderer can decode.
fn sample_png(path: &std::path::Path) {
    image::save_buffer(
        path,
        &[90u8; 4 * 2 * 3],
        4,
        2,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
}

#[test]
fn an_import_job_progresses_announces_assets_and_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();
    let events = library.subscribe();

    let source = dir.path().join("Shoot");
    std::fs::create_dir(&source).unwrap();
    sample_png(&source.join("a.png"));
    // A second, distinct file: different pixels so checksums differ.
    image::save_buffer(
        source.join("b.png"),
        &[10u8; 4 * 2 * 3],
        4,
        2,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();

    let job = library.import_async(
        &source,
        &ImportOptions {
            copy_files: true,
            recursive: false,
        },
    );
    let received = drain_until_finished(&events, job);

    let progress: Vec<(u64, u64)> = received
        .iter()
        .filter_map(|e| match e {
            Event::JobProgress {
                job_id,
                done,
                total,
            } if *job_id == job => Some((*done, *total)),
            _ => None,
        })
        .collect();
    assert_eq!(progress, vec![(1, 2), (2, 2)]);

    let added: Vec<usize> = received
        .iter()
        .filter_map(|e| match e {
            Event::AssetsAdded { asset_ids } => Some(asset_ids.len()),
            _ => None,
        })
        .collect();
    assert_eq!(added, vec![2]);

    match received.last() {
        Some(Event::JobFinished {
            result: JobResult::Import(report),
            ..
        }) => {
            assert_eq!(report.imported.len(), 2);
            assert_eq!(report.skipped, vec![]);
        }
        other => panic!("expected an import JobFinished, got {other:?}"),
    }

    // The catalog is shared: a clone sees the imported assets.
    assert_eq!(
        library
            .clone()
            .catalog()
            .count(&leyline_catalog::GridQuery::default())
            .unwrap(),
        2
    );
}

#[test]
fn a_preview_job_emits_preview_ready_then_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    let source = dir.path().join("photo.png");
    sample_png(&source);
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let asset = report.imported[0].registered.asset;

    let events = library.subscribe();
    let job = library.preview_async(asset, PreviewKind::Thumbnail);
    let received = drain_until_finished(&events, job);

    assert!(received.iter().any(|e| matches!(
        e,
        Event::PreviewReady { asset_id, kind } if *asset_id == asset && *kind == PreviewKind::Thumbnail
    )));
    match received.last() {
        Some(Event::JobFinished {
            result: JobResult::Preview(file),
            ..
        }) => assert!(file.path.is_file()),
        other => panic!("expected a preview JobFinished, got {other:?}"),
    }
}

#[test]
fn preview_state_reports_generating_with_nothing_cached() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    let source = dir.path().join("photo.png");
    sample_png(&source);
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let asset = report.imported[0].registered.asset;

    let events = library.subscribe();
    let state = library
        .preview_state(asset, PreviewKind::Thumbnail)
        .unwrap();
    let job = match state {
        Preview::Generating(job) => job,
        other => panic!("expected Generating, got {other:?}"),
    };

    let received = drain_until_finished(&events, job);
    assert!(received.iter().any(|e| matches!(
        e,
        Event::PreviewReady { asset_id, kind } if *asset_id == asset && *kind == PreviewKind::Thumbnail
    )));
}

#[test]
fn preview_state_reports_ready_once_the_head_revision_is_cached() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    let source = dir.path().join("photo.png");
    sample_png(&source);
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let asset = report.imported[0].registered.asset;

    // Generate the cache entry for the current head synchronously first.
    let generated = library.preview(asset, PreviewKind::Thumbnail).unwrap();

    let state = library
        .preview_state(asset, PreviewKind::Thumbnail)
        .unwrap();
    match state {
        Preview::Ready(path) => assert_eq!(path, generated.path),
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[test]
fn preview_state_reports_stale_and_starts_a_regeneration_job_after_an_edit() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    let source = dir.path().join("photo.png");
    sample_png(&source);
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;

    // Cache a preview for the initial revision.
    let old = library
        .preview(registered.asset, PreviewKind::Thumbnail)
        .unwrap();

    // Commit a new revision: the head moves on, the cached file above is
    // now for a non-head revision — stale but still displayable.
    {
        let mut session = library.edit(registered.version).unwrap();
        session
            .set(
                leyline_engine::Param::Exposure,
                leyline_engine::Value::Float(0.5),
            )
            .unwrap();
        session.commit().unwrap();
    }

    let events = library.subscribe();
    let state = library
        .preview_state(registered.asset, PreviewKind::Thumbnail)
        .unwrap();
    let (path, job) = match state {
        Preview::Stale { path, job } => (path, job),
        other => panic!("expected Stale, got {other:?}"),
    };
    assert_eq!(path, old.path);

    let received = drain_until_finished(&events, job);
    assert!(received.iter().any(|e| matches!(
        e,
        Event::PreviewReady { asset_id, kind }
            if *asset_id == registered.asset && *kind == PreviewKind::Thumbnail
    )));
}

#[test]
fn an_export_job_reports_per_version_failures_in_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();
    let events = library.subscribe();

    // An unknown version fails inside the report, not the job.
    let job = library.export_async(
        vec![VersionId::new(999)],
        ExportSettings::default(),
        dir.path().join("out"),
    );
    let received = drain_until_finished(&events, job);

    match received.last() {
        Some(Event::JobFinished {
            result: JobResult::Export(report),
            ..
        }) => {
            assert_eq!(report.exported, vec![]);
            assert_eq!(report.failed.len(), 1);
        }
        other => panic!("expected an export JobFinished, got {other:?}"),
    }
}

#[test]
fn a_preset_job_reports_per_version_failures_and_notifies() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    let source = dir.path().join("photo.png");
    sample_png(&source);
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;

    let preset = library
        .create_preset(
            "Contraste",
            registered.version,
            &[leyline_core::SettingsGroup::WhiteBalance],
        )
        .unwrap();

    let events = library.subscribe();
    // One known version and one unknown: the batch still finishes, the
    // unknown one lands in the report, not in a job-level `Failed`.
    let job = library.apply_preset_async(preset, vec![registered.version, VersionId::new(999)]);
    let received = drain_until_finished(&events, job);

    assert!(received.iter().any(|e| matches!(
        e,
        Event::VersionChanged { version_id } if *version_id == registered.version
    )));
    match received.last() {
        Some(Event::JobFinished {
            result: JobResult::Preset(report),
            ..
        }) => {
            assert_eq!(report.applied, vec![registered.version]);
            assert_eq!(report.failed.len(), 1);
        }
        other => panic!("expected a preset JobFinished, got {other:?}"),
    }
}

#[test]
fn a_reprocess_job_migrates_versions_and_reports_failures() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    let source = dir.path().join("photo.png");
    sample_png(&source);
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;

    // Simulate a photo imported before the current process version existed.
    library
        .catalog_mut()
        .connection()
        .execute(
            "UPDATE develop_revisions SET settings_json = '{\"schema\":1,\"process\":1}'
             WHERE id = ?1",
            [registered.revision.get()],
        )
        .unwrap();

    let events = library.subscribe();
    // One known version and one unknown: the batch still finishes, the
    // unknown one lands in the report, not in a job-level `Failed`.
    let job = library.reprocess_async(vec![registered.version, VersionId::new(999)]);
    let received = drain_until_finished(&events, job);

    assert!(received.iter().any(|e| matches!(
        e,
        Event::VersionChanged { version_id } if *version_id == registered.version
    )));
    match received.last() {
        Some(Event::JobFinished {
            result: JobResult::Reprocess(report),
            ..
        }) => {
            assert_eq!(report.reprocessed, vec![registered.version]);
            assert_eq!(report.failed.len(), 1);
        }
        other => panic!("expected a reprocess JobFinished, got {other:?}"),
    }
}

#[test]
fn facade_writes_and_edit_sessions_notify_subscribers() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    let source = dir.path().join("photo.png");
    sample_png(&source);
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;
    let events = library.subscribe();

    // Classement: one VersionChanged per version of the batch.
    library.set_rating(&[registered.version], Some(4)).unwrap();
    assert_eq!(
        events.try_recv(),
        Ok(Event::VersionChanged {
            version_id: registered.version
        })
    );

    // Keywords: AssetsChanged with the batch.
    let keyword = library.create_keyword(None, "Nature").unwrap();
    library.add_keyword(&[registered.asset], keyword).unwrap();
    assert_eq!(
        events.try_recv(),
        Ok(Event::AssetsChanged {
            asset_ids: vec![registered.asset]
        })
    );

    // An edit session notifies on commit and on undo, not on open/drop.
    {
        let mut session = library.edit(registered.version).unwrap();
        session
            .set(
                leyline_engine::Param::Exposure,
                leyline_engine::Value::Float(0.5),
            )
            .unwrap();
        session.commit().unwrap();
        assert_eq!(
            events.try_recv(),
            Ok(Event::VersionChanged {
                version_id: registered.version
            })
        );
        session.undo().unwrap();
        assert_eq!(
            events.try_recv(),
            Ok(Event::VersionChanged {
                version_id: registered.version
            })
        );
    }
    assert!(events.try_recv().is_err(), "no event without a write");
}

#[test]
fn a_dropped_subscriber_never_blocks_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    // Subscribe and drop immediately: later jobs must not care.
    drop(library.subscribe());
    let alive = library.subscribe();

    let source = dir.path().join("photo.png");
    sample_png(&source);
    let job = library.import_async(
        &source,
        &ImportOptions {
            copy_files: true,
            recursive: false,
        },
    );
    let received = drain_until_finished(&alive, job);
    assert!(matches!(
        received.last(),
        Some(Event::JobFinished {
            result: JobResult::Import(_),
            ..
        })
    ));
}
