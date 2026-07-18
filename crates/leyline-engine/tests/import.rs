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
