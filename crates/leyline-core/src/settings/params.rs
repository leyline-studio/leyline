//! The parameters of each develop stage, as `settings_json` stores them
//! (`docs/pipeline.md` §3.2). [`Settings`](super::Settings) holds one of each.

use super::*;

/// White balance override, in physical units.
///
/// Neutral state is the *absence* of an override (`None` in [`Settings`]):
/// the as-shot white balance of the camera is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WhiteBalance {
    /// Color temperature in Kelvin.
    pub temperature: u32,
    /// Green–magenta tint, unitless slider in [-100, +100], 0 = neutral.
    pub tint: i32,
}

impl Default for WhiteBalance {
    fn default() -> Self {
        Self {
            temperature: 6500,
            tint: 0,
        }
    }
}

/// One fixed white-balance preset (ADR 0091 §4).
///
/// `name` is a stable machine key — clients translate their own labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhiteBalancePreset {
    /// Stable identifier, lowercase, never shown untranslated.
    pub name: &'static str,
    /// Color temperature in Kelvin.
    pub temperature: u32,
    /// Green–magenta tint, same unit as [`WhiteBalance::tint`].
    pub tint: i32,
}

/// The conventional fixed white balances (ADR 0091 §4).
///
/// The slider being a correction dial referenced at 6500 K — the decode
/// already applied the as-shot multipliers — these values are exact only
/// for a shot the camera balanced to daylight; on anything else they are a
/// stated approximation, not a reading of the scene. "As shot" is not in
/// the table: it is the *absence* of an override (`None` in [`Settings`]).
pub const WHITE_BALANCE_PRESETS: &[WhiteBalancePreset] = &[
    WhiteBalancePreset {
        name: "daylight",
        temperature: 5500,
        tint: 10,
    },
    WhiteBalancePreset {
        name: "cloudy",
        temperature: 6500,
        tint: 10,
    },
    WhiteBalancePreset {
        name: "shade",
        temperature: 7500,
        tint: 10,
    },
    WhiteBalancePreset {
        name: "tungsten",
        temperature: 2850,
        tint: 0,
    },
    WhiteBalancePreset {
        name: "fluorescent",
        temperature: 3800,
        tint: 21,
    },
    WhiteBalancePreset {
        name: "flash",
        temperature: 5500,
        tint: 0,
    },
];

/// Lens correction step. Neutral: disabled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LensCorrection {
    /// Whether the Lensfun-backed correction (distortion, vignetting, TCA)
    /// is applied at all.
    pub enabled: bool,
    /// Profile selection; `"auto"` matches the lens from metadata.
    pub profile: String,
    /// Manual transverse chromatic aberration, red channel: the radial
    /// magnification error as a **percent of the radius**, in `[-1, 1]`
    /// (ADR 0111 §1). The output pixel at radius `r` reads red at
    /// `r × (1 + tca_red / 100)`.
    ///
    /// A percentage rather than a pixel displacement because one number is
    /// stored and applied at every resolution: a proxy, a preview and a
    /// full export correct the same aberration only if the unit is
    /// dimensionless.
    ///
    /// Measured by `Library::estimate_tca`, never by the render itself —
    /// and requires `lens` at version 2, which [`Settings::validate`]
    /// enforces rather than dropping the value in silence.
    #[serde(default)]
    pub tca_red: f64,
    /// Manual transverse chromatic aberration, blue channel. Same unit,
    /// range and rules as [`LensCorrection::tca_red`].
    #[serde(default)]
    pub tca_blue: f64,
}

impl Default for LensCorrection {
    fn default() -> Self {
        Self {
            enabled: false,
            profile: "auto".to_owned(),
            tca_red: 0.0,
            tca_blue: 0.0,
        }
    }
}

impl LensCorrection {
    /// Whether either manual coefficient asks for a resampling (ADR 0111 §5).
    ///
    /// The stage runs for these alone, without `enabled`: a lens Lensfun has
    /// never heard of is exactly the case this correction exists for, and
    /// switching on a distortion correction that has no data would be a
    /// strange price to pay for it.
    pub fn has_manual_tca(&self) -> bool {
        self.tca_red != 0.0 || self.tca_blue != 0.0
    }
}

/// A camera profile (DCP) reference (ADR 0035): a user-supplied `.dcp`
/// file, matched by an **explicit stored path** (never EXIF auto-match,
/// unlike [`LensCorrection::profile`]'s `"auto"` — a DCP is one file the
/// user made for their own camera body, not a community database to
/// fuzzy-match against). Neutral: absent — [`Settings::camera_profile`] is
/// `None`, LibRaw's own built-in sRGB conversion applies unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraProfile {
    /// Whether the profile is applied. Kept distinct from the field being
    /// absent so a reference can be turned off without losing it, the same
    /// UX [`LensCorrection::enabled`] gives.
    pub enabled: bool,
    /// Library-relative path to the `.dcp` file (`docs/catalog.md` §2.3),
    /// conventionally under `Profiles/Camera/`.
    pub path: String,
    /// BLAKE3 checksum of the `.dcp` file's bytes at the time this
    /// revision was written, `"blake3:<hex>"` (ADR 0006's algorithm,
    /// applied to a new kind of referenced input rather than a photo
    /// asset). A mismatch at render time means the file changed since —
    /// [`leyline_core::LeylineError::CameraProfileFailed`], never a
    /// silent re-render with different colors.
    pub checksum: String,
}

