//! Stable public API of the Leyline engine (`docs/engine-api.md` §13).
//!
//! Everything Studio can do, the CLI and any script can do — because they
//! all call exactly this surface. `leyline-engine` remains internal and may
//! change at every version; this crate is the semver contract (`0.x` until
//! V1). The core types (ids, errors, [`Settings`]) are part of it.

// ADR 0152: no panic on a production path, no `unsafe` outside raw and cli.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![forbid(unsafe_code)]

pub use leyline_core::{
    AssetId, BrushStroke, CURRENT_SCHEMA, CameraProfile, CameraSetting, CameraSettings,
    CollectionId, CollectionType, ColorGrading, ColorGradingZone, ColorLabel, ColorRange,
    ContactSheetPresetId, Crop, CurvePoint, Defringe, Demosaic, ExportPresetId, FolderId, Grain,
    HighlightReconstruction, HslBand, JobId, KeywordId, LensCorrection, LeylineError,
    LocalAdjustment, LocalAdjustmentValues, LuminanceRange, Lut, Mask, MediaType, NoiseReduction,
    ParametricCurve, Perspective, PickState, Point, PresetFolderId, PresetId, PresetSettings,
    PreviewKind, PrintPresetId, RangeMask, RedEye, ReshapePoint, Result, RevisionId, SETTINGS_KEYS,
    Settings, SettingsGroup, Sharpening, SpotRemoval, StageVersions, TetherSetting, ToneCurve,
    VersionId, Vignette, WHITE_BALANCE_PRESETS, WhiteBalance, WhiteBalancePreset,
};

pub use leyline_engine::{
    AutoTone, CatalogRead, CatalogWrite, ContactSheetRecipe, ContactSheetReport,
    ContactSheetRequest, CullEntry, CullOptions, CullProposal, DEFAULT_AMEND_WINDOW,
    DEFAULT_BURST_DISTANCE, DEFAULT_SESSION, EditSession, Event, ExportRecipe, ExportReport,
    ExportRequest, ExportVerdict, ExportedVersion, FailedApply, FailedExport, FailedPrint,
    FailedRename, FailedReprocess, Flow, GuideLine, ImportCandidate, ImportOptions, ImportReport,
    ImportedCameraProfile, ImportedFile, ImportedLut, JobResult, KEYSTONE_MIN_GUIDES,
    KeystoneSolution, Library, Param, PresetApplyReport, Preview, PreviewFile, PrintRecipe,
    PrintReport, PrintRequest, PrintedVersion, RangeSample, RejectReason, RemovalReport,
    RenameReport, RenamedAsset, ReprocessReport, Rgb8, RootStatus, ScanOptions, ScanReport,
    SkippedFile, SoftProof, Source, SourceColor, TCA_MIN_SAMPLES, TcaEstimate, TetherOptions,
    Value, Verdict, WatchError, WatchSessionEvent, WatchedFile, changed_settings, decoder_version,
    install_panic_hook, install_panic_hook_recording, neutral_settings, overlay,
    parametric_curve_samples, session_folder,
};

/// The measures assisted culling is built on (ADR 0084 §4) — focus,
/// clipping, and the burst fingerprint. Re-exported as a module because a
/// [`CullEntry`] carries a `Quality`, and a client that cannot name that
/// type cannot show what a verdict was based on.
///
/// Nothing here decides anything: `leyline-cull` computes numbers, the
/// engine turns them into a proposal, and the photographer turns the
/// proposal into keystrokes.
pub use leyline_cull as cull;

pub use leyline_catalog::{
    AssetDescription, CameraInfo, CollectionNode, ContactSheetPreset, ExportPreset, ExportRecord,
    FolderNode, GridItem, GridQuery, GridShape, JustifiedCell, JustifiedLayout, JustifiedRow,
    JustifyFrame, KeywordNode, LIBRARY_ROOT, LensInfo, LibraryInfo, MapPin, Metadata, Preset,
    PresetFolder, PrintPreset, RatingRule, Rational, RegisteredAsset, RevisionRow, Root,
    ShotFacets, ShotRange, SmartRules, Sort, VersionInfo, justify, step_vertical,
};

pub use leyline_color::RenderingIntent;
pub use leyline_export::{
    CaptionSource, ContactSheetSettings, DEFAULT_AVIF_SPEED, ExportFormat, ExportMetadata,
    ExportSettings, Margins, Orientation, PaperSize, PrintSettings, Watermark, WatermarkAnchor,
    WatermarkFont,
};
pub use leyline_map::TilePackInfo;

/// External mask detectors (ADR 0073). Not part of the engine: a detector is
/// a separate process turning a preview into a coverage image, and the
/// caller turns that image into a mask with `Library::store_mask_coverage`.
/// Re-exported here because Studio declares one Leyline dependency and one
/// only (`docs/architecture.md` §À l'intérieur de Studio).
pub use leyline_detect::{
    Conformance, DetectError, Detection, DetectorSource, Rejection, check_conformance,
    coverage_from_image, detect, detect_coverage, discover, discover_in, manifests_dir,
    rejected_in,
};

/// External pixel processors (ADR 0107). Not part of the engine either: a
/// processor is a separate process turning the develop buffer into another
/// image, and `Library::derive` files what comes back as a **new asset**,
/// never a stage.
///
/// A module rather than flat re-exports, and the reason is the sentence
/// above: the two sockets use the same words for the same ideas —
/// `discover`, `Rejection`, `check_conformance` — so flattening both would
/// force one of them to rename its own vocabulary in the façade.
/// `leyline_detect` is aliased beside it for symmetry; its flat re-exports
/// stay, because clients already use them.
pub use leyline_derive as derive;
pub use leyline_detect as detect;
