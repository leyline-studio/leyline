//! Shared types, identifiers and errors for the Leyline platform.
//!
//! `leyline-core` is the innermost crate of the workspace: every other crate
//! depends on it, and it depends on nothing but serde. Its types — identifiers,
//! [`LeylineError`], [`Settings`] — are part of the stable SDK contract
//! (`docs/engine-api.md` §13).

mod error;
mod id;
mod settings;
mod types;

pub use error::{LeylineError, Result};
pub use id::{
    AssetId, CollectionId, ExportPresetId, FolderId, JobId, KeywordId, PresetId, PrintPresetId,
    RevisionId, VersionId,
};
pub use settings::{
    BrushStroke, CURRENT_PROCESS, CURRENT_SCHEMA, ColorGrading, ColorGradingZone, Crop, CurvePoint,
    HslBand, LensCorrection, LocalAdjustment, LocalAdjustmentValues, Mask, NoiseReduction, Point,
    PresetSettings, Settings, SettingsGroup, Sharpening, SpotRemoval, ToneCurve, WhiteBalance,
};
pub use types::{CollectionType, ColorLabel, MediaType, PickState, PreviewKind};
