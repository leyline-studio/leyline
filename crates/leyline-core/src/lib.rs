//! Shared types, identifiers and errors for the Leyline platform.
//!
//! `leyline-core` is the innermost crate of the workspace: every other crate
//! depends on it, and it depends on nothing but serde. Its types — identifiers,
//! [`LeylineError`], [`Settings`] — are part of the stable SDK contract
//! (`docs/engine-api.md` §13).

mod error;
mod id;
mod path;
mod settings;
mod types;

pub use error::{LeylineError, Result};
pub use id::{
    AssetId, CollectionId, ExportPresetId, FolderId, JobId, KeywordId, PresetId, PrintPresetId,
    RevisionId, VersionId,
};
pub use path::validate_library_relative_path;
pub use settings::{
    BrushStroke, CURRENT_SCHEMA, CameraProfile, ColorGrading, ColorGradingZone, ColorRange, Crop,
    CurvePoint, HighlightReconstruction, HslBand, LensCorrection, LocalAdjustment,
    LocalAdjustmentValues, LuminanceRange, Lut, Mask, NoiseReduction, Perspective, Point,
    PresetSettings, RangeMask, Settings, SettingsGroup, Sharpening, SpotRemoval, StageVersions,
    ToneCurve, WhiteBalance,
};
pub use types::{CollectionType, ColorLabel, MediaType, PickState, PreviewKind};
