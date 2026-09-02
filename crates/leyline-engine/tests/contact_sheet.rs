//! Integration tests: contact sheets (ADR 0110).

use leyline_core::{LeylineError, VersionId};
use leyline_engine::{ContactSheetRecipe, ContactSheetRequest, ImportOptions, Library};
use leyline_export::{CaptionSource, ContactSheetSettings, PaperSize, PrintSettings};

fn library(dir: &tempfile::TempDir) -> Library {
    Library::create(&dir.path().join("Library"), "Sheets").unwrap()
}

/// Imports `count` real photographs, each a different flat colour so a cell
/// can be told from its neighbour.
fn import_photos(dir: &tempfile::TempDir, library: &Library, count: usize) -> Vec<VersionId> {
    let mut versions = Vec::new();
    for index in 0..count {
        let source = dir.path().join(format!("photo{index}.png"));
        let value = 40 + (index as u8) * 20;
        image::save_buffer(
            &source,
            &vec![value; 16 * 12 * 3],
            16,
            12,
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
                    thumbnails: false,
                },
                |_, _| {},
            )
            .unwrap();
        versions.push(report.imported[0].registered.version);
    }
    versions
}

/// A small sheet: two cells per page, on a postcard rather than an A4, so a
/// test renders a few thousand pixels instead of eight million.
fn small_sheet() -> ContactSheetSettings {
    ContactSheetSettings {
        page: PrintSettings {
            paper: PaperSize::Custom {
                width_mm: 100.0,
                height_mm: 60.0,
            },
            dpi: 72,
            ..PrintSettings::default()
        },
        columns: 2,
        rows: 1,
        ..ContactSheetSettings::default()
    }
}

#[test]
fn lays_several_photographs_out_on_one_multi_page_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let versions = import_photos(&dir, &library, 3);
    let destination = dir.path().join("sheets").join("index.pdf");

    let mut ticks = Vec::new();
    let report = library
        .contact_sheet(
            &ContactSheetRequest {
                versions: versions.clone(),
                recipe: ContactSheetRecipe::Adhoc(small_sheet()),
                destination: destination.clone(),
            },
            |done, total| ticks.push((done, total)),
        )
        .unwrap();

    assert_eq!(report.failed, vec![]);
    assert_eq!(report.placed, 3);
    // Two cells per page: three photographs need two pages, the second one
    // half empty.
    assert_eq!(report.pages, 2);
    assert_eq!(report.path, destination);
    assert!(destination.is_file());
    assert_eq!(std::fs::read(&destination).unwrap()[..5], *b"%PDF-");
    assert_eq!(ticks, vec![(1, 3), (2, 3), (3, 3)]);
}

#[test]
fn a_photograph_that_fails_leaves_an_empty_cell_and_the_sheet_is_still_written() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let mut versions = import_photos(&dir, &library, 2);
    versions.insert(1, VersionId::new(999));
    let destination = dir.path().join("index.pdf");

    let report = library
        .contact_sheet(
            &ContactSheetRequest {
                versions,
                recipe: ContactSheetRecipe::Adhoc(small_sheet()),
                destination: destination.clone(),
            },
            |_, _| {},
        )
        .unwrap();

    assert_eq!(report.placed, 2);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(
        report.failed[0].reason,
        LeylineError::VersionMissing(VersionId::new(999)).to_string()
    );
    // The failure took the second cell with it rather than shifting the
    // third photograph up: three cells, two pages of two.
    assert_eq!(report.pages, 2);
    assert!(destination.is_file());
}

#[test]
fn an_existing_destination_is_refused_before_anything_is_rendered() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let versions = import_photos(&dir, &library, 1);
    let destination = dir.path().join("index.pdf");
    std::fs::write(&destination, b"not a sheet").unwrap();

    let error = library
        .contact_sheet(
            &ContactSheetRequest {
                versions,
                recipe: ContactSheetRecipe::Adhoc(small_sheet()),
                destination: destination.clone(),
            },
            |_, _| {},
        )
        .unwrap_err();

    assert!(matches!(error, LeylineError::Io(_)), "{error}");
    assert_eq!(std::fs::read(&destination).unwrap(), b"not a sheet");
}

#[test]
fn an_empty_request_is_refused_rather_than_writing_a_blank_page() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let destination = dir.path().join("index.pdf");

    let error = library
        .contact_sheet(
            &ContactSheetRequest {
                versions: vec![],
                recipe: ContactSheetRecipe::Adhoc(small_sheet()),
                destination: destination.clone(),
            },
            |_, _| {},
        )
        .unwrap_err();

    assert!(matches!(error, LeylineError::InvalidSettings(_)), "{error}");
    assert!(!destination.exists());
}

#[test]
fn a_preset_recipe_is_resolved_when_the_sheet_runs() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let versions = import_photos(&dir, &library, 1);

    let settings = ContactSheetSettings {
        caption: CaptionSource::None,
        ..small_sheet()
    };
    let preset = library
        .create_contact_sheet_preset("Index", &settings)
        .unwrap();
    let stored = library.contact_sheet_presets().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].preset, preset);
    assert_eq!(stored[0].name, "Index");

    let report = library
        .contact_sheet(
            &ContactSheetRequest {
                versions,
                recipe: ContactSheetRecipe::Preset(preset),
                destination: dir.path().join("index.pdf"),
            },
            |_, _| {},
        )
        .unwrap();
    assert_eq!(report.placed, 1);
    assert_eq!(report.pages, 1);
}

#[test]
fn a_recipe_that_does_not_fit_its_page_is_refused_before_the_first_decode() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let versions = import_photos(&dir, &library, 1);
    let destination = dir.path().join("index.pdf");

    let error = library
        .contact_sheet(
            &ContactSheetRequest {
                versions,
                recipe: ContactSheetRecipe::Adhoc(ContactSheetSettings {
                    gutter_mm: 500.0,
                    ..small_sheet()
                }),
                destination: destination.clone(),
            },
            |_, _| {},
        )
        .unwrap_err();

    assert!(matches!(error, LeylineError::InvalidSettings(_)), "{error}");
    assert!(!destination.exists());
}
