//! Develop settings: the in-memory form of `settings_json` (`docs/pipeline.md` §3.2).
//!
//! A [`Settings`] value is always a complete, self-contained develop state —
//! never a delta. An omitted parameter takes its neutral value, frozen per
//! schema version: `{}` with `schema: 1` always produces the neutral rendering
//! of schema 1.
//!
//! Documents written by a newer schema are read losslessly: unknown fields are
//! preserved verbatim and re-serialized as-is, so an old engine never destroys
//! the work of a newer one (`docs/pipeline.md` §3.4).

use serde::{Deserialize, Serialize};

use crate::error::{LeylineError, Result};

// `Settings` stays here; the stage parameters it holds and the preset
// machinery built on it live beside it, re-exported so every path is unchanged.
mod params;
mod preset;

pub use params::*;
pub use preset::*;

/// Most recent settings format version this engine knows how to write.
pub const CURRENT_SCHEMA: u32 = 1;

/// The version of each pipeline stage a revision renders through
/// (`docs/pipeline.md` §3.3, ADR 0042 §2 and ADR 0043).
///
/// Keys are stage names as the engine registry spells them (`gains`,
/// `tone_curve`, `sharpen`, …); values are the version of that stage frozen
/// into this revision. A stage sitting at its neutral value does not run, has
/// therefore no behavior to pin, and **is absent** from the map — which makes
/// the map proportional to the actual edit rather than to the engine's stage
/// count.
///
/// The engine writes it when a revision is written, keeping any version
/// already recorded and adding the current version for a stage that just left
/// its neutral value. Nothing is ever inferred at read time.
pub type StageVersions = std::collections::BTreeMap<String, u16>;

