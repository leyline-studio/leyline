//! What a preset captures: the setting groups and the partial settings they
//! carry (`docs/presets.md` §3).

use super::*;

/// One category of develop settings a preset can capture (`docs/presets.md`
/// §3.1). Atomic: including a group captures — or applies — all of its
/// fields together, never a single field of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsGroup {
    /// [`Settings::white_balance`].
    WhiteBalance,
    /// [`Settings::exposure`], `contrast`, `highlights`, `shadows`, `whites`, `blacks`.
    Tone,
    /// [`Settings::clarity`], `texture`, `dehaze`, `vibrance`, `saturation`,
    /// `monochrome` — the whole of Basic below the tonal sliders
    /// (ADR 0132 §1). The first three joined the category with ADR 0132; a
    /// preset written before it simply does not carry them.
    Presence,
    /// [`Settings::tone_curve`] (ADR 0098) and
    /// [`Settings::parametric_curve`] (ADR 0137) — one block of the panel
    /// and one idea.
    ToneCurve,
    /// [`Settings::hsl`], all eight bands (ADR 0047).
    ColorMixer,
    /// [`Settings::color_grading`] (ADR 0047).
    ColorGrading,
    /// [`Settings::camera_profile`] (ADR 0035).
    CameraProfile,
    /// [`Settings::lut`] (ADR 0053).
    CreativeLut,
    /// [`Settings::vignette`], `grain` (ADR 0090 §5) — the two halves of a
    /// look a "film" preset would be missing without them.
    Effects,
    /// [`Settings::lens_correction`] and [`Settings::defringe`] — what the
    /// lens did to this photograph (ADR 0113 §5).
    LensCorrection,
    /// [`Settings::noise_reduction`], `sharpening`.
    Detail,
    /// [`Settings::highlight_reconstruction`], `demosaic`,
    /// `output_rendering` — how the photograph comes out of the file
    /// (ADR 0132 §1). Not included by default: it is a decision about *this*
    /// file, and a look copied onto a series does not mean to change a
    /// demosaic.
    Rendering,
    /// [`Settings::rotation`], `crop`, `perspective`. Never included by
    /// default when a preset is created (`docs/presets.md` §3.1): geometry
    /// is a per-photo judgment, not a reproducible style.
    Geometry,
    /// [`Settings::reshape`] (ADR 0109). Positional, so never included by
    /// default (ADR 0132 §6).
    Reshape,
    /// [`Settings::spot_removal`]. Positional — but the one positional
    /// category with a real batch use: sensor dust lands in the same place
    /// on every frame.
    SpotRemoval,
    /// [`Settings::red_eye`] (ADR 0103). Positional, so never included by
    /// default.
    RedEye,
    /// [`Settings::local_adjustments`] (ADR 0029). Positional, so never
    /// included by default — though a graduated filter over a horizon often
    /// suits a whole series.
    LocalAdjustments,
}

/// A named, partial jeu of develop settings (`docs/presets.md` §3.2,
/// `preset_json`), unlike [`Settings`] which is always complete.
///
/// Every key a `settings_json` document can carry (`docs/pipeline.md` §3.2),
/// sorted as the serialized document sorts them.
///
/// Except the ones from a future version: [`Settings::extra`] is flattened,
/// so what it round-trips appears at the top level under names this version
/// has never heard of (§3.4).
///
/// Published because a client that *names* a change needs the whole list to
/// name it from (ADR 0142 §3): the develop history turns keys into labels,
/// and a key with no label would go on screen as a raw identifier. Kept
/// honest by `every_settings_key_is_named_in_the_pipeline_specification`,
/// which builds a settings value with every field off its neutral and
/// compares what it serializes with this list.
pub const SETTINGS_KEYS: &[&str] = &[
    "blacks",
    "camera_profile",
    "clarity",
    "color_grading",
    "contrast",
    "crop",
    "defringe",
    "dehaze",
    "demosaic",
    "exposure",
    "grain",
    "highlight_reconstruction",
    "highlights",
    "hsl",
    "lens_correction",
    "levels",
    "local_adjustments",
    "lut",
    "moire",
    "monochrome",
    "noise_reduction",
    "output_rendering",
    "parametric_curve",
    "perspective",
    "red_eye",
    "reshape",
    "rotation",
    "saturation",
    "schema",
    "shadows",
    "sharpening",
    "source_encoding",
    "spot_removal",
    "stages",
    "texture",
    "tone_curve",
    "vibrance",
    "vignette",
    "white_balance",
    "whites",
];

