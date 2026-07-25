//! Stable public API of the Leyline engine (`docs/engine-api.md` §13).
//!
//! Everything Studio can do, the CLI and any script can do — because they
//! all call exactly this surface. `leyline-engine` remains internal and may
//! change at every version; this crate is the semver contract (`0.x` until
//! V1). The core types (ids, errors, [`Settings`]) are part of it.

pub use leyline_core::{
    AssetId, CURRENT_PROCESS, CURRENT_SCHEMA, CameraProfile, CollectionId, CollectionType,
    ColorGrading, ColorGradingZone, ColorLabel, Crop, CurvePoint, ExportPresetId, FolderId,
    HslBand, JobId, KeywordId, LensCorrection, LeylineError, MediaType, NoiseReduction, PickState,
    Point, PresetId, PresetSettings, PreviewKind, PrintPresetId, Result, RevisionId, Settings,
    SettingsGroup, Sharpening, SpotRemoval, ToneCurve, VersionId, WhiteBalance,
};

pub use leyline_engine::{
    CatalogRead, CatalogWrite, DEFAULT_AMEND_WINDOW, EditSession, Event, ExportRecipe,
    ExportReport, ExportRequest, ExportedVersion, FailedApply, FailedExport, FailedPrint,
    FailedReprocess, ImportOptions, ImportReport, ImportedCameraProfile, ImportedFile, JobResult,
    Library, Param, PresetApplyReport, PreviewFile, PrintRecipe, PrintReport, PrintRequest,
    PrintedVersion, ReprocessReport, Rgb8, SkippedFile, Value,
};

pub use leyline_catalog::{
    CameraInfo, CollectionNode, ExportPreset, ExportRecord, GridItem, GridQuery, KeywordNode,
    LensInfo, LibraryInfo, MapPin, Metadata, Preset, PrintPreset, RatingRule, Rational,
    RevisionRow, SmartRules, Sort, VersionInfo,
};

pub use leyline_color::RenderingIntent;
pub use leyline_export::{
    ExportFormat, ExportSettings, Margins, Orientation, PaperSize, PrintSettings,
};
pub use leyline_map::TilePackInfo;
