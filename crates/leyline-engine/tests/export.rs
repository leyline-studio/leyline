//! Integration tests: export orchestration (`docs/engine-api.md` §12).

use leyline_catalog::Catalog;
use leyline_core::{ExportPresetId, LeylineError, VersionId};
use leyline_engine::{
    ExportRecipe, ExportRequest, ImportOptions, Library, export_batch, export_version, import,
};
use leyline_export::{ExportFormat, ExportSettings};

fn library(dir: &tempfile::TempDir) -> (Catalog, std::path::PathBuf) {
    let root = dir.path().join("Library");
    std::fs::create_dir(&root).unwrap();
    let catalog = Catalog::create(&root.join("catalog.db"), "Export").unwrap();
    (catalog, root)
}

#[test]
fn undecodable_assets_and_missing_versions_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);

    std::fs::write(dir.path().join("photo.png"), b"not decodable").unwrap();
    let report = import(
        &mut catalog,
        &root,
        &dir.path().join("photo.png"),
        &ImportOptions {
            copy_files: true,
            recursive: false,
            pair_companions: true,
            thumbnails: false,
        },
        |_, _| {},
    )
    .unwrap();
    let registered = report.imported[0].registered;
    let out = dir.path().join("out");

    let err = export_version(
        &mut catalog,
        &root,
        registered.version,
        &ExportSettings::default(),
        None,
        &out,
    )
    .unwrap_err();
    assert!(matches!(err, LeylineError::DecodeFailed { .. }));
    // A failed export journals nothing.
    assert_eq!(catalog.export_history(registered.asset).unwrap(), vec![]);

    assert!(matches!(
        export_version(
            &mut catalog,
            &root,
            VersionId::new(999),
            &ExportSettings::default(),
            None,
            &out,
        ),
        Err(LeylineError::VersionMissing(_))
    ));
}

#[test]
fn a_failing_version_does_not_stop_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);

    // Two importable-but-undecodable files (distinct bytes, or the
    // checksum dedup would skip one): every export fails, none interrupts
    // the batch, and progress still counts through.
    for name in ["a.png", "b.png"] {
        std::fs::write(dir.path().join(name), format!("not decodable {name}")).unwrap();
    }
    let report = import(
        &mut catalog,
        &root,
        dir.path(),
        &ImportOptions {
            copy_files: true,
            recursive: false,
            pair_companions: true,
            thumbnails: false,
        },
        |_, _| {},
    )
    .unwrap();
    let versions: Vec<VersionId> = report
        .imported
        .iter()
        .map(|f| f.registered.version)
        .chain([VersionId::new(999)])
        .collect();

    let mut ticks = Vec::new();
    let batch = export_batch(
        &mut catalog,
        &root,
        &versions,
        &ExportSettings::default(),
        None,
        &dir.path().join("out"),
        |done, total| ticks.push((done, total)),
    )
    .unwrap();

    assert_eq!(batch.exported, vec![]);
    assert_eq!(batch.failed.len(), 3);
    assert_eq!(batch.failed[2].version, VersionId::new(999));
    assert!(batch.failed[2].reason.contains("999"));
    assert_eq!(ticks, [(1, 3), (2, 3), (3, 3)]);

    // An invalid recipe fails the whole batch before any version.
    assert!(matches!(
        export_batch(
            &mut catalog,
            &root,
            &versions,
            &ExportSettings {
                quality: 0,
                ..ExportSettings::default()
            },
            None,
            &dir.path().join("out"),
            |_, _| {},
        ),
        Err(LeylineError::InvalidSettings(_))
    ));
}

/// `Library::export` narrows the catalog lock to one version at a time
/// (ADR 0024) rather than holding it for the whole request, as the free
/// `export_batch` function still does. This exercises that facade path
/// end to end: every version in the request must still succeed, get
/// journaled against its own asset, and land under its own filename.
#[test]
fn library_export_narrows_the_lock_and_still_exports_every_version() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Batch").unwrap();
    let source = dir.path().join("Shoot");
    std::fs::create_dir(&source).unwrap();
    for (name, shade) in [("a.png", 200u8), ("b.png", 40u8)] {
        image::save_buffer(
            source.join(name),
            &[shade; 4 * 2 * 3],
            4,
            2,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    }
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: true,
                thumbnails: false,
            },
            |_, _| {},
        )
        .unwrap();
    let versions: Vec<VersionId> = report
        .imported
        .iter()
        .map(|f| f.registered.version)
        .collect();
    assert_eq!(versions.len(), 2);

    let out = dir.path().join("out");
    let mut ticks = Vec::new();
    let batch = library
        .export(
            &ExportRequest {
                versions: versions.clone(),
                recipe: ExportRecipe::Adhoc(ExportSettings::default()),
                destination_dir: out,
                concurrency: None,
            },
            |done, total| ticks.push((done, total)),
        )
        .unwrap();

    assert_eq!(batch.failed, vec![]);
    assert_eq!(batch.exported.len(), 2);
    assert_eq!(ticks, [(1, 2), (2, 2)]);
    for exported in &batch.exported {
        assert!(exported.path.is_file());
    }
    // Distinct source names, so no filename collision and both are
    // journaled against their own asset.
    let assets: Vec<_> = versions
        .iter()
        .map(|&v| library.catalog().version_asset(v).unwrap())
        .collect();
    for asset in assets {
        assert_eq!(library.catalog().export_history(asset).unwrap().len(), 1);
    }
}