/// A field is `Some` if and only if its [`SettingsGroup`] is in `groups` —
/// that list is the source of truth for what the preset touches; an absent
/// field means "leave untouched", never "neutral value" (the opposite rule
/// from [`Settings`], `docs/presets.md` §3.2). No stage versions: a preset
/// never fixes a rendering version, only values — the stages come from the
/// revision it is applied to (ADR 0043 §3).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetSettings {
    /// Version of the settings *format* this preset's fields use — the same
    /// numbering as [`Settings::schema`], not an independent space.
    pub schema: u32,
    /// The categories this preset touches.
    pub groups: Vec<SettingsGroup>,

    /// `Some(None)` = included, reset to as-shot; `Some(Some(wb))` = included
    /// with an override; `None` = category not included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub white_balance: Option<Option<WhiteBalance>>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f64>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlights: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadows: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whites: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blacks: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Presence`]
    /// (ADR 0132 §1). Absent from a preset written before that ADR, which
    /// then leaves the setting alone — the rule every absent field follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clarity: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Presence`] (ADR 0132 §1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Presence`] (ADR 0132 §1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dehaze: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Presence`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vibrance: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Presence`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturation: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Presence`]: black
    /// and white is a presence decision, so a preset that captures presence
    /// carries it (ADR 0088 §Consequences).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monochrome: Option<bool>,

    /// Present when `groups` includes [`SettingsGroup::Effects`] (ADR 0090 §5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vignette: Option<Vignette>,
    /// Present when `groups` includes [`SettingsGroup::Effects`] (ADR 0090 §5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grain: Option<Grain>,

    /// Present when `groups` includes [`SettingsGroup::LensCorrection`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens_correction: Option<LensCorrection>,
    /// Present when `groups` includes [`SettingsGroup::LensCorrection`]: the
    /// category is *what the lens did to this photograph*, not the
    /// `LensCorrection` struct, so defringe travels with it (ADR 0113 §5).
    /// Absent from a preset written before ADR 0113, which then leaves the
    /// setting alone — the rule every absent field follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defringe: Option<Defringe>,
    /// Present when `groups` includes [`SettingsGroup::Detail`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noise_reduction: Option<NoiseReduction>,
    /// Present when `groups` includes [`SettingsGroup::Detail`] (ADR 0165
    /// §3); absent from a preset written before that ADR, which then leaves
    /// moiré alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moire: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Detail`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharpening: Option<Sharpening>,

    /// Present when `groups` includes [`SettingsGroup::Geometry`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    /// `Some(None)` = included, cleared to full frame; `Some(Some(c))` =
    /// included with a crop; `None` = category not included.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "double_option"
    )]
    pub crop: Option<Option<Crop>>,
    /// `Some(None)` = included, cleared to no keystone; `Some(Some(p))` =
    /// included with a correction; `None` = category not included
    /// (ADR 0132 §1).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "double_option"
    )]
    pub perspective: Option<Option<Perspective>>,

    /// Present when `groups` includes [`SettingsGroup::ToneCurve`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_curve: Option<ToneCurve>,
    /// Present when `groups` includes [`SettingsGroup::ToneCurve`]
    /// (ADR 0137 §5). Absent from a preset written before that ADR, which
    /// then leaves the parametric curve alone — the rule every absent field
    /// follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parametric_curve: Option<ParametricCurve>,
    /// Present when `groups` includes [`SettingsGroup::ToneCurve`]
    /// (ADR 0164 §5); absent from a preset written before that ADR, which
    /// then leaves levels alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<Levels>,
    /// Present when `groups` includes [`SettingsGroup::ColorMixer`] — all
    /// eight bands together, since a mix is one decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hsl: Option<[HslBand; 8]>,
    /// Present when `groups` includes [`SettingsGroup::ColorGrading`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_grading: Option<ColorGrading>,
    /// `Some(None)` = included, back to the decoder's own conversion;
    /// `Some(Some(p))` = included with a profile; `None` = not included.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "double_option"
    )]
    pub camera_profile: Option<Option<CameraProfile>>,
    /// `Some(None)` = included, no look; `Some(Some(l))` = included with
    /// one; `None` = not included.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "double_option"
    )]
    pub lut: Option<Option<Lut>>,

    /// Present when `groups` includes [`SettingsGroup::Rendering`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight_reconstruction: Option<HighlightReconstruction>,
    /// Present when `groups` includes [`SettingsGroup::Rendering`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demosaic: Option<Demosaic>,
    /// Present when `groups` includes [`SettingsGroup::Rendering`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_rendering: Option<OutputRendering>,

    /// Present when `groups` includes [`SettingsGroup::Reshape`] — the
    /// whole list, replacing whatever the target held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reshape: Option<Vec<ReshapePoint>>,
    /// Present when `groups` includes [`SettingsGroup::SpotRemoval`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spot_removal: Option<Vec<SpotRemoval>>,
    /// Present when `groups` includes [`SettingsGroup::RedEye`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red_eye: Option<Vec<RedEye>>,
    /// Present when `groups` includes [`SettingsGroup::LocalAdjustments`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_adjustments: Option<Vec<LocalAdjustment>>,
}