/// A creative LUT reference (ADR 0053): a user-supplied `.cube` file, imported
/// into the library and referenced by relative path plus checksum — exactly the
/// shape [`CameraProfile`] established, and for the same reasons (a portable
/// library, a detectable substitution).
///
/// Neutral: absent. A LUT is a *look* chosen elsewhere, not a setting with a
/// zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lut {
    /// Whether the LUT is applied — distinct from the field being absent, so a
    /// look can be turned off without losing which one it was.
    pub enabled: bool,
    /// Library-relative path to the `.cube` file (`docs/catalog.md` §2.3),
    /// conventionally under `Profiles/LUT/`.
    pub path: String,
    /// BLAKE3 checksum of the file's bytes when this revision was written,
    /// `"blake3:<hex>"`. A mismatch at render time is an error, never a silent
    /// render through a different look.
    pub checksum: String,
    /// How much of the look to apply, slider in [0, 100]. 100 is the LUT as
    /// its author wrote it; a film simulation is very often better below that,
    /// which is why the dose is part of the setting (ADR 0053 §2).
    pub strength: i32,
}

/// Noise reduction strengths, unitless sliders in [0, 100]. Neutral: 0.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseReduction {
    /// Luminance noise reduction strength.
    pub luminance: i32,
    /// Color noise reduction strength.
    pub color: i32,
}

/// How the working buffer becomes a display signal (ADR 0044 §3).
///
/// What becomes of a channel that saturated at the sensor (ADR 0050).
///
/// Decided *before* demosaic, where a clipped pixel still has unclipped
/// neighbors in the other channels — which is why this is a decoder
/// configuration pinned by the `input` stage version, not an operator of its
/// own. `Clip` is the neutral value: it changes nothing, and it is what every
/// revision written before ADR 0050 renders through.
///
/// Requires `input` at version 2 or later. A non-neutral value on a revision
/// pinned at `input: 1` is refused by [`Settings::validate`] rather than
/// silently dropped (ADR 0050 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightReconstruction {
    /// Clip at white: nothing is recovered.
    #[default]
    Clip,
    /// Blend the clipped and unclipped channels: recovers texture without
    /// drifting in color.
    Blend,
    /// Rebuild the saturated channel from the others: recovers the most, at
    /// the risk of a hue shift in deeply saturated areas.
    Rebuild,
}

impl HighlightReconstruction {
    /// Whether this is the neutral value — the predicate that keeps it out of
    /// a stored `settings_json` when nothing was asked for.
    pub fn is_clip(&self) -> bool {
        *self == HighlightReconstruction::Clip
    }
}

/// What the samples handed to the `input` stage already are (ADR 0107 §6).
///
/// Every source this program decodes itself — a RAW through LibRaw, a JPEG,
/// a PNG, an ordinary TIFF — arrives in a space `input` still has to convert:
/// that is [`SourceEncoding::Srgb`], the neutral value and the only one
/// versions 1 to 4 of the stage knew about.
///
/// [`SourceEncoding::LinearWorkspace`] says the file is **already** linear
/// Rec. 2020 with white at 1.0, so `input` has nothing to convert. It is
/// written by one thing only — the derivation of ADR 0107, whose exchange
/// file is the develop buffer as it stood before rank 20 — and it is what
/// lets a derived asset carry its parent's development instead of starting
/// from neutral.
///
/// Requires `input` at version 5 or later, and refused by
/// [`Settings::validate`] below it: the same capability rule ADR 0048 §5 set
/// and ADR 0050, ADR 0061, ADR 0096 and ADR 0098 applied since.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceEncoding {
    /// The decoder's own space: `input` decodes the transfer function and
    /// rotates the primaries into the working space.
    #[default]
    Srgb,
    /// Linear Rec. 2020, white at 1.0 — the working space itself. `input`
    /// passes the buffer through untouched.
    LinearWorkspace,
}

impl SourceEncoding {
    /// Whether this is the neutral value — the predicate that keeps it out of
    /// a stored `settings_json` when nothing was asked for.
    pub fn is_srgb(&self) -> bool {
        *self == SourceEncoding::Srgb
    }
}

/// Which interpolation reconstructs the two missing channels of every sensor
/// site (ADR 0061) — the very first rendering decision, taken by the decoder.
///
/// Requires `input` at version 3 or later. A non-neutral value on a revision
/// pinned at an earlier `input` is refused by [`Settings::validate`] rather
/// than silently dropped, the same capability rule ADR 0050 set.
///
/// Has **no effect on `Thumbnail` and `Small` previews**: those decode at half
/// size, which skips interpolation altogether.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Demosaic {
    /// Adaptive Homogeneity-Directed: good everywhere, best nowhere.
    #[default]
    Ahd,
    /// Variable Number of Gradients: gentler on gradients, less maze
    /// artifacting on flat areas.
    Vng,
    /// DCB: cleaner hard edges — the one to reach for when moire is the
    /// problem.
    Dcb,
    /// DHT: the finest on high-frequency detail, and the slowest.
    Dht,
}