/// ADR 0068: the batch runs several photos at once, and that must be an
/// ordering change and nothing else. The same request at concurrency 1 and
/// at 4 has to produce **byte-identical files** — `pipeline.md` §5.1 read at
/// its strongest — plus a report in request order either way.
#[test]
fn concurrency_changes_the_schedule_and_not_one_byte_of_the_output() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Concurrent").unwrap();
    let source = dir.path().join("Shoot");
    std::fs::create_dir(&source).unwrap();
    // Enough files that four in flight actually interleave, each a
    // different image so a mixed-up buffer would show.
    for index in 0..8u8 {
        let shade = 20 + index * 25;
        image::save_buffer(
            source.join(format!("{index}.png")),
            &[shade; 16 * 16 * 3],
            16,
            16,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    }
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: true,
                thumbnails: false,
            },
            |_, _| {},
        )
        .unwrap();
    let versions: Vec<VersionId> = report
        .imported
        .iter()
        .map(|f| f.registered.version)
        .collect();
    assert_eq!(versions.len(), 8);

    let run = |name: &str, concurrency: usize| {
        let out = dir.path().join(name);
        let mut ticks = Vec::new();
        let report = library
            .export(
                &ExportRequest {
                    versions: versions.clone(),
                    recipe: ExportRecipe::Adhoc(ExportSettings::default()),
                    destination_dir: out.clone(),
                    concurrency: Some(concurrency),
                },
                |done, total| ticks.push((done, total)),
            )
            .unwrap();
        // Progress counts completions, whatever order they finish in.
        assert_eq!(
            ticks,
            (1..=8).map(|done| (done, 8)).collect::<Vec<_>>(),
            "at concurrency {concurrency}"
        );
        (report, out)
    };

    let (serial, serial_dir) = run("serial", 1);
    let (concurrent, concurrent_dir) = run("concurrent", 4);

    // The report keeps request order regardless of completion order.
    assert_eq!(serial.failed, vec![]);
    assert_eq!(concurrent.failed, vec![]);
    let requested: Vec<VersionId> = versions.clone();
    let ordered = |report: &leyline_engine::ExportReport| -> Vec<VersionId> {
        report.exported.iter().map(|e| e.version).collect()
    };
    assert_eq!(ordered(&serial), requested);
    assert_eq!(ordered(&concurrent), requested);

    // And the files themselves are identical, byte for byte.
    for exported in &serial.exported {
        let name = exported.path.file_name().unwrap();
        let one = std::fs::read(serial_dir.join(name)).unwrap();
        let many = std::fs::read(concurrent_dir.join(name)).unwrap();
        assert_eq!(
            one,
            many,
            "{} differs between a serial and a concurrent batch",
            name.to_string_lossy()
        );
    }
}

/// Two versions of one asset want the same output name. The later one in
/// request order loses — and it has to lose *deterministically*, which is
/// why the batch reserves names before it renders anything (ADR 0068 §2):
/// left to the filesystem, two concurrent renders could both pass the
/// existence check and one would silently overwrite the other.
#[test]
fn a_name_taken_by_an_earlier_version_of_the_batch_fails_the_later_one() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Collide").unwrap();
    let source = dir.path().join("Shoot");
    std::fs::create_dir(&source).unwrap();
    image::save_buffer(
        source.join("only.png"),
        &[128; 8 * 8 * 3],
        8,
        8,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: true,
                thumbnails: false,
            },
            |_, _| {},
        )
        .unwrap();
    let first = report.imported[0].registered.version;
    // A virtual copy of the same asset: same file on disk, so same stem.
    let second = library
        .catalog_mut()
        .create_version(first, "Copy", None)
        .unwrap();

    for concurrency in [1, 4] {
        let out = dir.path().join(format!("out{concurrency}"));
        let report = library
            .export(
                &ExportRequest {
                    versions: vec![first, second],
                    recipe: ExportRecipe::Adhoc(ExportSettings::default()),
                    destination_dir: out,
                    concurrency: Some(concurrency),
                },
                |_, _| {},
            )
            .unwrap();
        assert_eq!(
            report
                .exported
                .iter()
                .map(|e| e.version)
                .collect::<Vec<_>>(),
            vec![first],
            "the first version in request order wins, at concurrency {concurrency}"
        );
        assert_eq!(
            report.failed.iter().map(|f| f.version).collect::<Vec<_>>(),
            vec![second],
            "and the second loses, at concurrency {concurrency}"
        );
        assert!(report.failed[0].reason.contains("never overwrite"));
    }
}

