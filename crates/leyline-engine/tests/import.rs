//! Integration tests: the import pipeline (`docs/engine-api.md` §6).

use std::path::Path;

use leyline_catalog::{Catalog, GridQuery};
use leyline_engine::{ImportOptions, ImportReport, import};

/// A library root with its catalog, and a source directory next to it.
fn library(dir: &tempfile::TempDir) -> (Catalog, std::path::PathBuf) {
    let root = dir.path().join("Library");
    std::fs::create_dir(&root).unwrap();
    let catalog = Catalog::create(&root.join("catalog.db"), "Import").unwrap();
    (catalog, root)
}

fn write(path: &Path, content: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

const COPY: ImportOptions = ImportOptions {
    copy_files: true,
    recursive: true,
};

#[test]
fn a_readable_image_imports_with_its_dimensions() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);

    // One real 6×2 PNG, one unreadable JPEG: both import (non-RAW files
    // are never refused), only the readable one gets dimensions.
    let shoot = dir.path().join("Shoot");
    std::fs::create_dir(&shoot).unwrap();
    image::save_buffer(
        shoot.join("real.png"),
        &[10u8; 6 * 2 * 3],
        6,
        2,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    write(&shoot.join("opaque.jpg"), b"not a jpeg");

    let report = import(&mut catalog, &root, &shoot, &COPY, |_, _| {}).unwrap();
    assert_eq!(report.skipped, vec![]);
    assert_eq!(report.imported.len(), 2);

    for file in &report.imported {
        let details = catalog.asset_details(file.registered.asset).unwrap();
        if file.relative_path.ends_with("real.png") {
            assert_eq!((details.width, details.height), (Some(6), Some(2)));
        } else {
            assert_eq!((details.width, details.height), (None, None));
        }
    }
}

#[test]
fn copy_import_mirrors_the_source_under_photos() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);

    let shoot = dir.path().join("Shoot");
    write(&shoot.join("day1/heron.png"), b"png one");
    write(&shoot.join("day1/eagle.jpg"), b"jpeg two");
    write(&shoot.join("day2/owl.tiff"), b"tiff three");
    write(&shoot.join(".hidden.png"), b"never seen");

    let mut ticks = Vec::new();
    let report = import(&mut catalog, &root, &shoot, &COPY, |done, total| {
        ticks.push((done, total))
    })
    .unwrap();

    assert_eq!(report.skipped, vec![]);
    let paths: Vec<_> = report
        .imported
        .iter()
        .map(|i| i.relative_path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "Photos/day1/eagle.jpg",
            "Photos/day1/heron.png",
            "Photos/day2/owl.tiff"
        ]
    );
    // The files really are in the library, the originals untouched.
    assert!(root.join("Photos/day1/heron.png").is_file());
    assert!(shoot.join("day1/heron.png").is_file());
    // Progress ticked once per file, with a stable total.
    assert_eq!(ticks, [(1, 3), (2, 3), (3, 3)]);
    // Every asset got its develop trio and appears in the grid.
    assert_eq!(catalog.count(&GridQuery::default()).unwrap(), 3);
    assert_eq!(
        catalog
            .asset_relative_path(report.imported[0].registered.asset)
            .unwrap(),
        "Photos/day1/eagle.jpg"
    );
}

#[test]
fn duplicates_unsupported_and_broken_raws_are_skipped_with_reasons() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);

    let shoot = dir.path().join("Shoot");
    write(&shoot.join("heron.png"), b"same content");
    write(&shoot.join("copy-of-heron.png"), b"same content");
    write(&shoot.join("notes.txt"), b"not a photo");
    write(&shoot.join("broken.cr3"), b"not really a raw file");

    let report = import(&mut catalog, &root, &shoot, &COPY, |_, _| {}).unwrap();

    // Files are processed in sorted order: copy-of-heron.png wins the race,
    // heron.png becomes the duplicate.
    assert_eq!(report.imported.len(), 1);
    assert_eq!(report.imported[0].relative_path, "Photos/copy-of-heron.png");

    let reasons: Vec<(&str, &str)> = report
        .skipped
        .iter()
        .map(|s| {
            (
                s.path.file_name().unwrap().to_str().unwrap(),
                s.reason.as_str(),
            )
        })
        .collect();
    assert_eq!(reasons.len(), 3);
    assert!(reasons[0].0 == "broken.cr3" && reasons[0].1.contains("raw identification"));
    assert!(reasons[1].0 == "heron.png" && reasons[1].1.contains("duplicate of asset"));
    assert!(reasons[2].0 == "notes.txt" && reasons[2].1.contains("unsupported extension"));

    // Re-importing the same source: everything is a duplicate now.
    let again = import(&mut catalog, &root, &shoot, &COPY, |_, _| {}).unwrap();
    assert_eq!(again.imported, vec![]);
    assert_eq!(again.skipped.len(), 3 + 1); // + the file imported first time
}

#[test]
fn non_recursive_import_stays_at_the_surface() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);

    let shoot = dir.path().join("Shoot");
    write(&shoot.join("top.png"), b"top");
    write(&shoot.join("deep/nested.png"), b"nested");

    let flat = ImportOptions {
        copy_files: true,
        recursive: false,
    };
    let report = import(&mut catalog, &root, &shoot, &flat, |_, _| {}).unwrap();
    assert_eq!(report.imported.len(), 1);
    assert_eq!(report.imported[0].relative_path, "Photos/top.png");
}

