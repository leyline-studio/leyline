//! Integration tests: renaming files on disk (ADR 0100).
//!
//! The only engine operation that moves a user's original, so what is
//! tested here is mostly what it *refuses* to do.

use leyline_engine::{ImportOptions, Library};

const COPY: ImportOptions = ImportOptions {
    copy_files: true,
    recursive: true,
    pair_companions: true,
    thumbnails: false,
};

/// A library with one imported PNG.
fn library_with(dir: &std::path::Path, names: &[&str]) -> Library {
    let library = Library::create(&dir.join("Lib"), "Rename").unwrap();
    let source = dir.join("Source");
    std::fs::create_dir_all(&source).unwrap();
    // Each file gets its own pixels: identical content would be refused
    // at import as a duplicate (ADR 0095), and the fixture would then hold
    // fewer photos than it looks like it does.
    for (i, name) in names.iter().enumerate() {
        image::save_buffer(
            source.join(name),
            &[(40 + i * 30) as u8; 4 * 4 * 3],
            4,
            4,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    }
    library.import(&source, &COPY, |_, _| {}).unwrap();
    library
}

#[test]
fn a_template_renames_the_file_and_the_catalog_together() {
    let dir = tempfile::tempdir().unwrap();
    let library = library_with(dir.path(), &["a.png", "b.png"]);
    let assets: Vec<_> = library
        .catalog()
        .grid(&leyline_catalog::GridQuery::default())
        .unwrap()
        .iter()
        .map(|item| item.asset_id)
        .collect();

    let report = library.rename(&assets, "Heron-{seq}").unwrap();
    assert_eq!(report.renamed.len(), 2, "{report:?}");
    assert!(report.failed.is_empty(), "{report:?}");

    // The catalog says the new name, and the file is there under it.
    for renamed in &report.renamed {
        let details = library.catalog().asset_details(renamed.asset).unwrap();
        assert_eq!(details.filename, renamed.to);
        assert!(
            library.locate(renamed.asset).unwrap().is_file(),
            "the catalog must name a file that exists: {}",
            renamed.to
        );
    }
    // `{seq}` counted within the batch, zero-padded.
    let names: Vec<_> = report.renamed.iter().map(|r| r.to.as_str()).collect();
    assert!(names.contains(&"Heron-0001.png"), "{names:?}");
    assert!(names.contains(&"Heron-0002.png"), "{names:?}");
}

/// ADR 0100 §2: the one operation that could destroy a photograph refuses
/// to, and says so per asset rather than aborting the batch.
#[test]
fn renaming_onto_an_existing_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let library = library_with(dir.path(), &["a.png", "b.png"]);
    let assets: Vec<_> = library
        .catalog()
        .grid(&leyline_catalog::GridQuery::default())
        .unwrap()
        .iter()
        .map(|item| item.asset_id)
        .collect();

    // Both would become "same.png": the first succeeds, the second is
    // refused, and the file it would have overwritten still holds its own
    // pixels.
    let report = library.rename(&assets, "same").unwrap();
    assert_eq!(report.renamed.len(), 1, "{report:?}");
    assert_eq!(report.failed.len(), 1, "{report:?}");
    assert!(
        report.failed[0].reason.contains("already exists"),
        "{:?}",
        report.failed[0]
    );
    // The refused asset kept its name, and its file.
    let kept = library
        .catalog()
        .asset_details(report.failed[0].asset)
        .unwrap();
    assert!(
        kept.filename == "a.png" || kept.filename == "b.png",
        "{kept:?}"
    );
    assert!(library.locate(report.failed[0].asset).unwrap().is_file());
}

/// ADR 0100 §1: a bad template refuses the whole batch's worth of names
/// rather than producing them.
#[test]
fn a_bad_template_renames_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library_with(dir.path(), &["a.png"]);
    let assets: Vec<_> = library
        .catalog()
        .grid(&leyline_catalog::GridQuery::default())
        .unwrap()
        .iter()
        .map(|item| item.asset_id)
        .collect();

    for template in ["{sequence}", "2026/{name}", "{name"] {
        let report = library.rename(&assets, template).unwrap();
        assert!(report.renamed.is_empty(), "{template}: {report:?}");
        assert_eq!(report.failed.len(), 1, "{template}: {report:?}");
    }
    // The file still carries the name it came in with.
    assert_eq!(
        library.catalog().asset_details(assets[0]).unwrap().filename,
        "a.png"
    );
}

/// ADR 0100 §3: a sidecar follows the photograph it describes.
#[test]
fn a_sidecar_follows_its_photograph() {
    let dir = tempfile::tempdir().unwrap();
    let library = library_with(dir.path(), &["a.png"]);
    let asset = library
        .catalog()
        .grid(&leyline_catalog::GridQuery::default())
        .unwrap()[0]
        .asset_id;

    // Write one, so there is something to follow.
    let sidecar = library.write_xmp(asset).unwrap();
    assert!(sidecar.is_file());

    let report = library.rename(&[asset], "renamed").unwrap();
    assert_eq!(report.renamed.len(), 1, "{report:?}");
    assert!(!sidecar.exists(), "the old sidecar must not stay behind");
    let moved = library.locate(asset).unwrap().with_extension("xmp");
    assert!(moved.is_file(), "the sidecar must be beside the new name");
}

/// A name the file already has is not a rename, and not a failure either.
#[test]
fn renaming_to_the_current_name_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = library_with(dir.path(), &["a.png"]);
    let asset = library
        .catalog()
        .grid(&leyline_catalog::GridQuery::default())
        .unwrap()[0]
        .asset_id;

    let report = library.rename(&[asset], "{name}").unwrap();
    assert!(report.renamed.is_empty(), "{report:?}");
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(
        library.catalog().asset_details(asset).unwrap().filename,
        "a.png"
    );
}