impl PresetSettings {
    /// Captures the fields of `groups` from a complete develop state
    /// (`docs/presets.md` §3.1 — the create-a-preset step).
    pub fn capture(settings: &Settings, groups: &[SettingsGroup]) -> PresetSettings {
        let mut preset = PresetSettings {
            schema: settings.schema,
            groups: groups.to_vec(),
            ..PresetSettings::default()
        };
        for group in groups {
            match group {
                SettingsGroup::WhiteBalance => {
                    preset.white_balance = Some(settings.white_balance.clone());
                }
                SettingsGroup::Tone => {
                    preset.exposure = Some(settings.exposure);
                    preset.contrast = Some(settings.contrast);
                    preset.highlights = Some(settings.highlights);
                    preset.shadows = Some(settings.shadows);
                    preset.whites = Some(settings.whites);
                    preset.blacks = Some(settings.blacks);
                }
                SettingsGroup::Presence => {
                    preset.clarity = Some(settings.clarity);
                    preset.texture = Some(settings.texture);
                    preset.dehaze = Some(settings.dehaze);
                    preset.vibrance = Some(settings.vibrance);
                    preset.saturation = Some(settings.saturation);
                    preset.monochrome = Some(settings.monochrome);
                }
                SettingsGroup::ToneCurve => {
                    // Both curves (ADR 0137 §5): they are one block of the
                    // panel and one idea — *the tone curve of this
                    // photograph* — and a photographer copying it means both.
                    preset.tone_curve = Some(settings.tone_curve.clone());
                    preset.parametric_curve = Some(settings.parametric_curve);
                    preset.levels = Some(settings.levels);
                }
                SettingsGroup::ColorMixer => {
                    preset.hsl = Some(settings.hsl);
                }
                SettingsGroup::ColorGrading => {
                    preset.color_grading = Some(settings.color_grading);
                }
                SettingsGroup::CameraProfile => {
                    preset.camera_profile = Some(settings.camera_profile.clone());
                }
                SettingsGroup::CreativeLut => {
                    preset.lut = Some(settings.lut.clone());
                }
                SettingsGroup::Rendering => {
                    preset.highlight_reconstruction = Some(settings.highlight_reconstruction);
                    preset.demosaic = Some(settings.demosaic);
                    preset.output_rendering = Some(settings.output_rendering.clone());
                }
                SettingsGroup::Reshape => {
                    preset.reshape = Some(settings.reshape.clone());
                }
                SettingsGroup::SpotRemoval => {
                    preset.spot_removal = Some(settings.spot_removal.clone());
                }
                SettingsGroup::RedEye => {
                    preset.red_eye = Some(settings.red_eye.clone());
                }
                SettingsGroup::LocalAdjustments => {
                    preset.local_adjustments = Some(settings.local_adjustments.clone());
                }
                SettingsGroup::Effects => {
                    preset.vignette = Some(settings.vignette);
                    preset.grain = Some(settings.grain);
                }
                SettingsGroup::LensCorrection => {
                    preset.lens_correction = Some(settings.lens_correction.clone());
                    preset.defringe = Some(settings.defringe);
                }
                SettingsGroup::Detail => {
                    preset.noise_reduction = Some(settings.noise_reduction.clone());
                    preset.moire = Some(settings.moire);
                    preset.sharpening = Some(settings.sharpening.clone());
                }
                SettingsGroup::Geometry => {
                    preset.rotation = Some(settings.rotation);
                    preset.crop = Some(settings.crop.clone());
                    preset.perspective = Some(settings.perspective);
                }
            }
        }
        preset
    }

    /// Parses a `preset_json` document.
    pub fn parse(json: &str) -> Result<PresetSettings> {
        serde_json::from_str(json).map_err(|e| LeylineError::InvalidSettings(e.to_string()))
    }

    /// Serializes back to a `preset_json` document.
    #[allow(
        clippy::expect_used,
        reason = "serde_json only fails on a failing Serialize impl or a non-string map key, and a preset has neither"
    )]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("preset serialization cannot fail")
    }
}