impl Demosaic {
    /// Whether this is the neutral value — the predicate that keeps it out of
    /// a stored `settings_json` when nothing was asked for.
    pub fn is_ahd(&self) -> bool {
        *self == Demosaic::Ahd
    }
}

/// The pipeline carries highlights above white — a window brighter than the
/// wall beside it — all the way to the end. This says what becomes of them
/// when the image has to fit on a screen or in a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputRendering {
    /// How far the highlight shoulder reaches, slider in [0, 100].
    ///
    /// `0` cuts the headroom off at white. Higher values start the shoulder
    /// lower and bend more of it into the last stretch below white, so a
    /// bright sky comes back as a bright sky instead of a white shape.
    ///
    /// Unlike every other slider here, its neutral value is not 0: there is
    /// no "do nothing" for this stage — the buffer has to reach the display
    /// somehow — so the default is a rendering choice, frozen with the
    /// stage version that reads it.
    pub highlight_rolloff: i32,
}

impl Default for OutputRendering {
    fn default() -> Self {
        Self {
            highlight_rolloff: 50,
        }
    }
}

/// Defringe (ADR 0113): the coloured halo axial chromatic aberration and
/// blooming leave beside a high-contrast edge — purple in front of the focal
/// plane, green behind it.
///
/// Neutral: both amounts 0, and the stage does not run. Two amounts and no
/// hue ranges: the bands are fixed (magenta-violet around 285°, green around
/// 120°), and moving them would be a new version of the stage, never a third
/// slider (ADR 0113 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Defringe {
    /// How much saturation is taken out of the purple band at an edge,
    /// unitless slider in [0, 100].
    pub purple: i32,
    /// The same for the green band.
    pub green: i32,
}

impl Defringe {
    /// Whether this defringe asks for anything (ADR 0113 §1).
    pub fn is_neutral(&self) -> bool {
        self.purple == 0 && self.green == 0
    }
}

/// Sharpening step. Neutral: amount 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sharpening {
    /// Strength, unitless slider in [0, 100], 0 = no sharpening.
    pub amount: i32,
    /// Radius in pixels, strictly positive.
    pub radius: f64,
    /// Edge mask, unitless slider in [0, 100] (ADR 0096 §2). 0 sharpens
    /// every pixel — an unsharp mask amplifies noise and skin as readily as
    /// eyelashes; raising it confines the effect to what actually has an
    /// edge.
    ///
    /// Requires `sharpen` at version 2 or later: a revision pinned at v1
    /// cannot express it, and [`Settings::validate`] refuses the
    /// combination rather than let the slider do nothing (ADR 0096 §3).
    #[serde(default)]
    pub masking: i32,
}

impl Default for Sharpening {
    fn default() -> Self {
        Self {
            amount: 0,
            radius: 1.0,
            masking: 0,
        }
    }
}

/// The vignette a photographer *wants* (ADR 0090 §2) — not the one the lens
/// made, which [`LensCorrection`] removes at the other end of the pipeline.
///
/// Drawn **after the crop**, so it is centred on the composed frame and
/// follows a re-framing: that rank is the whole of ADR 0090 §1. Neutral:
/// `amount` at 0.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Vignette {
    /// Strength, slider in [-100, 100]. Negative darkens the corners,
    /// positive brightens them — Lightroom's sign. 0 = no vignette, and the
    /// stage does not run.
    pub amount: i32,
    /// Where the falloff starts, in [0, 100]: 0 at the frame's centre, 100
    /// at its corners.
    pub midpoint: i32,
    /// Shape, in [-100, 100]: 0 is an ellipse fitted to the frame, positive
    /// squares it off, negative makes it pointier.
    pub roundness: i32,
    /// Width of the transition, in [0, 100]. 0 is a hard edge.
    pub feather: i32,
}

impl Default for Vignette {
    fn default() -> Self {
        Self {
            amount: 0,
            midpoint: 50,
            roundness: 0,
            feather: 50,
        }
    }
}

impl Vignette {
    /// Whether this vignette is away from its neutral value.
    ///
    /// Only `amount` decides: the other three describe a shape, and a shape
    /// at zero strength is not a rendering (ADR 0090 §2).
    pub fn is_neutral(&self) -> bool {
        self.amount == 0
    }
}

/// Film grain (ADR 0090 §3): value noise recomputed from the pixel's own
/// coordinates, so it is deterministic without storing a seed. Neutral:
/// `amount` at 0.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grain {
    /// Strength, slider in [0, 100]. 0 = no grain, and the stage does not
    /// run.
    pub amount: i32,
    /// Lattice spacing, slider in [0, 100], mapped to 1..16
    /// **full-resolution** pixels — so a preview samples the same field as
    /// the export rather than a coarser one of its own (ADR 0090 §3).
    pub size: i32,
    /// Weight of a second octave at half the spacing, in [0, 100].
    pub roughness: i32,
    /// How much the three layers disagree, in [0, 100]. 0 is one grey on
    /// all three channels — exactly what ADR 0090 shipped; 100 is three
    /// emulsion layers graining independently (ADR 0118 §2).
    ///
    /// The grain's *grey* is as loud at 100 as at 0: what this adds sums to
    /// zero across the channels, so it carries chroma and no luminance.
    ///
    /// Requires `grain` at version 2, and [`Settings::validate`] refuses it
    /// on an earlier one rather than let a slider do nothing (ADR 0118 §4).
    pub color: i32,
}

