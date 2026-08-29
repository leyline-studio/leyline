//! The façade's own guard.
//!
//! `leyline-sdk` re-exports and nothing else, so its only way of being wrong
//! is a *hole*: an engine method whose return type — or a `Settings` field's
//! type — cannot be named through `leyline_sdk::`. A caller hitting one has
//! to reach past the façade into `leyline_engine`, which is exactly what
//! `docs/engine-api.md` §13 says the SDK exists to prevent, and nothing in
//! the workspace notices because Studio and the CLI depend on the engine
//! directly.
//!
//! Every item below is therefore referenced *only* through `leyline_sdk::`.
//! The test is a compile-time assertion first and a runtime one second: if a
//! re-export goes missing, this file stops building.

use std::path::PathBuf;

use leyline_sdk::{
    AssetId, BrushStroke, CameraProfile, ColorGrading, ColorGradingZone, ColorLabel, ColorRange,
    Crop, CurvePoint, EditSession, Event, ExportFormat, ExportRecipe, ExportRequest,
    ExportSettings, GridItem, GridQuery, HslBand, ImportOptions, ImportReport, ImportedFile, JobId,
    LensCorrection, LeylineError, Library, LocalAdjustment, LocalAdjustmentValues, LuminanceRange,
    Margins, Mask, NoiseReduction, Orientation, PaperSize, PickState, Point, Preview, PreviewKind,
    PrintRecipe, PrintRequest, PrintSettings, RangeMask, RegisteredAsset, RenderingIntent,
    RevisionId, Settings, Sharpening, ShotFacets, ShotRange, SkippedFile, SpotRemoval,
    StageVersions, ToneCurve, VersionId, WatchSessionEvent, WatchedFile, Watermark,
    WatermarkAnchor, WatermarkFont,
};

/// A `Settings` built field by field, every nested type named through the
/// façade. This is the one that actually caught something: local adjustments
/// (ADR 0029) shipped without [`Mask`] or [`LocalAdjustment`] reaching the
/// SDK, so no external caller could construct the masks the engine renders.
fn fully_specified_settings() -> Settings {
    let values = LocalAdjustmentValues {
        exposure: Some(0.4),
        ..LocalAdjustmentValues::default()
    };
    Settings {
        stages: StageVersions::from([("gains".to_owned(), 1), ("crop".to_owned(), 1)]),
        exposure: 0.3,
        crop: Some(Crop {
            x: 0.05,
            y: 0.05,
            width: 0.9,
            height: 0.9,
        }),
        lens_correction: LensCorrection {
            enabled: true,
            profile: "auto".to_owned(),
        },
        noise_reduction: NoiseReduction {
            luminance: 20,
            color: 15,
        },
        sharpening: Sharpening {
            amount: 40,
            radius: 1.0,
        },
        tone_curve: ToneCurve {
            points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }],
        },
        spot_removal: vec![SpotRemoval {
            target: Point { x: 0.3, y: 0.3 },
            source: Point { x: 0.6, y: 0.6 },
            radius: 0.05,
            feather: 0.5,
            opacity: 1.0,
        }],
        local_adjustments: vec![
            LocalAdjustment {
                mask: Mask::Radial {
                    cx: 0.5,
                    cy: 0.5,
                    rx: 0.3,
                    ry: 0.3,
                    angle: 0.0,
                    feather: 0.5,
                    inverted: false,
                },
                range: None,
                opacity: 1.0,
                adjustments: values,
            },
            LocalAdjustment {
                mask: Mask::Gradient {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 0.0,
                    y1: 0.5,
                },
                range: None,
                opacity: 0.8,
                adjustments: values,
            },
            // A range mask (ADR 0048) through the façade, on the geometry
            // that has none of its own.
            LocalAdjustment {
                mask: Mask::Everything,
                range: Some(RangeMask {
                    luminance: Some(LuminanceRange {
                        min: 0.2,
                        max: 0.8,
                        softness: 0.1,
                    }),
                    color: Some(ColorRange {
                        center: 210.0,
                        width: 40.0,
                        softness: 15.0,
                    }),
                }),
                opacity: 0.5,
                adjustments: values,
            },
            LocalAdjustment {
                mask: Mask::Brush {
                    strokes: vec![BrushStroke {
                        x: 0.4,
                        y: 0.4,
                        radius: 0.05,
                        flow: 0.7,
                        hardness: 0.5,
                    }],
                },
                range: None,
                opacity: 1.0,
                adjustments: values,
            },
        ],
        hsl: [HslBand {
            hue: 5,
            saturation: 10,
            luminance: 0,
        }; 8],
        color_grading: ColorGrading {
            shadows: ColorGradingZone {
                hue: 220,
                saturation: 10,
                luminance: 0,
            },
            ..ColorGrading::default()
        },
        camera_profile: Some(CameraProfile {
            enabled: true,
            path: "Profiles/Camera/sample.dcp".to_owned(),
            checksum: "blake3:00".to_owned(),
        }),
        ..Settings::default()
    }
}

