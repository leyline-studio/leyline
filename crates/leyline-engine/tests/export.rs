//! Integration tests: export orchestration (`docs/engine-api.md` §12).

use leyline_catalog::Catalog;
use leyline_core::{ExportPresetId, LeylineError, VersionId};
use leyline_engine::{ImportOptions, Library, export_batch, export_version, import};
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

#[test]
fn presets_are_validated_stored_and_drive_batches() {
    let dir = tempfile::tempdir().unwrap();
    let mut library = Library::create(&dir.path().join("Library"), "Presets").unwrap();

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

    // The preset drives a batch; an unknown version fails inside the
    // report, an unknown preset fails the whole call.
    let report = library
        .export_with_preset(
            &[VersionId::new(999)],
            preset,
            &dir.path().join("out"),
            |_, _| {},
        )
        .unwrap();
    assert_eq!(report.exported, vec![]);
    assert_eq!(report.failed.len(), 1);

    assert!(matches!(
        library.export_with_preset(
            &[VersionId::new(999)],
            ExportPresetId::new(999),
            &dir.path().join("out"),
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
