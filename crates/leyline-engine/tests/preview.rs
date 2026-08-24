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
            pair_companions: true,
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
            pair_companions: true,
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

#[test]
fn generates_a_scaled_preview_from_a_png() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root, cache) = library(&dir);

    // A real 8×4 PNG: the thumbnail must come out scaled, not refused.
    let source = dir.path().join("photo.png");
    image::save_buffer(
        &source,
        &[128u8; 8 * 4 * 3],
        8,
        4,
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
        },
        |_, _| {},
    )
    .unwrap();
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
    assert_eq!((thumb.width, thumb.height), (8, 4));

    // Second call: served from cache.
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
            pair_companions: true,
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

/// The window of ADR 0075, end to end: editing a photo over and over leaves a
/// bounded number of preview files behind, and the current one still serves.
///
/// The failure this guards against is silent by nature — nothing in an
/// interface reports a cache that grew to forty gigabytes, and the cause is
/// unfindable once it has.
#[test]
fn editing_a_photo_many_times_leaves_a_bounded_cache() {
    use leyline_engine::{Library, Param, Value};

    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Lib"), "Retention").unwrap();
    let source = dir.path().join("Shoot");
    std::fs::create_dir(&source).unwrap();
    image::save_buffer(
        source.join("flat.png"),
        &[128u8; 16 * 16 * 3],
        16,
        16,
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
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;

    // Ten edits, each one previewed the way the develop view would.
    for step in 1..=10 {
        {
            let mut session = library.edit(registered.version).unwrap();
            session
                .set(Param::Exposure, Value::Float(f64::from(step) * 0.1))
                .unwrap();
            session.commit().unwrap();
        }
        library
            .preview(registered.asset, PreviewKind::Small)
            .unwrap();
    }

    let files: Vec<_> = walk(&dir.path().join("Lib/Cache")).collect();
    assert!(
        files.len() <= 4,
        "the cache kept {} preview files for one photo: {files:?}",
        files.len()
    );
    // And what it kept is the one that matters.
    let current = library
        .preview(registered.asset, PreviewKind::Small)
        .unwrap();
    assert!(!current.freshly_generated, "the head's preview was evicted");
}

/// Every file under `root`, recursively — the cache lays its files out in
/// subdirectories, so counting the top level would prove nothing.
fn walk(root: &std::path::Path) -> impl Iterator<Item = std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.into_iter()
}