/// Binds, by name, the type of every value the engine hands back that a
/// caller has to store or match on. Never called: the compiler is the
/// assertion.
#[expect(dead_code, reason = "the signatures are the test")]
fn every_returned_type_is_nameable(library: &Library, version: VersionId, asset: AssetId) {
    let _: Preview = library.preview_state(asset, PreviewKind::Medium).unwrap();
    let _: PathBuf = library.write_xmp(asset).unwrap();
    // Reading is not the mirror of writing: it reports whether the sidecar
    // filled anything, not a path (ADR 0047 §3).
    let _: bool = library.read_xmp(asset).unwrap();
    let _: JobId = library.preview_async(asset, PreviewKind::Medium);
    let _: EditSession<_> = library.edit(version).unwrap();
    let _: [[u32; 256]; 3] = library.histogram(asset, PreviewKind::Medium).unwrap();
    let _: Result<(), LeylineError> = library.set_pick(&[version], PickState::Pick);
    let _: Result<(), LeylineError> = library.set_color_label(&[version], Some(ColorLabel::Yellow));
}

/// An import report, destructured down to the ids it carries. Reaching
/// `registered` used to require naming `leyline_catalog::RegisteredAsset`,
/// which the façade did not re-export.
#[expect(dead_code, reason = "the destructuring is the test")]
fn an_import_report_is_fully_readable(report: &ImportReport) {
    for file in &report.imported {
        let _: &ImportedFile = file;
        let registered: &RegisteredAsset = &file.registered;
        let (_, _, _): (AssetId, VersionId, RevisionId) =
            (registered.asset, registered.version, registered.revision);
    }
    for skipped in &report.skipped {
        let _: &SkippedFile = skipped;
    }
}

/// The three `Preview` states and the two watch events, matched exhaustively
/// through the façade: a variant that stops being reachable from here is a
/// caller who cannot handle it.
#[expect(dead_code, reason = "the match arms are the test")]
fn every_enum_is_matchable(preview: Preview, watch: WatchSessionEvent) {
    match preview {
        Preview::Ready(_) | Preview::Stale { .. } | Preview::Generating(_) => {}
    }
    match watch {
        WatchSessionEvent::Ready(file) => {
            let _: WatchedFile = file;
        }
        WatchSessionEvent::Stopped(_) => {}
    }
}

/// The request structs a caller has to build, spelled out through the façade
/// including the `leyline-export` and `leyline-color` types they embed.
#[expect(dead_code, reason = "the constructors are the test")]
fn every_request_is_constructible(version: VersionId) -> (ExportRequest, PrintRequest) {
    (
        ExportRequest {
            versions: vec![version],
            recipe: ExportRecipe::Adhoc(ExportSettings {
                format: ExportFormat::Jpeg,
                quality: 90,
                avif_speed: leyline_sdk::DEFAULT_AVIF_SPEED,
                max_edge: Some(2048),
                watermark: Some(Watermark {
                    text: "© 2026".to_owned(),
                    font: WatermarkFont::Sans,
                    size: 3.0,
                    color: "#FFFFFF".to_owned(),
                    opacity: 0.7,
                    anchor: WatermarkAnchor::BottomRight,
                }),
            }),
            destination_dir: PathBuf::from("out"),
            concurrency: None,
        },
        PrintRequest {
            versions: vec![version],
            recipe: PrintRecipe::Adhoc(PrintSettings {
                paper: PaperSize::A4,
                orientation: Orientation::Portrait,
                margins_mm: Margins::default(),
                intent: RenderingIntent::Perceptual,
                ..PrintSettings::default()
            }),
            destination_dir: PathBuf::from("print"),
            copies: 1,
        },
    )
}