impl Default for Grain {
    fn default() -> Self {
        Self {
            amount: 0,
            size: 25,
            roughness: 50,
            color: 0,
        }
    }
}

impl Grain {
    /// Whether this grain is away from its neutral value — `amount` alone,
    /// for the same reason as [`Vignette::is_neutral`].
    pub fn is_neutral(&self) -> bool {
        self.amount == 0
    }
}

/// One control point of a [`ToneCurve`], normalized coordinates in [0, 1].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    /// Input level, in [0, 1].
    pub x: f64,
    /// Output level, in [0, 1].
    pub y: f64,
}

/// Parametric tone curve (ADR 0137): four regions lifted by their own
/// slider, and three splits saying where each region begins.
///
/// The instrument one reaches for without knowing where on the axis a tone
/// lives — *lift the darks a little*, and then adjust what counts as dark.
/// Applied before [`ToneCurve`], the precise one, which therefore has the
/// last word (ADR 0137 §2).
///
/// Neutral: every region at 0, splits at 25/50/75 — in which case the stage
/// does not run at all.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParametricCurve {
    /// Lifts the darkest region, in [-100, 100].
    pub shadows: i32,
    /// Lifts the region between the first and second splits, in [-100, 100].
    pub darks: i32,
    /// Lifts the region between the second and third splits, in [-100, 100].
    pub lights: i32,
    /// Lifts the brightest region, in [-100, 100].
    pub highlights: i32,
    /// Where the shadows region ends, in percent of the tonal axis [1, 99].
    pub shadow_split: i32,
    /// Where the darks region ends, in [1, 99].
    pub midtone_split: i32,
    /// Where the lights region ends, in [1, 99].
    pub highlight_split: i32,
}

impl Default for ParametricCurve {
    fn default() -> Self {
        Self {
            shadows: 0,
            darks: 0,
            lights: 0,
            highlights: 0,
            shadow_split: 25,
            midtone_split: 50,
            highlight_split: 75,
        }
    }
}

impl ParametricCurve {
    /// Whether any region is lifted — i.e. whether the **stage changes a
    /// pixel**. The splits alone change nothing: they say where regions are,
    /// and a region at 0 lifts nothing wherever it begins.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.shadows == 0 && self.darks == 0 && self.lights == 0 && self.highlights == 0
    }

    /// Whether the whole control is where it started — which is a different
    /// question from [`Self::is_neutral`], and the one the **document** asks.
    ///
    /// Found by dragging a split on the running binary: with
    /// `skip_serializing_if` asking `is_neutral`, moving a marker while every
    /// region sat at 0 was stored nowhere, so the marker sprang back on the
    /// next open. A split is a decision even when it changes no pixel yet.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// `skip_serializing_if` for an integer setting added after the first
/// schema: a revision that never moved it stores nothing, so its document is
/// the one it always was.
#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes a reference
pub(crate) fn is_zero_i32(value: &i32) -> bool {
    *value == 0
}

/// Levels (ADR 0164): black, gamma and white points, and output limits, on
/// the 0–255 scale the numbers are known in — only a scale: nothing is
/// quantised to 8 bits. Applied at rank 78, between the parametric curve and
/// the point curve, identically on every channel.
///
/// Neutral: `0, 255, 1.0, 0, 255`, in which case the stage does not run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Levels {
    /// The input level mapped to `output_black`, in [0, 254].
    pub input_black: i32,
    /// The input level mapped to `output_white`, in [1, 255], above
    /// `input_black`.
    pub input_white: i32,
    /// Midtone gamma in [0.10, 9.99]; above 1 brightens (Photoshop's
    /// convention).
    pub gamma: f64,
    /// The darkest output level, in [0, 255].
    pub output_black: i32,
    /// The brightest output level, in [0, 255], not below `output_black`:
    /// an inverted output is a negative, not a level (ADR 0164 §3).
    pub output_white: i32,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            input_black: 0,
            input_white: 255,
            gamma: 1.0,
            output_black: 0,
            output_white: 255,
        }
    }
}

impl Levels {
    /// Whether the stage would change no pixel — and then it does not run.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        *self == Self::default()
    }
}

/// Tone curve step (ADR 0030): a point curve applied in luminance, i.e. the
/// same curve to every channel of the working buffer. Neutral: no points,
/// in which case the stage does not run at all.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToneCurve {
    /// Control points, ordered by strictly increasing `x`. Empty = identity.
    ///
    /// The *master* curve: applied to every channel, and applied before the
    /// three below (ADR 0098 §2).
    pub points: Vec<CurvePoint>,
    /// Red channel's own curve, applied after [`ToneCurve::points`]
    /// (ADR 0098). Empty = identity, the same convention as the master.
    ///
    /// Requires `tone_curve` at version 2 or later: a revision pinned at v1
    /// cannot express it, and [`Settings::validate`] refuses the
    /// combination rather than let the curve be silently dropped
    /// (ADR 0098 §3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub red: Vec<CurvePoint>,
    /// Green channel's own curve. See [`ToneCurve::red`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub green: Vec<CurvePoint>,
    /// Blue channel's own curve. See [`ToneCurve::red`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blue: Vec<CurvePoint>,
}

