//! Integration tests: print orchestration (ADR 0036).

use leyline_core::{LeylineError, VersionId};
use leyline_engine::{ImportOptions, Library, PrintRecipe, PrintRequest};
use leyline_export::PrintSettings;

fn library(dir: &tempfile::TempDir) -> Library {
    Library::create(&dir.path().join("Library"), "Print").unwrap()
}

fn import_a_real_png(dir: &tempfile::TempDir, library: &Library) -> leyline_core::VersionId {
    let source = dir.path().join("photo.png");
    image::save_buffer(
        &source,
        &[128u8; 8 * 4 * 3],
        8,
        4,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    report.imported[0].registered.version
}

#[test]
fn prints_a_real_image_as_a_single_page_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let version = import_a_real_png(&dir, &library);
    let out = dir.path().join("out");

    let request = PrintRequest {
        versions: vec![version],
        recipe: PrintRecipe::Adhoc(PrintSettings::default()),
        destination_dir: out.clone(),
        copies: 1,
    };
    let mut ticks = Vec::new();
    let report = library
        .print(&request, |done, total| ticks.push((done, total)))
        .unwrap();

    assert_eq!(report.failed, vec![]);
    assert_eq!(report.printed.len(), 1);
    let path = &report.printed[0].path;
    assert!(path.is_file());
    assert_eq!(path.extension().unwrap(), "pdf");
    assert_eq!(std::fs::read(path).unwrap()[..5], *b"%PDF-");
    assert_eq!(ticks, vec![(1, 1)]);
}

#[test]
fn a_missing_version_is_reported_without_stopping_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let version = import_a_real_png(&dir, &library);
    let out = dir.path().join("out");

    let request = PrintRequest {
        versions: vec![version, VersionId::new(999)],
        recipe: PrintRecipe::Adhoc(PrintSettings::default()),
        destination_dir: out,
        copies: 1,
    };
    let report = library.print(&request, |_, _| {}).unwrap();

    assert_eq!(report.printed.len(), 1);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(
        report.failed[0].reason,
        LeylineError::VersionMissing(VersionId::new(999)).to_string()
    );
}

#[test]
fn preset_recipe_is_resolved_at_print_time() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let version = import_a_real_png(&dir, &library);
    let out = dir.path().join("out");

    let preset = library
        .create_print_preset("Postcard", &PrintSettings::default())
        .unwrap();
    let presets = library.print_presets().unwrap();
    assert_eq!(presets.len(), 1);
    assert_eq!(presets[0].preset, preset);

    let request = PrintRequest {
        versions: vec![version],
        recipe: PrintRecipe::Preset(preset),
        destination_dir: out,
        copies: 2,
    };
    let report = library.print(&request, |_, _| {}).unwrap();
    assert_eq!(report.printed.len(), 1);
}

#[test]
fn printing_never_overwrites_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let version = import_a_real_png(&dir, &library);
    let out = dir.path().join("out");

    let request = PrintRequest {
        versions: vec![version],
        recipe: PrintRecipe::Adhoc(PrintSettings::default()),
        destination_dir: out,
        copies: 1,
    };
    library.print(&request, |_, _| {}).unwrap();
    let second = library.print(&request, |_, _| {}).unwrap();
    assert_eq!(second.printed, vec![]);
    assert_eq!(second.failed.len(), 1);
}