/// Complete develop state of one revision (`docs/pipeline.md` §3.2, schema 1).
///
/// [`Settings::default`] is the neutral state of schema 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Version of the settings *format* (JSON structure).
    pub schema: u32,
    /// Version of each stage that renders this revision — the *rendering*
    /// axis, replacing the single `process` counter of ADR 0028 (ADR 0043).
    /// A stage at its neutral value has no entry, since it does not run.
    ///
    /// Two stages have no neutral value and are therefore always recorded:
    /// `input` and `output_rendering`, which say where the pixels come from
    /// and how they leave (ADR 0044 §3). The map is empty only on settings
    /// no engine has pinned yet — freshly built in memory, or read from a
    /// revision written before those two stages existed.
    #[serde(default, skip_serializing_if = "StageVersions::is_empty")]
    pub stages: StageVersions,

    /// Camera profile (DCP, ADR 0035): the very first pipeline stage, even
    /// before lens correction. `None` = neutral, LibRaw's own built-in
    /// sRGB conversion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_profile: Option<CameraProfile>,
    /// Creative LUT reference (ADR 0053); `None` = no look applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lut: Option<Lut>,
    /// White balance override; `None` = as-shot (neutral).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub white_balance: Option<WhiteBalance>,
    /// Exposure compensation in EV. Neutral: 0.
    pub exposure: f64,
    /// Contrast, slider in [-100, +100]. Neutral: 0.
    pub contrast: i32,
    /// Highlights recovery, slider in [-100, +100]. Neutral: 0.
    pub highlights: i32,
    /// Shadows lift, slider in [-100, +100]. Neutral: 0.
    pub shadows: i32,
    /// White point, slider in [-100, +100]. Neutral: 0.
    pub whites: i32,
    /// Black point, slider in [-100, +100]. Neutral: 0.
    pub blacks: i32,
    /// Local contrast at a large blur radius (ADR 0033), slider in
    /// [-100, +100]. Neutral: 0. Same algorithm family as [`Settings::texture`],
    /// a different radius constant.
    pub clarity: i32,
    /// Local contrast at a small blur radius (ADR 0033), slider in
    /// [-100, +100]. Neutral: 0.
    pub texture: i32,
    /// Dark-channel-prior haze removal (ADR 0033), slider in [-100, +100].
    /// Neutral: 0. Positive removes atmospheric haze; negative re-adds it.
    pub dehaze: i32,
    /// Vibrance, slider in [-100, +100]. Neutral: 0.
    pub vibrance: i32,
    /// Saturation, slider in [-100, +100]. Neutral: 0.
    pub saturation: i32,
    /// Black and white (ADR 0088 §3): the photo is collapsed to its luma
    /// **after** the HSL mixer, so the mixer's eight luminance sliders are
    /// the black-and-white mix. Neutral: `false`.
    ///
    /// A flag and nothing else — turning it on moves no other setting, so
    /// turning it off gives the photo back exactly as it was (§5).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub monochrome: bool,

    /// Parametric tone curve (ADR 0137), applied at rank 75 — before the
    /// point curve below, which is the precise instrument and therefore has
    /// the last word. Neutral: every region at 0.
    #[serde(default, skip_serializing_if = "ParametricCurve::is_default")]
    pub parametric_curve: ParametricCurve,
    /// Levels (ADR 0164), applied at rank 78 — after the parametric curve,
    /// before the point curve. Neutral: `0, 255, 1.0, 0, 255`.
    #[serde(default, skip_serializing_if = "Levels::is_neutral")]
    pub levels: Levels,
    /// Tone curve step. Neutral: no points.
    pub tone_curve: ToneCurve,

    /// 8-band HSL mixer (ADR 0031), fixed band order: red, orange, yellow,
    /// green, aqua, blue, purple, magenta. Neutral: every band zero.
    pub hsl: [HslBand; 8],
    /// Shadows/midtones/highlights color grading (ADR 0031). Neutral:
    /// default.
    pub color_grading: ColorGrading,

    /// Spot removal clones, applied in list order. Neutral: empty (ADR 0032).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spot_removal: Vec<SpotRemoval>,
    /// Reshape handles (ADR 0109). Neutral: empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reshape: Vec<ReshapePoint>,
    /// Red-eye corrections (ADR 0103). Empty = none, the neutral state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub red_eye: Vec<RedEye>,
    /// Backdrop extension (ADR 0172). Neutral: no region.
    #[serde(default, skip_serializing_if = "Backdrop::is_neutral")]
    pub backdrop: Backdrop,

    /// Masked local adjustments, applied in list order. Neutral: empty (ADR
    /// 0029).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub local_adjustments: Vec<LocalAdjustment>,

    /// Lens correction step.
    pub lens_correction: LensCorrection,
    /// Noise reduction step.
    pub noise_reduction: NoiseReduction,
    /// Colour moiré reduction (ADR 0165), in [0, 100]; 0 = the stage does
    /// not run. Colour only: luminance moiré is refused (§1).
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub moire: i32,
    /// Sharpening step.
    pub sharpening: Sharpening,
    /// Defringe (ADR 0113), beside the lens corrections it belongs with.
    #[serde(default, skip_serializing_if = "Defringe::is_neutral")]
    pub defringe: Defringe,

    /// How the working buffer becomes a display signal (ADR 0044 §3). Has
    /// no neutral value: some rendering always happens.
    pub output_rendering: OutputRendering,

    /// What the decoder does with channels that saturated at the sensor
    /// (ADR 0050). Neutral: [`HighlightReconstruction::Clip`], which is why
    /// it is absent from a stored document that never asked for anything.
    #[serde(default, skip_serializing_if = "HighlightReconstruction::is_clip")]
    pub highlight_reconstruction: HighlightReconstruction,

    /// Which interpolation the decoder uses (ADR 0061). Neutral:
    /// [`Demosaic::Ahd`], which is why it is absent from a stored document
    /// that never asked for anything.
    #[serde(default, skip_serializing_if = "Demosaic::is_ahd")]
    pub demosaic: Demosaic,

    /// What the file's samples already are (ADR 0107 §6). Neutral:
    /// [`SourceEncoding::Srgb`] — every file this engine decodes itself —
    /// which is why it is absent from every stored document but a derived
    /// asset's.
    #[serde(default, skip_serializing_if = "SourceEncoding::is_srgb")]
    pub source_encoding: SourceEncoding,

    /// Rotation in degrees, clockwise. Neutral: 0.
    pub rotation: f64,
    /// Perspective correction (ADR 0052); `None` = neutral.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perspective: Option<Perspective>,
    /// Crop rectangle; `None` = full frame (neutral).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,

    /// The vignette the photographer adds (ADR 0090 §2), drawn on the
    /// cropped frame. Neutral: default.
    pub vignette: Vignette,
    /// Film grain (ADR 0090 §3), the last operator before the buffer
    /// becomes a display signal. Neutral: default.
    pub grain: Grain,

    /// Fields from schema versions this engine does not know, preserved
    /// verbatim for lossless round-tripping.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: CURRENT_SCHEMA,
            stages: StageVersions::new(),
            camera_profile: None,
            lut: None,
            white_balance: None,
            exposure: 0.0,
            contrast: 0,
            highlights: 0,
            shadows: 0,
            whites: 0,
            blacks: 0,
            clarity: 0,
            texture: 0,
            dehaze: 0,
            vibrance: 0,
            saturation: 0,
            monochrome: false,
            parametric_curve: ParametricCurve::default(),
            levels: Levels::default(),
            tone_curve: ToneCurve::default(),
            hsl: [HslBand::default(); 8],
            color_grading: ColorGrading::default(),
            spot_removal: Vec::new(),
            reshape: Vec::new(),
            red_eye: Vec::new(),
            backdrop: Backdrop::default(),
            local_adjustments: Vec::new(),
            lens_correction: LensCorrection::default(),
            noise_reduction: NoiseReduction::default(),
            moire: 0,
            output_rendering: OutputRendering::default(),
            highlight_reconstruction: HighlightReconstruction::default(),
            demosaic: Demosaic::default(),
            source_encoding: SourceEncoding::default(),
            sharpening: Sharpening::default(),
            defringe: Defringe::default(),
            rotation: 0.0,
            perspective: None,
            crop: None,
            vignette: Vignette::default(),
            grain: Grain::default(),
            extra: serde_json::Map::new(),
        }
    }
}

impl Settings {
    /// Parses a `settings_json` document.
    ///
    /// Documents from newer schemas parse successfully; their unknown fields
    /// land in [`Settings::extra`]. Editing such a document is the engine's
    /// responsibility to refuse.
    pub fn parse(json: &str) -> Result<Settings> {
        serde_json::from_str(json).map_err(|e| LeylineError::InvalidSettings(e.to_string()))
    }

