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
    RevisionId, Settings, Sharpening, SkippedFile, SpotRemoval, StageVersions, ToneCurve,
    VersionId, WatchSessionEvent, WatchedFile,
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
                max_edge: Some(2048),
            }),
            destination_dir: PathBuf::from("out"),
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
    };
    let report = library.import(&root.join("nothing"), &options, |_, _| {});
    assert!(report.is_err() || report.unwrap().imported.is_empty());

    // Reaching the catalog through the façade's `CatalogRead` deref, with
    // the query type named from the SDK too.
    let grid: Vec<GridItem> = library.catalog().grid(&GridQuery::default()).unwrap();
    assert!(grid.is_empty());
    assert_eq!(library.catalog().count(&GridQuery::default()).unwrap(), 0);
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
