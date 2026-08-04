//! Integration tests: stored mask coverages (ADR 0070), end to end through
//! the façade — stored, referenced by a revision, and actually rendered.

use leyline_core::{LeylineError, LocalAdjustment, LocalAdjustmentValues, Mask, StageVersions};
use leyline_engine::{ImportOptions, Library, Param, Value};

/// A library with one flat mid-grey PNG imported, plus its version id.
fn library_with_a_flat_photo(
    dir: &std::path::Path,
) -> (Library, leyline_core::AssetId, leyline_core::VersionId) {
    let library = Library::create(&dir.join("Lib"), "Coverage").unwrap();
    let source = dir.join("Shoot");
    std::fs::create_dir(&source).unwrap();
    image::save_buffer(
        source.join("flat.png"),
        &[128u8; 32 * 32 * 3],
        32,
        32,
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
    let registered = report.imported[0].registered;
    (library, registered.asset, registered.version)
}

/// A local adjustment that brightens hard, so coverage is legible in the
/// output as "did this pixel move at all".
fn brighten(mask: Mask) -> LocalAdjustment {
    LocalAdjustment {
        mask,
        range: None,
        opacity: 1.0,
        adjustments: LocalAdjustmentValues {
            exposure: Some(2.0),
            ..LocalAdjustmentValues::default()
        },
    }
}

/// The whole point: a coverage stored as a file drives the render, and it
/// drives it *where the file says* — left half covered, right half not.
#[test]
fn a_stored_coverage_masks_the_render_where_its_samples_say() {
    let dir = tempfile::tempdir().unwrap();
    let (library, asset, version) = library_with_a_flat_photo(dir.path());

    // 8x8, fully covered on the left half, not at all on the right.
    let mut samples = vec![0u16; 64];
    for row in 0..8 {
        for column in 0..4 {
            samples[row * 8 + column] = u16::MAX;
        }
    }
    let mask = library.store_mask_coverage(8, 8, &samples).unwrap();
    assert!(matches!(mask, Mask::Coverage { .. }));

    // Scoped: an edit session holds the catalog write guard, so it must be
    // dropped before anything that renders takes the lock again.
    {
        let mut session = library.edit(version).unwrap();
        session
            .set(
                Param::LocalAdjustment(0),
                Value::LocalAdjustment(Some(brighten(mask.clone()))),
            )
            .unwrap();
        session.commit().unwrap();
    }

    let preview = library
        .preview(asset, leyline_core::PreviewKind::Medium)
        .unwrap();
    let decoded = image::open(&preview.path).unwrap().to_rgb8();
    let (width, height) = (decoded.width() as usize, decoded.height() as usize);
    let at = |x: usize, y: usize| decoded.get_pixel(x as u32, y as u32).0[0];
    let (left, right) = (at(width / 8, height / 2), at(width * 7 / 8, height / 2));
    assert!(
        left > right + 20,
        "the covered half must be brighter: left {left}, right {right}"
    );

    // And a neutral revision of the same photo is darker than the covered
    // half everywhere — the adjustment really is local, not global.
    {
        let mut session = library.edit(version).unwrap();
        session
            .set(Param::LocalAdjustment(0), Value::LocalAdjustment(None))
            .unwrap();
        session.commit().unwrap();
    }
    let neutral = library
        .preview(asset, leyline_core::PreviewKind::Medium)
        .unwrap();
    let neutral_decoded = image::open(&neutral.path).unwrap().to_rgb8();
    let neutral_at = neutral_decoded
        .get_pixel((width * 7 / 8) as u32, (height / 2) as u32)
        .0[0];
    assert_eq!(
        neutral_at, right,
        "the uncovered half must match the neutral render exactly"
    );
}

/// The capability rule (ADR 0070 §4): a revision pinned on a version that
/// cannot express a stored coverage is refused by name, never rendered with
/// the mask quietly dropped — which would apply the adjustment to the whole
/// photo.
#[test]
fn a_revision_pinned_before_v3_refuses_a_stored_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Lib"), "Capability").unwrap();
    let mask = library.store_mask_coverage(2, 2, &[0, 0, 0, 0]).unwrap();

    for pinned in [1, 2] {
        let settings = leyline_core::Settings {
            stages: StageVersions::from([("local_adjustments".to_owned(), pinned)]),
            local_adjustments: vec![brighten(mask.clone())],
            ..leyline_core::Settings::default()
        };
        let error = settings.validate().unwrap_err();
        assert!(
            matches!(&error, LeylineError::InvalidSettings(m)
                if m.contains("version 3") && m.contains("reprocess")),
            "pinned at v{pinned}, got {error}"
        );
    }

    // Version 3 accepts it.
    let settings = leyline_core::Settings {
        stages: StageVersions::from([("local_adjustments".to_owned(), 3)]),
        local_adjustments: vec![brighten(mask)],
        ..leyline_core::Settings::default()
    };
    settings.validate().unwrap();
}

/// The fail-closed contract, through the façade: a coverage file deleted
/// after the revision recorded it stops the render with a named error rather
/// than rendering the adjustment over the whole photo.
#[test]
fn a_missing_coverage_file_fails_the_render_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let (library, asset, version) = library_with_a_flat_photo(dir.path());
    let mask = library
        .store_mask_coverage(4, 4, &[u16::MAX; 16])
        .unwrap();
    let Mask::Coverage { path, .. } = &mask else {
        unreachable!()
    };

    {
        let mut session = library.edit(version).unwrap();
        session
            .set(
                Param::LocalAdjustment(0),
                Value::LocalAdjustment(Some(brighten(mask.clone()))),
            )
            .unwrap();
        session.commit().unwrap();
    }
    library
        .preview(asset, leyline_core::PreviewKind::Medium)
        .expect("renders while the file is there");

    std::fs::remove_file(
        dir.path()
            .join("Lib")
            .join(path.replace('/', std::path::MAIN_SEPARATOR_STR)),
    )
    .unwrap();
    // The preview cache would happily serve the previous render, so ask for a
    // size class that has not been rendered yet.
    let error = library
        .preview(asset, leyline_core::PreviewKind::Large)
        .unwrap_err();
    assert!(
        matches!(&error, LeylineError::MaskCoverageFailed { path: p, .. } if p == path),
        "got {error}"
    );
}

/// Serialization round trip: a stored mask survives `settings_json`, which is
/// what makes it a *setting* rather than a rendering detail (ADR 0069 §1).
#[test]
fn a_stored_coverage_survives_the_settings_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Lib"), "RoundTrip").unwrap();
    let mask = library.store_mask_coverage(2, 2, &[1, 2, 3, 4]).unwrap();
    let settings = leyline_core::Settings {
        stages: StageVersions::from([("local_adjustments".to_owned(), 3)]),
        local_adjustments: vec![brighten(mask)],
        ..leyline_core::Settings::default()
    };
    let json = settings.to_json();
    assert!(json.contains("\"type\":\"coverage\""), "{json}");
    assert_eq!(leyline_core::Settings::parse(&json).unwrap(), settings);
}