    /// Serializes the complete state back to a `settings_json` document.
    #[allow(
        clippy::expect_used,
        reason = "serde_json only fails on a failing Serialize impl or a non-string map key, and Settings has neither"
    )]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("settings serialization cannot fail")
    }

    /// Hashes the rendering identity of `keys`, plus the schema and the
    /// pinned stage versions (ADR 0041 §3).
    ///
    /// Two settings sharing this value drive the named settings — and the
    /// pipeline built from them — identically, so a buffer rendered under
    /// one may be reused under the other. `schema` and `stages` are always
    /// folded in because a change to either rebuilds the pipeline itself,
    /// whatever the keys.
    ///
    /// Values are compared through their JSON form rather than field by
    /// field: `f64` has no `Hash`, and JSON is already the shape the
    /// reproducibility contract stores (`docs/pipeline.md` §3.2). An
    /// unknown key contributes nothing — a caller naming a field that does
    /// not exist gets a fingerprint that simply does not depend on it.
    pub fn fingerprint(&self, keys: &[&str]) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.schema.hash(&mut hasher);
        for (name, version) in &self.stages {
            name.hash(&mut hasher);
            version.hash(&mut hasher);
        }

        #[allow(
            clippy::expect_used,
            reason = "serde_json only fails on a failing Serialize impl or a non-string map key, and Settings has neither"
        )]
        let json = serde_json::to_value(self).expect("settings serialization cannot fail");
        #[allow(
            clippy::expect_used,
            reason = "Settings is a struct, which serde_json always serializes to an object"
        )]
        let object = json.as_object().expect("settings serialize to an object");
        // Sorted and deduplicated so the value depends on *which* settings
        // were named, never on the order they were named in.
        let mut keys: Vec<&str> = keys.to_vec();
        keys.sort_unstable();
        keys.dedup();
        for key in keys {
            key.hash(&mut hasher);
            if let Some(value) = object.get(key) {
                value.to_string().hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    /// Validates value ranges for schema 1.
    ///
    /// Only meaningful before *writing* a schema-1 revision; documents from
    /// newer schemas are not covered by these rules.
    pub fn validate(&self) -> Result<()> {
        use crate::validate_library_relative_path;

        // `process` was removed by ADR 0043, so it now falls through to
        // `extra` like any unknown field — which is exactly wrong for it.
        // The lossless-unknown-fields rule exists to protect a *newer*
        // engine's work; a *removed* field means the document predates the
        // stage map, and rendering it as if it did not would silently give
        // it another engine's pixels. Refuse it instead.
        if self.extra.contains_key("process") {
            return Err(LeylineError::InvalidSettings(
                "settings carry `process`, removed by ADR 0043: this revision \
                 predates the stage map and cannot be rendered"
                    .to_owned(),
            ));
        }

        // Same capability rule as ADR 0048 §5, one layer lower: the mode is
        // a *decoder* configuration, and `input::v1` has no code that reads
        // it. Keeping the pinned version and dropping the mode would leave
        // the user with a setting that does nothing (ADR 0050 §5).
        // Same capability rule as `highlight_reconstruction` below: a
        // setting the pinned version cannot express is refused by name, so
        // the user learns their revision needs reprocessing instead of
        // watching a control do nothing (ADR 0061 §2).
        if !self.demosaic.is_ahd() && matches!(self.stages.get("input"), Some(&v) if v < 3) {
            return Err(LeylineError::InvalidSettings(
                "demosaic needs stage input version 3, but this revision \
                 pins an earlier one; reprocess it first"
                    .to_owned(),
            ));
        }
        if !self.highlight_reconstruction.is_clip() && self.stages.get("input") == Some(&1) {
            return Err(LeylineError::InvalidSettings(
                "highlight_reconstruction needs stage input version 2, but this revision \
                 pins version 1; reprocess the photo to the current stage versions first"
                    .to_owned(),
            ));
        }
        // The capability rule once more, and here it guards more than a
        // control doing nothing: an earlier `input` would convert a buffer
        // that is already in the working space, so the photograph would come
        // back visibly wrong rather than merely unchanged (ADR 0107 §6).
        if !self.source_encoding.is_srgb() && matches!(self.stages.get("input"), Some(&v) if v < 5)
        {
            return Err(LeylineError::InvalidSettings(
                "source_encoding needs stage input version 5, but this revision \
                 pins an earlier one; reprocess it first"
                    .to_owned(),
            ));
        }

        fn slider(name: &str, value: i32, min: i32, max: i32) -> Result<()> {
            if (min..=max).contains(&value) {
                Ok(())
            } else {
                Err(LeylineError::InvalidSettings(format!(
                    "{name} must be in [{min}, {max}], got {value}"
                )))
            }
        }
        fn finite(name: &str, value: f64) -> Result<()> {
            if value.is_finite() {
                Ok(())
            } else {
                Err(LeylineError::InvalidSettings(format!(
                    "{name} must be finite, got {value}"
                )))
            }
        }

        finite("exposure", self.exposure)?;
        finite("rotation", self.rotation)?;
        if let Some(perspective) = &self.perspective {
            slider("perspective.vertical", perspective.vertical, -100, 100)?;
            slider("perspective.horizontal", perspective.horizontal, -100, 100)?;
        }
        slider("contrast", self.contrast, -100, 100)?;
        slider("highlights", self.highlights, -100, 100)?;
        slider("shadows", self.shadows, -100, 100)?;
        slider("whites", self.whites, -100, 100)?;
        slider("blacks", self.blacks, -100, 100)?;
        slider("clarity", self.clarity, -100, 100)?;
        slider("texture", self.texture, -100, 100)?;
        slider("dehaze", self.dehaze, -100, 100)?;
        slider("vibrance", self.vibrance, -100, 100)?;
        slider("saturation", self.saturation, -100, 100)?;
        slider("vignette.amount", self.vignette.amount, -100, 100)?;
        slider("vignette.midpoint", self.vignette.midpoint, 0, 100)?;
        slider("vignette.roundness", self.vignette.roundness, -100, 100)?;
        slider("vignette.feather", self.vignette.feather, 0, 100)?;
        slider("grain.amount", self.grain.amount, 0, 100)?;
        slider("grain.size", self.grain.size, 0, 100)?;
        slider("grain.roughness", self.grain.roughness, 0, 100)?;
        slider("grain.color", self.grain.color, 0, 100)?;
        // The capability rule again (ADR 0118 §4): `grain::v1` is frozen and
        // has no code that reads `color`, so keeping the pinned version and
        // dropping the setting would leave a slider that does nothing.
        if self.grain.color != 0 && self.stages.get("grain") == Some(&1) {
            return Err(LeylineError::InvalidSettings(
                "grain.color needs stage grain version 2, but this revision pins \
                 version 1; reprocess the photo to the current stage versions first"
                    .to_owned(),
            ));
        }
        // The parametric curve (ADR 0137 §1). The splits are **strictly**
        // increasing, which is stricter than Lightroom and costs a
        // photographer nothing — a region of zero width has no slider that
        // can reach it — while buying the control points of §3 their
        // strictly increasing `x` for free.
        for (name, value) in [
            ("parametric_curve.shadows", self.parametric_curve.shadows),
            ("parametric_curve.darks", self.parametric_curve.darks),
            ("parametric_curve.lights", self.parametric_curve.lights),
            (
                "parametric_curve.highlights",
                self.parametric_curve.highlights,
            ),
        ] {
            slider(name, value, -100, 100)?;
        }
        for (name, value) in [
            (
                "parametric_curve.shadow_split",
                self.parametric_curve.shadow_split,
            ),
            (
                "parametric_curve.midtone_split",
                self.parametric_curve.midtone_split,
            ),
            (
                "parametric_curve.highlight_split",
                self.parametric_curve.highlight_split,
            ),
        ] {
            slider(name, value, 1, 99)?;
        }
        if self.parametric_curve.shadow_split >= self.parametric_curve.midtone_split
            || self.parametric_curve.midtone_split >= self.parametric_curve.highlight_split
        {
            return Err(LeylineError::InvalidSettings(
                "parametric_curve splits must be strictly increasing".to_owned(),
            ));
        }

        slider("moire", self.moire, 0, 100)?;

        // Levels (ADR 0164 §3).
        let levels = &self.levels;
        slider("levels.input_black", levels.input_black, 0, 254)?;
        slider("levels.input_white", levels.input_white, 1, 255)?;
        slider("levels.output_black", levels.output_black, 0, 255)?;
        slider("levels.output_white", levels.output_white, 0, 255)?;
        if levels.input_black >= levels.input_white {
            return Err(LeylineError::InvalidSettings(
                "levels.input_black must be below levels.input_white".to_owned(),
            ));
        }
        if levels.output_black > levels.output_white {
            return Err(LeylineError::InvalidSettings(
                "levels.output_black must not exceed levels.output_white".to_owned(),
            ));
        }
        if !(0.10..=9.99).contains(&levels.gamma) {
            return Err(LeylineError::InvalidSettings(format!(
                "levels.gamma must be in [0.10, 9.99], got {}",
                levels.gamma
            )));
        }

        // The master curve and the three channel curves are validated by
        // the same rules (ADR 0098 §1), so they are validated by the same
        // code: a curve that is legal as the master is legal as a channel.
        for (field, points) in [
            ("tone_curve.points", &self.tone_curve.points),
            ("tone_curve.red", &self.tone_curve.red),
            ("tone_curve.green", &self.tone_curve.green),
            ("tone_curve.blue", &self.tone_curve.blue),
        ] {
            if points.is_empty() {
                continue;
            }
            if points.len() < 2 {
                return Err(LeylineError::InvalidSettings(format!(
                    "{field} must have at least 2 points, or be empty"
                )));
            }
            let mut previous_x = None;
            for point in points {
                for (name, value) in [
                    (format!("{field}.x"), point.x),
                    (format!("{field}.y"), point.y),
                ] {
                    if !(0.0..=1.0).contains(&value) {
                        return Err(LeylineError::InvalidSettings(format!(
                            "{name} must be in [0, 1], got {value}"
                        )));
                    }
                }
                if let Some(previous_x) = previous_x
                    && point.x <= previous_x
                {
                    return Err(LeylineError::InvalidSettings(format!(
                        "{field} must have strictly increasing x"
                    )));
                }
                previous_x = Some(point.x);
            }
        }
        // The capability rule (ADR 0098 §3): v1 applies one curve to every
        // channel and has no code that reads the three below.
        if self.tone_curve.has_channel_curves() && self.stages.get("tone_curve") == Some(&1) {
            return Err(LeylineError::InvalidSettings(
                "tone_curve.red/green/blue need stage tone_curve version 2, but this revision \
                 pins version 1; reprocess the photo to the current stage versions first"
                    .to_owned(),
            ));
        }
        self.validate_backdrop()?;
        // Validated exactly like `spot_removal` below, which shares its
        // geometry (ADR 0103 §1).
        for (i, eye) in self.red_eye.iter().enumerate() {
            for (name, value) in [
                (format!("red_eye[{i}].center.x"), eye.center.x),
                (format!("red_eye[{i}].center.y"), eye.center.y),
                (format!("red_eye[{i}].feather"), eye.feather),
                (format!("red_eye[{i}].darken"), eye.darken),
            ] {
                if !(0.0..=1.0).contains(&value) {
                    return Err(LeylineError::InvalidSettings(format!(
                        "{name} must be in [0, 1], got {value}"
                    )));
                }
            }
            if !(eye.radius > 0.0 && eye.radius <= 1.0) {
                return Err(LeylineError::InvalidSettings(format!(
                    "red_eye[{i}].radius must be in (0, 1], got {}",
                    eye.radius
                )));
            }
        }
        for (i, point) in self.reshape.iter().enumerate() {
            for (name, value) in [
                (format!("reshape[{i}].from.x"), point.from.x),
                (format!("reshape[{i}].from.y"), point.from.y),
                (format!("reshape[{i}].to.x"), point.to.x),
                (format!("reshape[{i}].to.y"), point.to.y),
            ] {
                if !(0.0..=1.0).contains(&value) {
                    return Err(LeylineError::InvalidSettings(format!(
                        "{name} must be in [0, 1], got {value}"
                    )));
                }
            }
            if !(point.radius > 0.0 && point.radius <= 1.0) {
                return Err(LeylineError::InvalidSettings(format!(
                    "reshape[{i}].radius must be in (0, 1], got {}",
                    point.radius
                )));
            }
            if !(0.0..=1.0).contains(&point.strength) {
                return Err(LeylineError::InvalidSettings(format!(
                    "reshape[{i}].strength must be in [0, 1], got {}",
                    point.strength
                )));
            }
        }
        for (i, spot) in self.spot_removal.iter().enumerate() {
            for (name, value) in [
                (format!("spot_removal[{i}].target.x"), spot.target.x),
                (format!("spot_removal[{i}].target.y"), spot.target.y),
                (format!("spot_removal[{i}].source.x"), spot.source.x),
                (format!("spot_removal[{i}].source.y"), spot.source.y),
            ] {
                if !(0.0..=1.0).contains(&value) {
                    return Err(LeylineError::InvalidSettings(format!(
                        "{name} must be in [0, 1], got {value}"
                    )));
                }
            }
            if !(spot.radius > 0.0 && spot.radius <= 1.0) {
                return Err(LeylineError::InvalidSettings(format!(
                    "spot_removal[{i}].radius must be in (0, 1], got {}",
                    spot.radius
                )));
            }
            if !(0.0..=1.0).contains(&spot.feather) {
                return Err(LeylineError::InvalidSettings(format!(
                    "spot_removal[{i}].feather must be in [0, 1], got {}",
                    spot.feather
                )));
            }
            if !(0.0..=1.0).contains(&spot.opacity) {
                return Err(LeylineError::InvalidSettings(format!(
                    "spot_removal[{i}].opacity must be in [0, 1], got {}",
                    spot.opacity
                )));
            }
        }
        for (i, adjustment) in self.local_adjustments.iter().enumerate() {
            if !(0.0..=1.0).contains(&adjustment.opacity) {
                return Err(LeylineError::InvalidSettings(format!(
                    "local_adjustments[{i}].opacity must be in [0, 1], got {}",
                    adjustment.opacity
                )));
            }
            if matches!(adjustment.mask, Mask::Coverage { .. })
                && matches!(self.stages.get("local_adjustments"), Some(&v) if v < 3)
            {
                // Same capability rule as `range` below, one variant wider:
                // v1 and v2 have no code that reads a stored coverage, and an
                // ignored mask is a local adjustment applied to the whole
                // image (ADR 0070 §4).
                return Err(LeylineError::InvalidSettings(format!(
                    "local_adjustments[{i}].mask is a stored coverage, which needs \
                     stage local_adjustments version 3, but this revision pins an \
                     earlier one; reprocess the photo to the current stage versions \
                     first"
                )));
            }
            validate_mask(&format!("local_adjustments[{i}].mask"), &adjustment.mask)?;
            let unit = |name: &str, value: f64| -> Result<()> {
                if (0.0..=1.0).contains(&value) {
                    Ok(())
                } else {
                    Err(LeylineError::InvalidSettings(format!(
                        "local_adjustments[{i}].{name} must be in [0, 1], got {value}"
                    )))
                }
            };

            if let Some(range) = &adjustment.range {
                // A range is a *capability* of the stage version, not just a
                // value: v1 has no code for it, and the pinning rule
                // (ADR 0042 §2) keeps a pinned stage at its version. Refusing
                // is the only honest outcome — the alternative is a slider
                // that does nothing (ADR 0048 §5).
                if self.stages.get("local_adjustments") == Some(&1) {
                    return Err(LeylineError::InvalidSettings(format!(
                        "local_adjustments[{i}].range needs stage local_adjustments version 2, \
                         but this revision pins version 1; reprocess the photo to the current \
                         stage versions first"
                    )));
                }
                if let Some(luminance) = &range.luminance {
                    unit("range.luminance.min", luminance.min)?;
                    unit("range.luminance.max", luminance.max)?;
                    unit("range.luminance.softness", luminance.softness)?;
                    if luminance.max < luminance.min {
                        return Err(LeylineError::InvalidSettings(format!(
                            "local_adjustments[{i}].range.luminance.max must be >= min, got {} < {}",
                            luminance.max, luminance.min
                        )));
                    }
                }
                if let Some(color) = &range.color {
                    finite("range.color.center", color.center)?;
                    if !(0.0..=180.0).contains(&color.width) || color.width <= 0.0 {
                        return Err(LeylineError::InvalidSettings(format!(
                            "local_adjustments[{i}].range.color.width must be in (0, 180], got {}",
                            color.width
                        )));
                    }
                    if !(0.0..=180.0).contains(&color.softness) {
                        return Err(LeylineError::InvalidSettings(format!(
                            "local_adjustments[{i}].range.color.softness must be in [0, 180], got {}",
                            color.softness
                        )));
                    }
                }
            }

            let values = &adjustment.adjustments;
            if let Some(temperature) = values.temperature
                && temperature == 0
            {
                return Err(LeylineError::InvalidSettings(format!(
                    "local_adjustments[{i}].adjustments.temperature must be strictly positive"
                )));
            }
            if let Some(tint) = values.tint {
                slider(
                    &format!("local_adjustments[{i}].adjustments.tint"),
                    tint,
                    -100,
                    100,
                )?;
            }
            if let Some(exposure) = values.exposure {
                finite(
                    &format!("local_adjustments[{i}].adjustments.exposure"),
                    exposure,
                )?;
            }
            // The five neighbourhood operators are a *capability* of v4, for
            // the same reason a range mask is one of v2 and a stored coverage
            // one of v3: the earlier versions have no code for them, and the
            // pinning rule (ADR 0042 §2) keeps a pinned stage where it is.
            // Refusing names the version; dropping them would be a slider
            // that does nothing (ADR 0048 §5, applied again by ADR 0108 §5).
            if values.uses_neighbourhood_operators()
                && matches!(self.stages.get("local_adjustments"), Some(&v) if v < 4)
            {
                return Err(LeylineError::InvalidSettings(format!(
                    "local_adjustments[{i}].adjustments asks for clarity, texture, \
                     sharpness or noise, which needs stage local_adjustments version 4, \
                     but this revision pins an earlier one; reprocess the photo to the \
                     current stage versions first"
                )));
            }
            // The defringe pair is a capability of v5, the sixth time this
            // rule applies (ADR 0116 §4).
            if values.uses_defringe()
                && matches!(self.stages.get("local_adjustments"), Some(&v) if v < 5)
            {
                return Err(LeylineError::InvalidSettings(format!(
                    "local_adjustments[{i}].adjustments asks for defringe, which needs \
                     stage local_adjustments version 5, but this revision pins an \
                     earlier one; reprocess the photo to the current stage versions first"
                )));
            }
            for (name, value) in [
                ("contrast", values.contrast),
                ("highlights", values.highlights),
                ("shadows", values.shadows),
                ("whites", values.whites),
                ("blacks", values.blacks),
                ("vibrance", values.vibrance),
                ("saturation", values.saturation),
                ("clarity", values.clarity),
                ("texture", values.texture),
                ("sharpness", values.sharpness),
            ] {
                if let Some(value) = value {
                    slider(
                        &format!("local_adjustments[{i}].adjustments.{name}"),
                        value,
                        -100,
                        100,
                    )?;
                }
            }
            // Noise reduction has no meaningful negative, exactly as the
            // global sliders of `noise_reduction` have none.
            for (name, value) in [
                ("noise_luminance", values.noise_luminance),
                ("noise_color", values.noise_color),
                ("defringe_purple", values.defringe_purple),
                ("defringe_green", values.defringe_green),
            ] {
                if let Some(value) = value {
                    slider(
                        &format!("local_adjustments[{i}].adjustments.{name}"),
                        value,
                        0,
                        100,
                    )?;
                }
            }
        }
        const HSL_BAND_NAMES: [&str; 8] = [
            "red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta",
        ];
        for (band, name) in self.hsl.iter().zip(HSL_BAND_NAMES) {
            slider(&format!("hsl.{name}.hue"), band.hue, -100, 100)?;
            slider(
                &format!("hsl.{name}.saturation"),
                band.saturation,
                -100,
                100,
            )?;
            slider(&format!("hsl.{name}.luminance"), band.luminance, -100, 100)?;
        }
        for (zone, name) in [
            (&self.color_grading.shadows, "shadows"),
            (&self.color_grading.midtones, "midtones"),
            (&self.color_grading.highlights, "highlights"),
        ] {
            if !(0..360).contains(&zone.hue) {
                return Err(LeylineError::InvalidSettings(format!(
                    "color_grading.{name}.hue must be in [0, 360), got {}",
                    zone.hue
                )));
            }
            slider(
                &format!("color_grading.{name}.saturation"),
                zone.saturation,
                0,
                100,
            )?;
            slider(
                &format!("color_grading.{name}.luminance"),
                zone.luminance,
                -100,
                100,
            )?;
        }
        slider(
            "color_grading.balance",
            self.color_grading.balance,
            -100,
            100,
        )?;
        slider(
            "color_grading.blending",
            self.color_grading.blending,
            0,
            100,
        )?;
        slider(
            "noise_reduction.luminance",
            self.noise_reduction.luminance,
            0,
            100,
        )?;
        slider("noise_reduction.color", self.noise_reduction.color, 0, 100)?;
        slider(
            "output_rendering.highlight_rolloff",
            self.output_rendering.highlight_rolloff,
            0,
            100,
        )?;
        slider("sharpening.amount", self.sharpening.amount, 0, 100)?;
        finite("sharpening.radius", self.sharpening.radius)?;
        if self.sharpening.radius <= 0.0 {
            return Err(LeylineError::InvalidSettings(format!(
                "sharpening.radius must be strictly positive, got {}",
                self.sharpening.radius
            )));
        }
        slider("sharpening.masking", self.sharpening.masking, 0, 100)?;
        slider("defringe.purple", self.defringe.purple, 0, 100)?;
        slider("defringe.green", self.defringe.green, 0, 100)?;
        // The capability rule (ADR 0096 §3): v1 has no edge mask, and the
        // pinning rule (ADR 0042 §2) keeps a pinned stage at its version.
        // Refusing is the only honest outcome — the alternative is a slider
        // that does nothing, which nobody sees.
        if self.sharpening.masking != 0 && self.stages.get("sharpen") == Some(&1) {
            return Err(LeylineError::InvalidSettings(
                "sharpening.masking needs stage sharpen version 2, but this revision pins \
                 version 1; reprocess the photo to the current stage versions first"
                    .to_owned(),
            ));
        }
        // Manual transverse chromatic aberration (ADR 0111 §1): a percent of
        // the radius, and one percent is already far beyond any real lens.
        for (name, value) in [
            ("lens_correction.tca_red", self.lens_correction.tca_red),
            ("lens_correction.tca_blue", self.lens_correction.tca_blue),
        ] {
            finite(name, value)?;
            if !(-1.0..=1.0).contains(&value) {
                return Err(LeylineError::InvalidSettings(format!(
                    "{name} must be in [-1, 1] percent of the radius, got {value}"
                )));
            }
        }
        // The capability rule again (ADR 0111 §5), for the same reason as
        // `sharpening.masking` above: v1 has no manual map.
        if self.lens_correction.has_manual_tca() && self.stages.get("lens") == Some(&1) {
            return Err(LeylineError::InvalidSettings(
                "lens_correction.tca_red/tca_blue need stage lens version 2, but this \
                 revision pins version 1; reprocess the photo to the current stage \
                 versions first"
                    .to_owned(),
            ));
        }
        if let Some(profile) = &self.camera_profile {
            validate_library_relative_path("camera_profile.path", &profile.path)?;
            let hex = profile.checksum.strip_prefix("blake3:").ok_or_else(|| {
                LeylineError::InvalidSettings(
                    "camera_profile.checksum must start with \"blake3:\"".to_owned(),
                )
            })?;
            if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(LeylineError::InvalidSettings(
                    "camera_profile.checksum must be \"blake3:\" followed by 64 hex digits"
                        .to_owned(),
                ));
            }
        }
        if let Some(lut) = &self.lut {
            validate_library_relative_path("lut.path", &lut.path)?;
            let hex = lut.checksum.strip_prefix("blake3:").ok_or_else(|| {
                LeylineError::InvalidSettings("lut.checksum must start with \"blake3:\"".to_owned())
            })?;
            if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(LeylineError::InvalidSettings(
                    "lut.checksum must be \"blake3:\" followed by 64 hex digits".to_owned(),
                ));
            }
            slider("lut.strength", lut.strength, 0, 100)?;
        }
        if let Some(wb) = &self.white_balance {
            slider("white_balance.tint", wb.tint, -100, 100)?;
            if wb.temperature == 0 {
                return Err(LeylineError::InvalidSettings(
                    "white_balance.temperature must be strictly positive".to_owned(),
                ));
            }
        }
        if let Some(crop) = &self.crop {
            for (name, value) in [("crop.x", crop.x), ("crop.y", crop.y)] {
                if !(0.0..=1.0).contains(&value) {
                    return Err(LeylineError::InvalidSettings(format!(
                        "{name} must be in [0, 1], got {value}"
                    )));
                }
            }
            for (name, value) in [("crop.width", crop.width), ("crop.height", crop.height)] {
                if !(value > 0.0 && value <= 1.0) {
                    return Err(LeylineError::InvalidSettings(format!(
                        "{name} must be in (0, 1], got {value}"
                    )));
                }
            }
            if crop.x + crop.width > 1.0 || crop.y + crop.height > 1.0 {
                return Err(LeylineError::InvalidSettings(
                    "crop rectangle exceeds the image bounds".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// `Option<Option<T>>` where the outer level means *included* and the inner
/// one means *cleared*, so `null` in the document has to survive the trip.
///
/// Serde's default deserialization of `Option<Option<T>>` folds `null` and
/// "key absent" into the same `None`, which for these fields is the
/// difference between "this preset clears the crop" and "this preset does
/// not touch the crop". Found by [`PresetSettings`]'s round-trip test the
/// day ADR 0132 gave `perspective` the same shape `crop` already had — and
/// it was silently wrong for `crop` before that: a Geometry preset captured
/// from an uncropped photograph stored `"crop": null` and read back as *not
/// included*, so applying it left the target's crop alone instead of
/// clearing it.
///
/// Paired with `skip_serializing_if = "Option::is_none"`, which is what
/// keeps "not included" out of the document entirely.
mod double_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<T, S>(
        value: &Option<Option<T>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        T: Serialize,
        S: Serializer,
    {
        value.serialize(serializer)
    }

    pub(super) fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        // The key is present — `null` included — so it *is* included, and
        // the inner `Option` carries whether it holds a value.
        Option::deserialize(deserializer).map(Some)
    }
}

impl Settings {
    /// The backdrop's own rules (ADR 0172 §2); its masks go through
    /// [`validate_mask`] like a local adjustment's.
    fn validate_backdrop(&self) -> Result<()> {
        let backdrop = &self.backdrop;
        let invalid = |message: String| Err(LeylineError::InvalidSettings(message));
        let polygon = |label: &str, points: &[Point]| -> Result<()> {
            if points.len() < 3 {
                return invalid(format!(
                    "{label} needs at least 3 vertices, got {}",
                    points.len()
                ));
            }
            for (k, point) in points.iter().enumerate() {
                for (axis, value) in [("x", point.x), ("y", point.y)] {
                    if !(0.0..=1.0).contains(&value) {
                        return invalid(format!(
                            "{label}[{k}].{axis} must be in [0, 1], got {value}"
                        ));
                    }
                }
            }
            Ok(())
        };
        for (i, region) in backdrop.regions.iter().enumerate() {
            polygon(&format!("backdrop.regions[{i}].source"), &region.source)?;
            if let BackdropZone::Polygons(zones) = &region.zone {
                if zones.is_empty() {
                    return invalid(format!(
                        "backdrop.regions[{i}].zone.polygons must not be empty"
                    ));
                }
                for (j, zone) in zones.iter().enumerate() {
                    polygon(&format!("backdrop.regions[{i}].zone.polygons[{j}]"), zone)?;
                }
            }
            for (j, hole) in region.holes.iter().enumerate() {
                polygon(&format!("backdrop.regions[{i}].holes[{j}]"), hole)?;
            }
        }
        for (i, protect) in backdrop.protect.iter().enumerate() {
            validate_mask(&format!("backdrop.protect[{i}].mask"), &protect.mask)?;
        }
        for (i, mask) in backdrop.release.iter().enumerate() {
            validate_mask(&format!("backdrop.release[{i}]"), mask)?;
        }
        if !(0.0..=0.05).contains(&backdrop.feather) {
            return invalid(format!(
                "backdrop.feather must be in [0, 0.05], got {}",
                backdrop.feather
            ));
        }
        if !(0..=200).contains(&backdrop.texture) {
            return invalid(format!(
                "backdrop.texture must be in [0, 200], got {}",
                backdrop.texture
            ));
        }
        Ok(())
    }
}

/// Validates one mask's geometry, every message prefixed by `label` (the
/// mask's path in the settings, e.g. `local_adjustments[2].mask`). Shared by
/// the local adjustments and the backdrop's protect/release masks
/// (ADR 0172 §2), which use the same shapes and so the same rules.
fn validate_mask(label: &str, mask: &Mask) -> Result<()> {
    let invalid = |message: String| Err(LeylineError::InvalidSettings(message));
    let unit = |name: &str, value: f64| -> Result<()> {
        if (0.0..=1.0).contains(&value) {
            Ok(())
        } else {
            invalid(format!("{label}.{name} must be in [0, 1], got {value}"))
        }
    };
    match mask {
        Mask::Radial {
            cx,
            cy,
            rx,
            ry,
            angle,
            feather,
            inverted: _,
        } => {
            unit("cx", *cx)?;
            unit("cy", *cy)?;
            if *rx <= 0.0 || *ry <= 0.0 {
                return invalid(format!(
                    "{label}.rx/ry must be strictly positive, got {rx}/{ry}"
                ));
            }
            if !angle.is_finite() {
                return invalid(format!("{label}.angle must be finite, got {angle}"));
            }
            unit("feather", *feather)?;
        }
        Mask::Gradient { x0, y0, x1, y1 } => {
            unit("x0", *x0)?;
            unit("y0", *y0)?;
            unit("x1", *x1)?;
            unit("y1", *y1)?;
            if (x1 - x0).hypot(y1 - y0) <= 0.0 {
                return invalid(format!("{label}: gradient endpoints must differ"));
            }
        }
        Mask::Everything => {}
        Mask::Brush { strokes } => {
            if strokes.is_empty() {
                return invalid(format!("{label}.strokes must not be empty"));
            }
            for (j, stroke) in strokes.iter().enumerate() {
                unit(&format!("strokes[{j}].x"), stroke.x)?;
                unit(&format!("strokes[{j}].y"), stroke.y)?;
                if stroke.radius <= 0.0 {
                    return invalid(format!(
                        "{label}.strokes[{j}].radius must be strictly positive, got {}",
                        stroke.radius
                    ));
                }
                unit(&format!("strokes[{j}].flow"), stroke.flow)?;
                unit(&format!("strokes[{j}].hardness"), stroke.hardness)?;
            }
        }
        Mask::Coverage { path, checksum } => {
            crate::validate_library_relative_path(&format!("{label}.path"), path)?;
            if !checksum.starts_with("blake3:") || checksum.len() != "blake3:".len() + 64 {
                return invalid(format!(
                    "{label}.checksum must be \"blake3:<64 hex>\", got {checksum:?}"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod specification;
