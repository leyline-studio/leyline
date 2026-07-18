//! Integration tests: export orchestration (`docs/engine-api.md` §12).

use leyline_catalog::Catalog;
use leyline_core::{LeylineError, VersionId};
use leyline_engine::{ImportOptions, export_version, import};
use leyline_export::ExportSettings;

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
