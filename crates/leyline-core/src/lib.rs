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
mod tether;
mod types;

pub use error::{LeylineError, Result};
pub use id::{
    AssetId, CollectionId, ContactSheetPresetId, ExportPresetId, FolderId, JobId, KeywordId,
    PresetFolderId, PresetId, PrintPresetId, RevisionId, VersionId,
};
pub use path::validate_library_relative_path;
pub use settings::{
    BrushStroke, CURRENT_SCHEMA, CameraProfile, ColorGrading, ColorGradingZone, ColorRange, Crop,
    CurvePoint, Demosaic, Grain, HighlightReconstruction, HslBand, LensCorrection, LocalAdjustment,
    LocalAdjustmentValues, LuminanceRange, Lut, Mask, NoiseReduction, Perspective, Point,
    PresetSettings, RangeMask, RedEye, Settings, SettingsGroup, Sharpening, SourceEncoding,
    SpotRemoval, StageVersions, ToneCurve, Vignette, WHITE_BALANCE_PRESETS, WhiteBalance,
    WhiteBalancePreset,
};
pub use tether::{CameraSetting, CameraSettings, TetherSetting};
pub use types::{CollectionType, ColorLabel, MediaType, PickState, PreviewKind, PreviewOrigin};
