//! Stable public API of the Leyline engine (`docs/engine-api.md` §13).
//!
//! Everything Studio can do, the CLI and any script can do — because they
//! all call exactly this surface. `leyline-engine` remains internal and may
//! change at every version; this crate is the semver contract (`0.x` until
//! V1). The core types (ids, errors, [`Settings`]) are part of it.

pub use leyline_core::{
    AssetId, CURRENT_PROCESS, CURRENT_SCHEMA, CollectionId, CollectionType, ColorLabel, Crop,
    ExportPresetId, FolderId, JobId, KeywordId, LensCorrection, LeylineError, MediaType,
    NoiseReduction, PickState, PreviewKind, Result, RevisionId, Settings, Sharpening, VersionId,
    WhiteBalance,
};

pub use leyline_engine::{
    CatalogRead, CatalogWrite, DEFAULT_AMEND_WINDOW, EditSession, Event, ExportReport,
    ExportedVersion, FailedExport, ImportOptions, ImportReport, ImportedFile, JobResult, Library,
    Param, PreviewFile, SkippedFile, Value,
};

pub use leyline_catalog::{
    CameraInfo, CollectionNode, ExportPreset, ExportRecord, GridItem, GridQuery, KeywordNode,
    LensInfo, LibraryInfo, Metadata, RatingRule, Rational, RevisionRow, SmartRules, Sort,
    VersionInfo,
};

pub use leyline_export::{ExportFormat, ExportSettings};