#[test]
fn a_library_round_trip_needs_nothing_but_the_sdk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");

    let library = Library::create(&root, "SDK surface").unwrap();
    let _: std::sync::mpsc::Receiver<Event> = library.subscribe();

    // An empty import: enough to prove the report types are reachable and
    // that no engine-only type leaks into the signature.
    let options = ImportOptions {
        copy_files: true,
        recursive: true,
        pair_companions: true,
        thumbnails: false,
    };
    let report = library.import(&root.join("nothing"), &options, |_, _| {});
    assert!(report.is_err() || report.unwrap().imported.is_empty());

    // Reaching the catalog through the façade's `CatalogRead` deref, with
    // the query type named from the SDK too.
    let grid: Vec<GridItem> = library.catalog().grid(&GridQuery::default()).unwrap();
    assert!(grid.is_empty());
    assert_eq!(library.catalog().count(&GridQuery::default()).unwrap(), 0);

    // The shot filters and the lists they are chosen from (ADR 0064), named
    // from the SDK: a client never has to reach into the catalog crate.
    let shot = GridQuery {
        camera: Some("EOS 60D".to_owned()),
        lens: Some("EF 50mm f/1.8 STM".to_owned()),
        iso: ShotRange::at_least(3200.0),
        aperture: ShotRange::at_most(2.8),
        focal_length: ShotRange::between(24.0, 70.0),
        shutter_speed: ShotRange::default(),
        ..GridQuery::default()
    };
    assert_eq!(library.catalog().count(&shot).unwrap(), 0);
    let facets: ShotFacets = library.catalog().shot_facets().unwrap();
    assert!(facets.cameras.is_empty() && facets.iso.is_none());
    assert!(library.collections().unwrap().is_empty());
    library.close().unwrap();

    // Reopening through the same surface: the façade covers the whole
    // lifecycle, not just creation.
    let reopened = Library::open(&root).unwrap();
    assert_eq!(reopened.root(), root);
    reopened.close().unwrap();
}

#[test]
fn settings_round_trip_through_the_sdk_surface() {
    let settings = fully_specified_settings();
    // One entry per mask kind, plus the range-refined one (ADR 0048).
    assert_eq!(settings.local_adjustments.len(), 4);
    assert!(settings.local_adjustments.iter().any(|a| a.range.is_some()));
    assert_eq!(settings.stages.get("gains"), Some(&1));

    // `Settings` is the reproducibility contract; the SDK must expose it in
    // a form that survives the same serialization the catalog stores.
    let json = serde_json::to_string(&settings).unwrap();
    let back: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(back, settings);
}

/// A detector, end to end through the façade alone (ADR 0073): discovered,
/// named, run, and its refusal read back. Studio declares one Leyline
/// dependency, so anything it needs here has to be nameable from `leyline_sdk`
/// — a hole in this re-export would be found in the interface, not in a test.
#[test]
fn mask_detectors_are_reachable_through_the_sdk_surface() {
    use leyline_sdk::{DetectError, Detection, DetectorSource, detect, discover_in};

    let dir = tempfile::tempdir().unwrap();
    // Nothing installed is the normal state, and it must be an empty list
    // rather than an error: no detector, no menu.
    assert!(discover_in(dir.path()).is_empty());

    let source = DetectorSource {
        id: "sample".to_owned(),
        label: "Sample".to_owned(),
        command: std::path::PathBuf::from("/no/such/detector"),
        args: Vec::new(),
        detections: vec![Detection {
            id: "sky".to_owned(),
            label: "Sky".to_owned(),
        }],
    };
    std::fs::write(
        dir.path().join("sample.json"),
        serde_json::to_string(&source).unwrap(),
    )
    .unwrap();
    // Declared but not installed: discovery drops it, so the menu never
    // offers a detection that cannot run.
    assert!(discover_in(dir.path()).is_empty());

    let error = detect(
        &source,
        "subject",
        std::path::Path::new("in.png"),
        std::path::Path::new("out.png"),
    )
    .unwrap_err();
    assert!(matches!(error, DetectError::UnknownDetection(_)));
}

/// The decoder version is reachable without reaching past the façade.
///
/// It is not a convenience: `docs/pipeline.md` §5.1 counts the decoder among
/// the terms of the bit-for-bit promise (ADR 0086), so a client that has to
/// report *what rendered this* — the CLI's `--version`, Studio's About — must
/// be able to ask the SDK. Both of them depend on `leyline-sdk` alone, so a
/// missing re-export here is a hole of exactly the kind this file exists to
/// catch.
#[test]
fn the_decoder_version_is_reachable_through_the_sdk_surface() {
    let version = leyline_sdk::decoder_version();
    assert!(
        version.starts_with(|c: char| c.is_ascii_digit()),
        "a LibRaw version starts with a digit, got {version:?}"
    );
}
