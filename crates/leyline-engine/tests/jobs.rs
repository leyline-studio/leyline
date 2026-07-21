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
fn import_leaves_a_cached_thumbnail_with_no_separate_call() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    let source = dir.path().join("Shoot");
    std::fs::create_dir(&source).unwrap();
    sample_png(&source.join("a.png"));

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

    // No `preview`/`preview_async` call: the import job already rendered
    // the thumbnail as part of itself.
    let cached = library
        .cached_preview(asset, PreviewKind::Thumbnail)
        .unwrap();
    assert!(cached.is_some_and(|file| file.path.is_file()));
}

#[test]
fn import_async_also_leaves_a_cached_thumbnail_once_finished() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();
    let events = library.subscribe();

    let source = dir.path().join("Shoot");
    std::fs::create_dir(&source).unwrap();
    sample_png(&source.join("a.png"));

    let job = library.import_async(
        &source,
        &ImportOptions {
            copy_files: true,
            recursive: false,
        },
    );
    let received = drain_until_finished(&events, job);
    let asset = match received.last() {
        Some(Event::JobFinished {
            result: JobResult::Import(report),
            ..
        }) => report.imported[0].registered.asset,
        other => panic!("expected an import JobFinished, got {other:?}"),
    };

    let cached = library
        .cached_preview(asset, PreviewKind::Thumbnail)
        .unwrap();
    assert!(cached.is_some_and(|file| file.path.is_file()));
}

#[test]
fn a_thumbnail_that_fails_to_render_does_not_fail_the_import() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    let source = dir.path().join("Shoot");
    std::fs::create_dir(&source).unwrap();
    // A non-RAW image import never refuses on an unreadable header
    // (`docs/engine-api.md` §6): it lands in the catalog with no
    // dimensions, and its thumbnail render fails the same way a decode
    // fails at display time today — best-effort, never fatal.
    std::fs::write(source.join("opaque.jpg"), b"not a jpeg").unwrap();

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

    assert_eq!(report.skipped, vec![]);
    assert_eq!(report.imported.len(), 1);
    let asset = report.imported[0].registered.asset;

    // No cached thumbnail (the decode failed), but the asset is still a
    // normal, successfully imported asset — the lazy path picks the
    // failure back up the same way it always has.
    assert_eq!(
        library
            .cached_preview(asset, PreviewKind::Thumbnail)
            .unwrap(),
        None
    );
    assert!(library.preview(asset, PreviewKind::Thumbnail).is_err());
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
    // The import job only renders the `Thumbnail` size class (§11); a
    // larger size class is still nothing cached at all.
    let state = library.preview_state(asset, PreviewKind::Small).unwrap();
    let job = match state {
        Preview::Generating(job) => job,
        other => panic!("expected Generating, got {other:?}"),
    };

    let received = drain_until_finished(&events, job);
    assert!(received.iter().any(|e| matches!(
        e,
        Event::PreviewReady { asset_id, kind } if *asset_id == asset && *kind == PreviewKind::Small
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

#[test]
fn many_concurrent_preview_jobs_all_complete_behind_the_bounded_pool() {
    // `*_async` jobs run on a shared, bounded render pool (§3.3, capped at
    // 16 threads regardless of core count) rather than one raw OS thread
    // per call. This count is comfortably past that cap on any host, so
    // most of these jobs must queue behind the pool instead of all running
    // at once — the point of this test is that queuing loses nothing: every
    // job still gets its `PreviewReady` and `JobFinished`, none silently
    // dropped by a full queue. `library.rs`'s unit tests separately prove
    // the pool's concurrency never exceeds its configured width; this is
    // the public-API-level correctness check under that same load.
    const ASSET_COUNT: usize = 20;

    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Jobs").unwrap();

    let mut assets = Vec::with_capacity(ASSET_COUNT);
    for i in 0..ASSET_COUNT {
        let source = dir.path().join(format!("photo-{i}.png"));
        // Distinct pixels per file: import dedupes by checksum, and
        // `sample_png` alone would make every one of these a duplicate of
        // the first.
        image::save_buffer(
            &source,
            &[i as u8; 4 * 2 * 3],
            4,
            2,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
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
        assets.push(report.imported[0].registered.asset);
    }

    let events = library.subscribe();
    // `Small` was never rendered by import (which only thumbnails, §11),
    // so every one of these is a real render job, not a cache hit.
    let jobs: Vec<(JobId, leyline_core::AssetId)> = assets
        .iter()
        .map(|&asset| (library.preview_async(asset, PreviewKind::Small), asset))
        .collect();

    // Jobs run concurrently behind the pool and can finish in any order, so
    // draining per job (stopping at that job's own `JobFinished`) would
    // silently swallow other jobs' events that arrive in between — those
    // jobs would then starve waiting on events already consumed. Instead,
    // drain the shared receiver once for everyone and bucket events by
    // asset/job as they arrive.
    let mut remaining: std::collections::HashSet<JobId> =
        jobs.iter().map(|&(job, _)| job).collect();
    let mut preview_ready: Vec<(leyline_core::AssetId, PreviewKind)> = Vec::new();
    let mut finished: std::collections::HashMap<JobId, JobResult> =
        std::collections::HashMap::new();

    while !remaining.is_empty() {
        let event = events
            .recv_timeout(Duration::from_secs(30))
            .expect("every job must finish");
        match event {
            Event::PreviewReady { asset_id, kind } => preview_ready.push((asset_id, kind)),
            Event::JobFinished { job_id, result } if remaining.remove(&job_id) => {
                finished.insert(job_id, result);
            }
            _ => {}
        }
    }

    for (job, asset) in jobs {
        assert!(
            preview_ready
                .iter()
                .any(|(id, kind)| *id == asset && *kind == PreviewKind::Small),
            "job {job:?} never emitted its own PreviewReady"
        );
        assert!(
            matches!(finished.get(&job), Some(JobResult::Preview(_))),
            "job {job:?} did not finish with a Preview result"
        );
    }
}