#[test]
fn single_file_import_lands_flat_in_photos() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);
    let file = dir.path().join("lone.png");
    write(&file, b"lone");

    let report = import(&mut catalog, &root, &file, &COPY, |_, _| {}).unwrap();
    assert_eq!(report.imported[0].relative_path, "Photos/lone.png");

    // Importing it again: the copy destination reports the duplicate first.
    write(&dir.path().join("lone2.png"), b"lone");
    let dup = import(
        &mut catalog,
        &root,
        &dir.path().join("lone2.png"),
        &COPY,
        |_, _| {},
    )
    .unwrap();
    assert!(dup.skipped[0].reason.contains("duplicate"));
}

#[test]
fn referencing_requires_files_inside_the_library_root() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);

    let reference = ImportOptions {
        copy_files: false,
        recursive: true,
    };

    // Inside the root: referenced with its own path, no copy made.
    write(&root.join("Originals/heron.png"), b"in place");
    let report = import(
        &mut catalog,
        &root,
        &root.join("Originals"),
        &reference,
        |_, _| {},
    )
    .unwrap();
    assert_eq!(report.imported[0].relative_path, "Originals/heron.png");
    assert!(!root.join("Photos").exists());

    // Outside the root: refused, §2.3 forbids non-relative references.
    let outside = dir.path().join("elsewhere.png");
    write(&outside, b"outside");
    let refused = import(&mut catalog, &root, &outside, &reference, |_, _| {}).unwrap();
    assert!(
        refused.skipped[0]
            .reason
            .contains("outside the library root")
    );
}

#[test]
fn a_missing_source_fails_the_whole_call() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);
    let missing = dir.path().join("nowhere");
    assert!(import(&mut catalog, &root, &missing, &COPY, |_, _| {}).is_err());
    let _: ImportReport = ImportReport::default();
}

#[test]
fn a_non_raw_file_imports_with_its_exif() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);

    let shoot = dir.path().join("Shoot");
    write(&shoot.join("dated.jpg"), &jpeg_with_exif());

    let report = import(&mut catalog, &root, &shoot, &COPY, |_, _| {}).unwrap();
    assert_eq!(report.skipped, vec![]);
    let asset = report.imported[0].registered.asset;
    let details = catalog.asset_details(asset).unwrap();

    // The capture instant is the wall clock the file states, moved by the
    // offset it also states: 14:05:09 +02:00 is 12:05:09 UTC (ADR 0056 §4).
    assert_eq!(details.capture_date, Some(1_710_677_109_000));
    let metadata = details.metadata.expect("a JPEG now has a metadata row");
    // The brand the file repeats in `Model` is dropped, so the body reads
    // the same as it does when its RAW is imported instead.
    assert_eq!(
        metadata.camera.map(|c| (c.manufacturer, c.model)),
        Some(("Nikon".to_owned(), "Z 6".to_owned()))
    );

    // `capture_offset_minutes` has no reader on `AssetDetails`; the column
    // is the point of the assertion, so it is read where it lives.
    let db = rusqlite::Connection::open(root.join("catalog.db")).unwrap();
    let offset: Option<i32> = db
        .query_row(
            "SELECT capture_offset_minutes FROM assets WHERE id = ?1",
            [asset.get()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(offset, Some(120));
}

/// The smallest JPEG that carries an EXIF block: `Make`, `Model`,
/// `DateTimeOriginal` and `OffsetTimeOriginal`, and nothing else.
///
/// Assembled byte by byte rather than copied from a real photo, so the
/// expected values sit next to the assertions above and the test depends on
/// no fixture file.
fn jpeg_with_exif() -> Vec<u8> {
    const MAKE: &[u8] = b"Nikon\0";
    const MODEL: &[u8] = b"Nikon Z 6\0";
    const SHOT_AT: &[u8] = b"2024:03:17 14:05:09\0";
    const OFFSET: &[u8] = b"+02:00\0";

    // Offsets inside the TIFF block: the 8-byte header, IFD0 and its three
    // entries, the Exif sub-IFD and its two, then the values too wide to sit
    // inline in an entry.
    let ifd0_at = 8u32;
    let sub_at = ifd0_at + 2 + 12 * 3 + 4;
    let make_at = sub_at + 2 + 12 * 2 + 4;
    let model_at = make_at + MAKE.len() as u32;
    let shot_at = model_at + MODEL.len() as u32;
    let offset_at = shot_at + SHOT_AT.len() as u32;

    // One IFD entry: tag, type code, component count, then the value itself
    // when it fits in four bytes or its offset when it does not.
    let entry = |tag: u16, kind: u16, count: u32, payload: u32| {
        let mut bytes = tag.to_le_bytes().to_vec();
        bytes.extend(kind.to_le_bytes());
        bytes.extend(count.to_le_bytes());
        bytes.extend(payload.to_le_bytes());
        bytes
    };

    let mut tiff = b"II".to_vec();
    tiff.extend(42u16.to_le_bytes());
    tiff.extend(ifd0_at.to_le_bytes());
    tiff.extend(3u16.to_le_bytes());
    tiff.extend(entry(0x010F, 2, MAKE.len() as u32, make_at));
    tiff.extend(entry(0x0110, 2, MODEL.len() as u32, model_at));
    tiff.extend(entry(0x8769, 4, 1, sub_at));
    tiff.extend(0u32.to_le_bytes());
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(entry(0x9003, 2, SHOT_AT.len() as u32, shot_at));
    tiff.extend(entry(0x9011, 2, OFFSET.len() as u32, offset_at));
    tiff.extend(0u32.to_le_bytes());
    tiff.extend(MAKE);
    tiff.extend(MODEL);
    tiff.extend(SHOT_AT);
    tiff.extend(OFFSET);

    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
    jpeg.extend(((tiff.len() + 8) as u16).to_be_bytes());
    jpeg.extend(b"Exif\0\0");
    jpeg.extend(tiff);
    jpeg.extend([0xFF, 0xD9]);
    jpeg
}
