//! Integration tests: looking at a folder before importing it (ADR 0065).

use std::path::{Path, PathBuf};

use leyline_catalog::{Catalog, GridQuery};
use leyline_engine::{ImportOptions, ScanOptions, import, import_files, scan};

/// A library root with its catalog, and a source directory next to it.
fn library(dir: &tempfile::TempDir) -> (Catalog, PathBuf) {
    let root = dir.path().join("Library");
    std::fs::create_dir(&root).unwrap();
    let catalog = Catalog::create(&root.join("catalog.db"), "Scan").unwrap();
    (catalog, root)
}

/// A real PNG of the given size, so the scan has something to reduce.
fn png(path: &Path, width: u32, height: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let pixels = vec![90u8; width as usize * height as usize * 3];
    image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgb8).unwrap();
}

const COPY: ImportOptions = ImportOptions {
    copy_files: true,
    recursive: true,
    pair_companions: true,
};

const LOOK: ScanOptions = ScanOptions {
    recursive: true,
    thumbnails: true,
};

#[test]
fn a_scan_describes_what_an_import_would_take_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);
    let shoot = dir.path().join("Shoot");
    png(&shoot.join("a.png"), 8, 4);
    png(&shoot.join("deeper/b.png"), 4, 8);
    std::fs::write(shoot.join("notes.txt"), b"not a photo").unwrap();

    let mut steps = Vec::new();
    let candidates = scan(&catalog, &shoot, &LOOK, |done, total| {
        steps.push((done, total));
    })
    .unwrap();

    // The text file is enumerated — it is what `total` counts — but never
    // described: an import would not take it either.
    assert_eq!(steps.last(), Some(&(3, 3)));
    assert_eq!(
        candidates
            .iter()
            .map(|c| c.filename.as_str())
            .collect::<Vec<_>>(),
        ["a.png", "b.png"]
    );
    assert!(candidates.iter().all(|c| c.thumbnail.is_some()));
    assert!(candidates.iter().all(|c| !c.already_imported));
    assert_eq!(
        candidates[0].file_size,
        std::fs::metadata(shoot.join("a.png")).unwrap().len()
    );

    // Nothing was written: no asset, and no file copied into the library.
    assert_eq!(catalog.count(&GridQuery::default()).unwrap(), 0);
    assert!(!root.join("Photos").exists());

    // And what it announced is exactly what the import then takes.
    let report = import(&mut catalog, &root, &shoot, &COPY, |_, _| {}).unwrap();
    assert_eq!(report.imported.len(), candidates.len());
}

#[test]
fn a_scan_can_be_asked_for_nothing_but_the_facts() {
    let dir = tempfile::tempdir().unwrap();
    let (catalog, _root) = library(&dir);
    let shoot = dir.path().join("Shoot");
    png(&shoot.join("a.png"), 8, 4);
    png(&shoot.join("deeper/b.png"), 4, 8);

    let flat = ScanOptions {
        recursive: false,
        thumbnails: false,
    };
    let candidates = scan(&catalog, &shoot, &flat, |_, _| {}).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].filename, "a.png");
    assert_eq!(candidates[0].thumbnail, None);
}

#[test]
fn an_already_imported_file_is_marked_but_still_offered() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);
    let shoot = dir.path().join("Shoot");
    png(&shoot.join("kept.png"), 8, 4);
    png(&shoot.join("new.png"), 6, 6);

    import(
        &mut catalog,
        &root,
        &shoot.join("kept.png"),
        &COPY,
        |_, _| {},
    )
    .unwrap();

    let candidates = scan(&catalog, &shoot, &LOOK, |_, _| {}).unwrap();
    let marked: Vec<_> = candidates
        .iter()
        .map(|c| (c.filename.as_str(), c.already_imported))
        .collect();
    // Still listed, and still importable: the mark is a hint, and the
    // checksum at import is the only verdict (ADR 0065 §3).
    assert_eq!(marked, [("kept.png", true), ("new.png", false)]);

    let report = import_files(
        &mut catalog,
        &root,
        &shoot,
        &[shoot.join("kept.png")],
        &COPY,
        |_, _| {},
    )
    .unwrap();
    assert_eq!(report.imported, vec![]);
    assert_eq!(report.skipped.len(), 1);
    assert!(report.skipped[0].reason.contains("duplicate"));
}