impl ToneCurve {
    /// Whether any per-channel curve is set — what decides between the two
    /// code paths of `tone_curve::v2` (ADR 0098 §1).
    #[must_use]
    pub fn has_channel_curves(&self) -> bool {
        !self.red.is_empty() || !self.green.is_empty() || !self.blue.is_empty()
    }
}

/// One band of the 8-band HSL mixer (ADR 0031): hue/saturation/luminance
/// offsets, sliders in [-100, 100]. Neutral: all zero. The band a pixel
/// belongs to is decided by its own hue at render time (with falloff
/// between adjacent bands) — this struct is just the three sliders for one
/// band, not a selector.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HslBand {
    /// Hue shift for pixels in this band.
    pub hue: i32,
    /// Saturation offset for pixels in this band.
    pub saturation: i32,
    /// Luminance offset for pixels in this band.
    pub luminance: i32,
}

/// One zone of [`ColorGrading`] (ADR 0031): the color (hue + saturation)
/// blended into pixels weighted by luminance zone membership, plus a
/// luminance offset. Neutral: all zero (no coloring, no offset).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorGradingZone {
    /// Hue of the color mixed into this zone, degrees in [0, 360).
    pub hue: i32,
    /// Saturation (strength) of the color mixed into this zone, in [0, 100].
    pub saturation: i32,
    /// Luminance offset for this zone, in [-100, 100].
    pub luminance: i32,
}

/// Shadows/midtones/highlights color grading (ADR 0031): a luminance-
/// weighted fondu between three tonal zones, each with its own color —
/// Lightroom's Color Grading panel / Darktable's color balance rgb.
///
/// **Not a spatial mask.** This weights every pixel by its own luminance
/// value, wherever it sits in the frame — orthogonal to
/// [`LocalAdjustment`]'s spatial coverage (ADR 0029). The two never share
/// infrastructure; combining them (regional color grading) is deferred to
/// a future ADR. Neutral: every zone at its default and `balance`/
/// `blending` at zero.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorGrading {
    /// Low-luminance zone.
    pub shadows: ColorGradingZone,
    /// Mid-luminance zone.
    pub midtones: ColorGradingZone,
    /// High-luminance zone.
    pub highlights: ColorGradingZone,
    /// Shifts the shadows/highlights zone boundary, in [-100, 100]. 0 =
    /// centered.
    pub balance: i32,
    /// Width of the smoothstep blend between adjacent zones, in [0, 100].
    /// Meaningless (and harmless) when every zone is neutral.
    pub blending: i32,
}

/// A point in the local-correction coordinate frame (ADR 0026): normalized
/// `[0, 1]`, relative to the image *after* rotation, *before* crop — the
/// same referential as [`Crop`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Horizontal position, in [0, 1].
    pub x: f64,
    /// Vertical position, in [0, 1].
    pub y: f64,
}

/// One spot removal clone (ADR 0032): copies the disk around `source` onto
/// the disk around `target`, feathered at the edge and modulated by
/// `opacity`. Clone only — no *heal* mode in V2.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpotRemoval {
    /// Center of the disk to paint into.
    pub target: Point,
    /// Center of the disk to copy from.
    pub source: Point,
    /// Radius of the copied disk, normalized coordinates. Strictly positive.
    pub radius: f64,
    /// Radial falloff at the disk's edge, in [0, 1]: 0 = hard edge, 1 = the
    /// feather spans the whole disk.
    pub feather: f64,
    /// Overall blend strength, in [0, 1]. Neutral value for a spot would be
    /// 1.0, but there is no neutral *entry* — an empty list is neutral.
    pub opacity: f64,
}

/// One reshape handle (ADR 0109): content grabbed at [`ReshapePoint::from`]
/// and dropped at [`ReshapePoint::to`], with the pixels around it stretched
/// to follow.
///
/// Nothing is invented — every output pixel comes from an input pixel, which
/// is why this needs no model. An empty [`Settings::reshape`] is the neutral
/// value and there is no neutral entry, the convention [`SpotRemoval`],
/// [`RedEye`] and [`LocalAdjustment`] already share.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReshapePoint {
    /// The content to move: the point the user grabbed. Normalized,
    /// post-rotation, pre-crop (ADR 0026).
    pub from: Point,
    /// Where it is to appear: the point they dropped it at.
    pub to: Point,
    /// Radius of influence around `to`, normalized against the buffer's
    /// larger dimension. Strictly positive.
    pub radius: f64,
    /// How much of the displacement to apply, in `[0, 1]` — the dose
    /// [`SpotRemoval::opacity`] and [`LocalAdjustment::opacity`] already
    /// have, per handle rather than per stage so one can be dialled back
    /// without touching its neighbours.
    pub strength: f64,
}

