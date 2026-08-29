//! Stable public API of the Leyline engine (`docs/engine-api.md` §13).
//!
//! Everything Studio can do, the CLI and any script can do — because they
//! all call exactly this surface. `leyline-engine` remains internal and may
//! change at every version; this crate is the semver contract (`0.x` until
//! V1). The core types (ids, errors, [`Settings`]) are part of it.

pub use leyline_core::{
    AssetId, BrushStroke, CURRENT_SCHEMA, CameraProfile, CollectionId, CollectionType,
    ColorGrading, ColorGradingZone, ColorLabel, ColorRange, Crop, CurvePoint, Demosaic,
    ExportPresetId, FolderId, HighlightReconstruction, HslBand, JobId, KeywordId, LensCorrection,
    LeylineError, LocalAdjustment, LocalAdjustmentValues, LuminanceRange, Lut, Mask, MediaType,
    NoiseReduction, Perspective, PickState, Point, PresetFolderId, PresetId, PresetSettings,
    PreviewKind, PrintPresetId, RangeMask, Result, RevisionId, Settings, SettingsGroup, Sharpening,
    SpotRemoval, StageVersions, ToneCurve, VersionId, WhiteBalance,
};

pub use leyline_engine::{
    CatalogRead, CatalogWrite, DEFAULT_AMEND_WINDOW, EditSession, Event, ExportRecipe,
    ExportReport, ExportRequest, ExportedVersion, FailedApply, FailedExport, FailedPrint,
    FailedReprocess, ImportCandidate, ImportOptions, ImportReport, ImportedCameraProfile,
    ImportedFile, ImportedLut, JobResult, Library, Param, PresetApplyReport, Preview, PreviewFile,
    PrintRecipe, PrintReport, PrintRequest, PrintedVersion, RemovalReport, ReprocessReport, Rgb8,
    RootStatus, ScanOptions, SkippedFile, SoftProof, SourceColor, Value, WatchError,
    WatchSessionEvent, WatchedFile, decoder_version, neutral_settings,
};

pub use leyline_catalog::{
    CameraInfo, CollectionNode, ExportPreset, ExportRecord, FolderNode, GridItem, GridQuery,
    KeywordNode, LIBRARY_ROOT, LensInfo, LibraryInfo, MapPin, Metadata, Preset, PresetFolder,
    PrintPreset, RatingRule, Rational, RegisteredAsset, RevisionRow, Root, ShotFacets, ShotRange,
    SmartRules, Sort, VersionInfo,
};

pub use leyline_color::RenderingIntent;
pub use leyline_export::{
    DEFAULT_AVIF_SPEED, ExportFormat, ExportSettings, Margins, Orientation, PaperSize,
    PrintSettings, Watermark, WatermarkAnchor, WatermarkFont,
};
pub use leyline_map::TilePackInfo;

/// External mask detectors (ADR 0073). Not part of the engine: a detector is
/// a separate process turning a preview into a coverage image, and the
/// caller turns that image into a mask with `Library::store_mask_coverage`.
/// Re-exported here because Studio declares one Leyline dependency and one
/// only (`docs/architecture.md` §À l'intérieur de Studio).
pub use leyline_detect::{
    DetectError, Detection, DetectorSource, detect, discover, discover_in, manifests_dir,
};