#[test]
fn importing_a_list_takes_that_list_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);
    let shoot = dir.path().join("Shoot");
    // Different sizes, so no two files share a checksum — this test is
    // about what the list selects, not about duplicate detection.
    for (name, edge) in [("a.png", 4), ("b.png", 5), ("c.png", 6)] {
        png(&shoot.join(name), edge, edge);
    }

    let chosen = vec![shoot.join("a.png"), shoot.join("c.png")];
    let report = import_files(&mut catalog, &root, &shoot, &chosen, &COPY, |_, _| {}).unwrap();

    assert_eq!(report.skipped, vec![]);
    let imported: Vec<_> = report
        .imported
        .iter()
        .map(|f| f.relative_path.as_str())
        .collect();
    assert_eq!(imported, ["Photos/a.png", "Photos/c.png"]);
    assert_eq!(catalog.count(&GridQuery::default()).unwrap(), 2);
    assert!(!root.join("Photos/b.png").exists());
}

#[test]
fn a_file_outside_the_source_is_refused_not_filed_somewhere_odd() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, root) = library(&dir);
    let shoot = dir.path().join("Shoot");
    png(&shoot.join("inside.png"), 4, 4);
    let stray = dir.path().join("Elsewhere/stray.png");
    png(&stray, 4, 4);

    let report = import_files(
        &mut catalog,
        &root,
        &shoot,
        &[shoot.join("inside.png"), stray],
        &COPY,
        |_, _| {},
    )
    .unwrap();

    assert_eq!(report.imported.len(), 1);
    assert_eq!(report.skipped.len(), 1);
    assert!(
        report.skipped[0].reason.contains("outside"),
        "{}",
        report.skipped[0].reason
    );
}

/// The embedded-preview path on a real RAW file: run with
/// `LEYLINE_TEST_RAW=/path/to/file.CR2 cargo test -p leyline-engine --test
/// scan -- --ignored`.
#[test]
#[ignore = "needs a real RAW file via LEYLINE_TEST_RAW"]
fn a_raw_candidate_shows_the_preview_its_camera_wrote() {
    let source = std::env::var("LEYLINE_TEST_RAW").expect("set LEYLINE_TEST_RAW");
    let source = PathBuf::from(source);
    let dir = tempfile::tempdir().unwrap();
    let (catalog, _root) = library(&dir);

    let candidates = scan(&catalog, &source, &LOOK, |_, _| {}).unwrap();
    assert_eq!(candidates.len(), 1);
    let candidate = &candidates[0];
    assert!(candidate.camera.is_some(), "a RAW file names its body");
    assert!(candidate.capture_date.is_some());

    let thumbnail = candidate.thumbnail.as_ref().expect("this body writes one");
    let image = image::load_from_memory(thumbnail).expect("a decodable JPEG");
    assert!(image.width().max(image.height()) <= 256);

    // Oriented like the photo itself: a contact sheet of sideways
    // thumbnails would be worse than none (ADR 0065 §2). Compared against
    // the full decode, which LibRaw rotates for us.
    let full = leyline_raw::decode(&source, &leyline_raw::DecodeParams::default()).unwrap();
    let portrait = |w: u32, h: u32| w < h;
    assert_eq!(
        portrait(image.width(), image.height()),
        portrait(full.image.width, full.image.height),
        "thumbnail {}x{}, photo {}x{}",
        image.width(),
        image.height(),
        full.image.width,
        full.image.height
    );
}