/// One red-eye correction (ADR 0103): a disk placed over a pupil, inside
/// which red is desaturated and darkened *in proportion to how red each
/// pixel is*.
///
/// Same post-rotation, pre-crop referential as [`SpotRemoval`] (ADR 0026),
/// and the same "an empty list is the neutral" convention — there is no
/// neutral entry.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RedEye {
    /// Center of the disk covering the pupil.
    pub center: Point,
    /// Radius, normalized against the buffer's larger dimension. Strictly
    /// positive.
    pub radius: f64,
    /// Radial falloff at the disk's edge, in [0, 1], like
    /// [`SpotRemoval::feather`].
    pub feather: f64,
    /// How much the corrected pixels are darkened, in [0, 1]: 0 removes the
    /// cast and leaves the brightness, 1 takes the pupil to black. A pupil
    /// is not merely grey, so the useful values are not near 0.
    pub darken: f64,
}

impl Default for RedEye {
    fn default() -> Self {
        Self {
            center: Point { x: 0.5, y: 0.5 },
            radius: 0.02,
            feather: 0.5,
            darken: 0.6,
        }
    }
}

/// Extending a backdrop (ADR 0172): replace what lies beside a too-narrow
/// backdrop — stands, wall, radiator — by a continuation of the backdrop
/// itself, measured on it.
///
/// An empty [`Backdrop::regions`] is the neutral value: the stage does not
/// run. Every polygon is in the post-rotation, pre-crop referential of
/// ADR 0026, like [`RedEye`] and [`SpotRemoval`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Backdrop {
    /// What to extend and where, in order: where two zones overlap, the
    /// later region wins (§2).
    pub regions: Vec<BackdropRegion>,
    /// Masks whose union is never replaced — the subjects. Usually a stored
    /// coverage written by a detector (ADR 0073), run by a gesture, never by
    /// default.
    pub protect: Vec<BackdropProtect>,
    /// Masks subtracted from the protected union: the piece of a stand a
    /// detector stuck to a hand.
    pub release: Vec<Mask>,
    /// Width of the blend between the fill and what it meets, as a fraction
    /// of the buffer's larger dimension, in `[0, 0.05]`.
    pub feather: f64,
    /// The synthetic mottle's amplitude, in percent of the one measured on
    /// the source, in `[0, 200]`. 0 leaves only the slow field.
    pub texture: i32,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
            protect: Vec::new(),
            release: Vec::new(),
            feather: 0.003,
            texture: 100,
        }
    }
}

impl Backdrop {
    /// No region: the stage does not run, whatever the other fields say.
    pub fn is_neutral(&self) -> bool {
        self.regions.is_empty()
    }
}

/// One region of a [`Backdrop`]: the polygon that is measured, and the zone
/// that is replaced by what was measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackdropRegion {
    /// The backdrop as traced by the photographer: at least three vertices,
    /// each in `[0, 1]`.
    pub source: Vec<Point>,
    /// What is replaced.
    #[serde(default)]
    pub zone: BackdropZone,
    /// Polygons *inside* the source that are replaced too — a logo printed
    /// on a mat — and never measured.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<Point>>,
}

/// Where a [`BackdropRegion`] writes.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackdropZone {
    /// The whole frame minus the source polygon: the common case.
    #[default]
    OutsideSource,
    /// Only these polygons.
    Polygons(Vec<Vec<Point>>),
}

/// One protected mask of a [`Backdrop`], and where its protection holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackdropProtect {
    /// The shape, any [`Mask`] a local adjustment may carry.
    pub mask: Mask,
    /// Where the protection applies.
    #[serde(default)]
    pub within: ProtectWithin,
}

/// Where a [`BackdropProtect`] holds — what lets a "main subject" coverage
/// and a "people" coverage be combined so that a stand outside the backdrop
/// is never kept (ADR 0172 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectWithin {
    /// Everywhere in the frame.
    #[default]
    Everywhere,
    /// Only inside the regions' source polygons.
    Source,
    /// Only outside them.
    OutsideSource,
}

/// One point of a [`Mask::Brush`] stroke (ADR 0029): a single dab in the
/// brush's path, in the same post-rotation, pre-crop referential as
/// [`Point`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BrushStroke {
    /// Horizontal position, in [0, 1].
    pub x: f64,
    /// Vertical position, in [0, 1].
    pub y: f64,
    /// Dab radius, normalized against the image's larger dimension. Strictly
    /// positive.
    pub radius: f64,
    /// Opacity build-up per dab, in [0, 1].
    pub flow: f64,
    /// Fraction of the radius with full coverage before the edge feathers
    /// out, in [0, 1]: 0 = soft throughout, 1 = a hard disk.
    pub hardness: f64,
}

