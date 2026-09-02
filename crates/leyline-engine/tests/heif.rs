//! Integration test: a HEIF file goes all the way through (ADR 0114).
//!
//! Skipped when the build has no HEIF backend, which is how our own packages
//! are built — the point of the feature is that everything else compiles and
//! runs either way.
#![cfg(feature = "heif")]

use leyline_core::PreviewKind;
use leyline_engine::{ImportOptions, Library};

/// The fixture is an **AV1-coded** HEIF: the build machine has no HEVC
/// encoder, and reading one needs none. It exercises every line of the path
/// except which codec plugin libheif reaches for (ADR 0114's stated limit).
fn sample() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets/sample.heic")
}

#[test]
fn a_heic_imports_develops_and_renders_like_any_other_photograph() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "HEIF").unwrap();
    let source = dir.path().join("photo.heic");
    std::fs::copy(sample(), &source).unwrap();

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
    assert_eq!(report.imported.len(), 1, "{:?}", report.skipped);
    let asset = report.imported[0].registered.asset;

    // The pixels come back, at the file's own size — the decoder ran.
    let preview = library.preview(asset, PreviewKind::Small).unwrap();
    assert!(preview.path.is_file());
    let image = leyline_engine::Rgb8::load_png(&preview.path).unwrap();
    assert!(image.width() > 0 && image.height() > 0);

    // And an ordinary edit renders differently, which proves the file is
    // going through the pipeline rather than being copied through it.
    let version = report.imported[0].registered.version;
    // Scoped, and that is not decoration: an `EditSession` holds the catalog
    // lock for its whole lifetime (`docs/engine-api.md` §10.1), so a
    // `library.preview(...)` while one is still alive deadlocks — which is
    // what the first version of this test did.
    {
        let mut session = library.edit(version).unwrap();
        session
            .set(
                leyline_engine::Param::Exposure,
                leyline_engine::Value::Float(1.0),
            )
            .unwrap();
        session.commit().unwrap();
    }
    let brightened = library.preview(asset, PreviewKind::Small).unwrap();
    let after = leyline_engine::Rgb8::load_png(&brightened.path).unwrap();
    let mean = |image: &leyline_engine::Rgb8| -> f64 {
        image.data().iter().map(|v| f64::from(*v)).sum::<f64>() / image.data().len() as f64
    };
    assert!(mean(&after) > mean(&image), "+1 EV brightened nothing");
}
