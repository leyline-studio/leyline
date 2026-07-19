//! Integration tests: preview orchestration (`docs/engine-api.md` §11).

use leyline_catalog::{Catalog, NewPreview};
use leyline_core::{LeylineError, PreviewKind};
use leyline_engine::{DecodeCache, ImportOptions, import, preview};
use leyline_preview::PreviewCache;

fn library(dir: &tempfile::TempDir) -> (Catalog, std::path::PathBuf, PreviewCache) {
    let root = dir.path().join("Library");
    std::fs::create_dir(&root).unwrap();
    let catalog = Catalog::create(&root.join("catalog.db"), "Preview").unwrap();
    let cache = PreviewCache::new(root.join("Cache"));
    (catalog, root, cache)
}

#[test]
fn a_valid_cached_preview_is_served_without_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root, cache) = library(&dir);

    // A PNG imports fine (no RAW identification) but LibRaw cannot decode
    // it — so a cache hit must come back without touching the decoder.
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

    // Pretend a generator already produced the file for the head revision.
    let cached = root.join("Cache/thumbnails/fake.png");
    std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
    std::fs::write(&cached, b"png bytes").unwrap();
    catalog
        .record_preview(&NewPreview {
            asset: registered.asset,
            revision: registered.revision,
            kind: PreviewKind::Thumbnail,
            width: 320,
            height: 213,
            relative_path: "thumbnails/fake.png".to_owned(),
        })
        .unwrap();

    let served = preview(
        &mut catalog,
        &cache,
        &mut DecodeCache::new(2),
        &root,
        registered.asset,
        PreviewKind::Thumbnail,
    )
    .unwrap();
    assert!(!served.freshly_generated);
    assert_eq!(served.path, cached);
    assert_eq!((served.width, served.height), (320, 213));
}

#[test]
fn an_undecodable_asset_reports_decode_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root, cache) = library(&dir);

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
    let asset = report.imported[0].registered.asset;

    // No valid cache entry: generation is attempted and the decoder refuses.
    let err = preview(
        &mut catalog,
        &cache,
        &mut DecodeCache::new(2),
        &root,
        asset,
        PreviewKind::Thumbnail,
    )
    .unwrap_err();
    assert!(matches!(err, LeylineError::DecodeFailed { asset: a, .. } if a == asset));
}

/// Full pipeline against a real RAW file. Run with
/// `LEYLINE_TEST_RAW=/path/to/file.ext cargo test -p leyline-engine -- --ignored`.
#[test]
#[ignore = "needs a real RAW file via LEYLINE_TEST_RAW"]
fn generates_scaled_previews_from_a_real_raw() {
    let raw = std::env::var("LEYLINE_TEST_RAW").expect("set LEYLINE_TEST_RAW");
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root, cache) = library(&dir);

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
    assert_eq!(report.skipped, vec![], "the sample RAW must import");
    let asset = report.imported[0].registered.asset;

    let mut decodes = DecodeCache::new(2);
    let thumb = preview(
        &mut catalog,
        &cache,
        &mut decodes,
        &root,
        asset,
        PreviewKind::Thumbnail,
    )
    .unwrap();
    assert!(thumb.freshly_generated);
    assert!(thumb.path.is_file());
    assert!(thumb.width.max(thumb.height) <= 256);

    // Second call: served from cache, same file.
    let again = preview(
        &mut catalog,
        &cache,
        &mut decodes,
        &root,
        asset,
        PreviewKind::Thumbnail,
    )
    .unwrap();
    assert!(!again.freshly_generated);
    assert_eq!(again.path, thumb.path);
}