/// Mask geometry of one [`LocalAdjustment`] (ADR 0029): a spatial coverage
/// `[0, 1]` per pixel, in the same post-rotation, pre-crop referential as
/// [`Crop`]/[`SpotRemoval`] (ADR 0026). Exactly three kinds are in scope for
/// V2: a brush's traced strokes, a radial (elliptical) filter, and a linear
/// gradient — Lightroom's "graduated filter" and "linear gradient" are the
/// same mechanism under two names, so there is no separate fourth type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Mask {
    /// A feathered ellipse.
    Radial {
        /// Center, horizontal.
        cx: f64,
        /// Center, vertical.
        cy: f64,
        /// Horizontal radius, in [0, 1] of the image width.
        rx: f64,
        /// Vertical radius, in [0, 1] of the image height.
        ry: f64,
        /// Clockwise rotation of the ellipse, in degrees.
        angle: f64,
        /// Radial falloff at the rim, in [0, 1]: 0 = hard edge, 1 = the
        /// feather spans the whole ellipse.
        feather: f64,
        /// `true` applies the adjustment *outside* the ellipse instead of
        /// inside it.
        inverted: bool,
    },
    /// A linear gradient: full coverage at `(x0, y0)`, easing to none at
    /// `(x1, y1)`, constant along lines perpendicular to that axis.
    Gradient {
        /// Full-coverage end, horizontal.
        x0: f64,
        /// Full-coverage end, vertical.
        y0: f64,
        /// Zero-coverage end, horizontal.
        x1: f64,
        /// Zero-coverage end, vertical.
        y1: f64,
    },
    /// A traced brush path: the union of every dab's soft-edged disk.
    Brush {
        /// Ordered dabs, one per recorded point of the stroke.
        strokes: Vec<BrushStroke>,
    },
    /// Full coverage everywhere — the geometry of "no geometry" (ADR 0048
    /// §1), so a range mask can stand on its own instead of having to be
    /// hung off a deliberately oversized radial.
    Everything,
    /// A coverage stored as a file rather than derived from a formula
    /// (ADR 0070): the one mask kind whose shape is not expressible in a
    /// handful of numbers — a subject, a sky, a selection painted
    /// elsewhere.
    ///
    /// Sampled bilinearly over the same normalized `[0, 1]²` canvas the
    /// four variants above are evaluated on, so it follows rotation and
    /// combines with a range and an opacity exactly like they do.
    Coverage {
        /// Library-relative path to the 16-bit grayscale PNG
        /// (`docs/catalog.md` §2.3), conventionally `Masks/<blake3>.png`.
        path: String,
        /// BLAKE3 checksum of the file's bytes when this revision was
        /// written, `"blake3:<hex>"` — the same fail-closed reference
        /// [`CameraProfile`] and [`Lut`] carry. A mismatch at render time
        /// is an error, never a silent render through a different mask.
        checksum: String,
    },
}

/// A range refinement of a [`LocalAdjustment`]'s geometric mask (ADR 0048):
/// the coverage is *multiplied* by these terms, so a range narrows a mask and
/// can never widen it.
///
/// Both terms are optional and independent. `RangeMask::default()` — neither
/// term — is a no-op, which is why the field on [`LocalAdjustment`] is an
/// `Option` rather than a struct with two `None`s standing for "off".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RangeMask {
    /// Restrict to a band of luminance; `None` = no luminance term.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub luminance: Option<LuminanceRange>,
    /// Restrict to a band of hue; `None` = no color term.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorRange>,
}

/// A band of luminance on the display axis (ADR 0048 §2–3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LuminanceRange {
    /// Lower edge of full coverage, in [0, 1].
    pub min: f64,
    /// Upper edge of full coverage, in [0, 1]; must be `>= min`.
    pub max: f64,
    /// Width of the smooth falloff outside each edge, in [0, 1]. 0 is a hard
    /// edge — visible as a contour as soon as noise makes a pixel cross the
    /// threshold, which is what this parameter exists to avoid.
    pub softness: f64,
}

impl Default for LuminanceRange {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 1.0,
            softness: 0.1,
        }
    }
}

/// A band of hue, in degrees (ADR 0048 §2). Hue is circular, so `center`
/// wraps and the distance to it is taken modulo 360.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorRange {
    /// Center of the band, in degrees.
    pub center: f64,
    /// Half-width of full coverage, in degrees, in (0, 180].
    pub width: f64,
    /// Width of the smooth falloff outside the band, in degrees.
    pub softness: f64,
}

impl Default for ColorRange {
    fn default() -> Self {
        Self {
            center: 0.0,
            width: 30.0,
            softness: 15.0,
        }
    }
}

