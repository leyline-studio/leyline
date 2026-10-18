//! Shared types, identifiers and errors for the Leyline platform.
//!
//! `leyline-core` is the innermost crate of the workspace: every other crate
//! depends on it, and it depends on nothing but serde. Its types — identifiers,
//! [`LeylineError`], [`Settings`] — are part of the stable SDK contract
//! (`docs/engine-api.md` §13).

// ADR 0152: no panic on a production path.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![forbid(unsafe_code)]

mod error;
mod id;
mod origin;
mod path;
mod settings;
mod tether;
mod types;

pub use error::{LeylineError, Result};
pub use id::{
    AssetId, CollectionId, ContactSheetPresetId, ExportPresetId, FolderId, JobId, KeywordId,
    PresetFolderId, PresetId, PrintPresetId, RevisionId, VersionId,
};
pub use origin::{MCP_CLIENT_NAME_MAX, RevisionOrigin};
pub use path::validate_library_relative_path;
pub use settings::{
    BrushStroke, CURRENT_SCHEMA, CameraProfile, ColorGrading, ColorGradingZone, ColorRange, Crop,
    CurvePoint, Defringe, Demosaic, Grain, HighlightReconstruction, HslBand, LensCorrection,
    Levels, LocalAdjustment, LocalAdjustmentValues, LuminanceRange, Lut, Mask, NoiseReduction,
    OutputRendering, ParametricCurve, Perspective, Point, PresetSettings, RangeMask, RedEye,
    ReshapePoint, SETTINGS_KEYS, Settings, SettingsGroup, Sharpening, SourceEncoding, SpotRemoval,
    StageVersions, ToneCurve, Vignette, WHITE_BALANCE_PRESETS, WhiteBalance, WhiteBalancePreset,
};
pub use tether::{CameraSetting, CameraSettings, TetherSetting};
pub use types::{CollectionType, ColorLabel, MediaType, PickState, PreviewKind, PreviewOrigin};