#[test]
fn presets_are_validated_stored_and_drive_batches() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Presets").unwrap();

    // The facade refuses an invalid recipe instead of storing it.
    assert!(matches!(
        library.create_export_preset(
            "Broken",
            &ExportSettings {
                quality: 101,
                ..ExportSettings::default()
            },
        ),
        Err(LeylineError::InvalidSettings(_))
    ));

    let settings = ExportSettings {
        format: ExportFormat::Png,
        max_edge: Some(1024),
        ..ExportSettings::default()
    };
    let preset = library.create_export_preset("Web", &settings).unwrap();
    let stored = library.export_presets().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].preset, preset);
    assert_eq!(
        ExportSettings::parse(&stored[0].settings_json).unwrap(),
        settings
    );

    // The preset drives a request; an unknown version fails inside the
    // report, an unknown preset fails the whole call.
    let report = library
        .export(
            &ExportRequest {
                versions: vec![VersionId::new(999)],
                recipe: ExportRecipe::Preset(preset),
                destination_dir: dir.path().join("out"),
                concurrency: None,
            },
            |_, _| {},
        )
        .unwrap();
    assert_eq!(report.exported, vec![]);
    assert_eq!(report.failed.len(), 1);

    assert!(matches!(
        library.export(
            &ExportRequest {
                versions: vec![VersionId::new(999)],
                recipe: ExportRecipe::Preset(ExportPresetId::new(999)),
                destination_dir: dir.path().join("out"),
                concurrency: None,
            },
            |_, _| {},
        ),
        Err(LeylineError::ExportPresetMissing(_))
    ));
}

#[test]
fn exports_a_png_source_to_jpeg_and_journals_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);

    let source = dir.path().join("photo.png");
    image::save_buffer(
        &source,
        &[200u8; 4 * 2 * 3],
        4,
        2,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    let report = import(
        &mut catalog,
        &root,
        &source,
        &ImportOptions {
            copy_files: true,
            recursive: false,
            pair_companions: true,
            thumbnails: false,
        },
        |_, _| {},
    )
    .unwrap();
    let registered = report.imported[0].registered;

    let written = export_version(
        &mut catalog,
        &root,
        registered.version,
        &ExportSettings::default(),
        None,
        &dir.path().join("out"),
    )
    .unwrap();
    let bytes = std::fs::read(&written).unwrap();
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "JPEG SOI marker");

    let history = catalog.export_history(registered.asset).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].format, "jpg");
}

/// Full pipeline against a real RAW file. Run with
/// `LEYLINE_TEST_RAW=/path/to/file.ext cargo test -p leyline-engine -- --ignored`.
#[test]
#[ignore = "needs a real RAW file via LEYLINE_TEST_RAW"]
fn exports_a_real_raw_to_jpeg_and_journals_it() {
    let raw = std::env::var("LEYLINE_TEST_RAW").expect("set LEYLINE_TEST_RAW");
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);

    let report = import(
        &mut catalog,
        &root,
        std::path::Path::new(&raw),
        &ImportOptions {
            copy_files: true,
            recursive: false,
            pair_companions: true,
            thumbnails: false,
        },
        |_, _| {},
    )
    .unwrap();
    let registered = report.imported[0].registered;

    let settings = ExportSettings {
        max_edge: Some(1024),
        ..ExportSettings::default()
    };
    let out = dir.path().join("out");
    let written = export_version(
        &mut catalog,
        &root,
        registered.version,
        &settings,
        None,
        &out,
    )
    .unwrap();

    assert!(written.is_file());
    let bytes = std::fs::read(&written).unwrap();
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "JPEG SOI marker");

    let history = catalog.export_history(registered.asset).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].format, "jpg");

    // Never overwrite: the same export twice is refused.
    assert!(matches!(
        export_version(
            &mut catalog,
            &root,
            registered.version,
            &settings,
            None,
            &out,
        ),
        Err(LeylineError::Io(_))
    ));
}