/// The restricted subset of [`Settings`]' global tonal/color fields a
/// [`LocalAdjustment`] may re-parameterize (ADR 0029): exactly the fields
/// that have a spatially-restricted meaning. Lens correction, noise
/// reduction, sharpening and rotation/crop are deliberately out of scope —
/// see ADR 0029's *Alternatives écartées*. `None` in any field means "no
/// change from the global value" for that operator within the mask, unlike
/// [`Settings`] where the same fields are always-present sliders.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalAdjustmentValues {
    /// Same unit and meaning as [`WhiteBalance::temperature`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<u32>,
    /// Same unit and meaning as [`WhiteBalance::tint`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tint: Option<i32>,
    /// Same unit and meaning as [`Settings::exposure`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f64>,
    /// Same unit and meaning as [`Settings::contrast`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast: Option<i32>,
    /// Same unit and meaning as [`Settings::highlights`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlights: Option<i32>,
    /// Same unit and meaning as [`Settings::shadows`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadows: Option<i32>,
    /// Same unit and meaning as [`Settings::whites`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whites: Option<i32>,
    /// Same unit and meaning as [`Settings::blacks`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blacks: Option<i32>,
    /// Same unit and meaning as [`Settings::vibrance`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vibrance: Option<i32>,
    /// Same unit and meaning as [`Settings::saturation`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturation: Option<i32>,
    /// Same unit, meaning and radius constant as [`Settings::clarity`]
    /// (ADR 0108 §3).
    ///
    /// Requires `local_adjustments` at version 4 or later: the five
    /// neighbourhood values below are a capability of that version, and
    /// [`Settings::validate`] refuses them on an earlier one rather than
    /// let a slider do nothing (ADR 0108 §5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clarity: Option<i32>,
    /// Same unit, meaning and radius constant as [`Settings::texture`].
    /// Negative is the softening the develop module has no other way to
    /// express (ADR 0108 §1). Requires `local_adjustments` at version 4.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<i32>,
    /// Unsharp mask strength, slider in [-100, +100]. Below zero it
    /// subtracts its own detail, which softens.
    ///
    /// One number, not [`Sharpening`]'s three: the radius and the edge mask
    /// are bound to the stage version, never read from the revision's global
    /// sharpening (ADR 0108 §3). Requires `local_adjustments` at version 4.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharpness: Option<i32>,
    /// Same unit and meaning as [`NoiseReduction::luminance`], slider in
    /// [0, 100]. Runs the edge-preserving operator of rank 170, never the
    /// measured one of rank 5, whose thresholds are meaningless this late
    /// (ADR 0108 §3). Requires `local_adjustments` at version 4.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noise_luminance: Option<i32>,
    /// Same unit and meaning as [`NoiseReduction::color`], slider in
    /// [0, 100]. Requires `local_adjustments` at version 4.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noise_color: Option<i32>,
    /// Same unit, meaning and hue band as [`Defringe::purple`], slider in
    /// [0, 100] (ADR 0116 §1). Runs **first** among a local adjustment's
    /// operators, because its stage's rank is 22 — ahead of every other one
    /// a mask can carry (ADR 0116 §2).
    ///
    /// Requires `local_adjustments` at version 5, which
    /// [`Settings::validate`] enforces rather than dropping the value in
    /// silence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defringe_purple: Option<i32>,
    /// Same for the green band. Requires `local_adjustments` at version 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defringe_green: Option<i32>,
}

impl LocalAdjustmentValues {
    /// Whether this entry asks for any of the five neighbourhood operators
    /// ADR 0108 added — the ones `local_adjustments` versions 1 to 3 have no
    /// code for.
    #[must_use]
    pub fn uses_neighbourhood_operators(&self) -> bool {
        self.clarity.is_some()
            || self.texture.is_some()
            || self.sharpness.is_some()
            || self.noise_luminance.is_some()
            || self.noise_color.is_some()
    }

    /// Whether this adjustment asks for the defringe pair, which is a
    /// capability of `local_adjustments` **version 5** (ADR 0116 §4) — its
    /// own question, because it is its own version.
    pub fn uses_defringe(&self) -> bool {
        self.defringe_purple.is_some() || self.defringe_green.is_some()
    }
}

/// One masked, locally re-parameterized adjustment (ADR 0029): a mask plus
/// the subset of tonal/color values it applies, faded in by `opacity` and
/// the mask's own coverage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalAdjustment {
    /// The spatial coverage this adjustment applies through.
    pub mask: Mask,
    /// Optional luminance/color refinement of `mask`, multiplying its
    /// coverage (ADR 0048). `None` is the ADR 0029 behavior exactly.
    ///
    /// Requires `local_adjustments` at version 2 or later: a revision pinned
    /// at v1 cannot express it, and [`Settings::validate`] refuses the
    /// combination rather than let the setting be silently dropped
    /// (ADR 0048 §5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<RangeMask>,
    /// Overall blend strength, in [0, 1]. 1.0 is the neutral "fully applied"
    /// value for an *entry*; there is no neutral value for the list itself
    /// other than being empty.
    pub opacity: f64,
    /// The values re-parameterized within `mask`'s coverage.
    pub adjustments: LocalAdjustmentValues,
}

/// Perspective correction (ADR 0052): two sliders driving one projective
/// transform, applied after rotation and before crop.
///
/// Neutral is the absence of the whole struct, not a struct of zeros — the same
/// convention [`Crop`] follows.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Perspective {
    /// Vertical correction, slider in [-100, +100]: positive brings the top
    /// edge's corners together, which is what straightens a building shot from
    /// below.
    pub vertical: i32,
    /// Horizontal correction, slider in [-100, +100].
    pub horizontal: i32,
}

/// Crop rectangle in normalized [0, 1] coordinates, relative to the image
/// *after* rotation. Neutral state is the absence of a crop (`None`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    /// Left edge, in [0, 1].
    pub x: f64,
    /// Top edge, in [0, 1].
    pub y: f64,
    /// Width, in (0, 1].
    pub width: f64,
    /// Height, in (0, 1].
    pub height: f64,
}
