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

    /// Masked local adjustments, applied in list order. Neutral: empty (ADR
    /// 0029).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub local_adjustments: Vec<LocalAdjustment>,

    /// Lens correction step.
    pub lens_correction: LensCorrection,
    /// Noise reduction step.
    pub noise_reduction: NoiseReduction,
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
            tone_curve: ToneCurve::default(),
            hsl: [HslBand::default(); 8],
            color_grading: ColorGrading::default(),
            spot_removal: Vec::new(),
            reshape: Vec::new(),
            red_eye: Vec::new(),
            local_adjustments: Vec::new(),
            lens_correction: LensCorrection::default(),
            noise_reduction: NoiseReduction::default(),
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

        let json = serde_json::to_value(self).expect("settings serialization cannot fail");
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
                if let Some(previous_x) = previous_x {
                    if point.x <= previous_x {
                        return Err(LeylineError::InvalidSettings(format!(
                            "{field} must have strictly increasing x"
                        )));
                    }
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
            let unit = |name: &str, value: f64| -> Result<()> {
                if (0.0..=1.0).contains(&value) {
                    Ok(())
                } else {
                    Err(LeylineError::InvalidSettings(format!(
                        "local_adjustments[{i}].{name} must be in [0, 1], got {value}"
                    )))
                }
            };
            match &adjustment.mask {
                Mask::Radial {
                    cx,
                    cy,
                    rx,
                    ry,
                    angle,
                    feather,
                    inverted: _,
                } => {
                    unit("mask.cx", *cx)?;
                    unit("mask.cy", *cy)?;
                    if *rx <= 0.0 || *ry <= 0.0 {
                        return Err(LeylineError::InvalidSettings(format!(
                            "local_adjustments[{i}].mask.rx/ry must be strictly positive, got {rx}/{ry}"
                        )));
                    }
                    finite("mask.angle", *angle)?;
                    unit("mask.feather", *feather)?;
                }
                Mask::Gradient { x0, y0, x1, y1 } => {
                    unit("mask.x0", *x0)?;
                    unit("mask.y0", *y0)?;
                    unit("mask.x1", *x1)?;
                    unit("mask.y1", *y1)?;
                    if (x1 - x0).hypot(y1 - y0) <= 0.0 {
                        return Err(LeylineError::InvalidSettings(format!(
                            "local_adjustments[{i}].mask: gradient endpoints must differ"
                        )));
                    }
                }
                Mask::Everything => {}
                Mask::Brush { strokes } => {
                    if strokes.is_empty() {
                        return Err(LeylineError::InvalidSettings(format!(
                            "local_adjustments[{i}].mask.strokes must not be empty"
                        )));
                    }
                    for (j, stroke) in strokes.iter().enumerate() {
                        unit(&format!("mask.strokes[{j}].x"), stroke.x)?;
                        unit(&format!("mask.strokes[{j}].y"), stroke.y)?;
                        if stroke.radius <= 0.0 {
                            return Err(LeylineError::InvalidSettings(format!(
                                "local_adjustments[{i}].mask.strokes[{j}].radius must be strictly positive, got {}",
                                stroke.radius
                            )));
                        }
                        unit(&format!("mask.strokes[{j}].flow"), stroke.flow)?;
                        unit(&format!("mask.strokes[{j}].hardness"), stroke.hardness)?;
                    }
                }
                Mask::Coverage { path, checksum } => {
                    // Same capability rule as `range` below, one variant
                    // wider: v1 and v2 have no code that reads a stored
                    // coverage, and an ignored mask is a local adjustment
                    // applied to the whole image (ADR 0070 §4).
                    if matches!(self.stages.get("local_adjustments"), Some(&v) if v < 3) {
                        return Err(LeylineError::InvalidSettings(format!(
                            "local_adjustments[{i}].mask is a stored coverage, which needs \
                             stage local_adjustments version 3, but this revision pins an \
                             earlier one; reprocess the photo to the current stage versions \
                             first"
                        )));
                    }
                    crate::validate_library_relative_path(
                        &format!("local_adjustments[{i}].mask.path"),
                        path,
                    )?;
                    if !checksum.starts_with("blake3:") || checksum.len() != "blake3:".len() + 64 {
                        return Err(LeylineError::InvalidSettings(format!(
                            "local_adjustments[{i}].mask.checksum must be \"blake3:<64 hex>\", \
                             got {checksum:?}"
                        )));
                    }
                }
            }

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
            if let Some(temperature) = values.temperature {
                if temperature == 0 {
                    return Err(LeylineError::InvalidSettings(format!(
                        "local_adjustments[{i}].adjustments.temperature must be strictly positive"
                    )));
                }
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
    /// [`Settings::tone_curve`] (ADR 0098).
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
                    preset.tone_curve = Some(settings.tone_curve.clone());
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
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("preset serialization cannot fail")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact example of `docs/pipeline.md` §3.2.
    const SPEC_EXAMPLE: &str = r#"{
        "schema": 1,
        "stages": { "gains": 1, "contrast": 1, "crop": 1 },

        "white_balance": { "temperature": 5400, "tint": 4 },
        "exposure": 0.35,
        "contrast": 12,
        "highlights": -40,
        "shadows": 25,
        "whites": 0,
        "blacks": -5,
        "vibrance": 18,
        "saturation": 0,

        "lens_correction": { "enabled": true, "profile": "auto" },
        "noise_reduction": { "luminance": 15, "color": 25 },
        "sharpening": { "amount": 40, "radius": 1.0 },

        "rotation": 0.0,
        "crop": { "x": 0.1, "y": 0.2, "width": 0.8, "height": 0.7 }
    }"#;

    #[test]
    fn parses_the_spec_example() {
        let s = Settings::parse(SPEC_EXAMPLE).unwrap();
        assert_eq!(s.schema, 1);
        assert_eq!(s.stages["gains"], 1);
        assert_eq!(s.stages.len(), 3);
        assert_eq!(
            s.white_balance,
            Some(WhiteBalance {
                temperature: 5400,
                tint: 4
            })
        );
        assert_eq!(s.exposure, 0.35);
        assert_eq!(s.contrast, 12);
        assert_eq!(s.highlights, -40);
        assert_eq!(s.noise_reduction.color, 25);
        assert_eq!(s.sharpening.amount, 40);
        assert_eq!(
            s.crop,
            Some(Crop {
                x: 0.1,
                y: 0.2,
                width: 0.8,
                height: 0.7
            })
        );
        assert!(s.extra.is_empty());
        s.validate().unwrap();
    }

    #[test]
    fn round_trips_losslessly() {
        let parsed = Settings::parse(SPEC_EXAMPLE).unwrap();
        let reparsed = Settings::parse(&parsed.to_json()).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn omitted_values_are_neutral() {
        let s = Settings::parse(r#"{ "schema": 1, "stages": { "gains": 1 } }"#).unwrap();
        assert_eq!(
            s,
            Settings {
                schema: 1,
                stages: StageVersions::from([("gains".to_owned(), 1)]),
                ..Settings::default()
            }
        );
        assert_eq!(Settings::parse("{}").unwrap(), Settings::default());
    }

    #[test]
    fn a_document_still_carrying_process_is_refused() {
        // Not the same case as an unknown field from a newer schema: this
        // one is a *removed* field, so the document predates ADR 0043 and
        // rendering it would silently give it another engine's pixels.
        let s = Settings::parse(r#"{ "schema": 1, "process": 8, "exposure": 0.5 }"#).unwrap();
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn preserves_unknown_fields_verbatim() {
        // A placeholder name for a field this engine doesn't know yet —
        // picked to stay clear of any real `Settings` field, past or
        // future (`clarity` used to fill this role until ADR 0033 made it
        // a real one).
        let json = r#"{ "schema": 2, "vignette_style": 30, "exposure": 1.5 }"#;
        let s = Settings::parse(json).unwrap();
        assert_eq!(s.schema, 2);
        assert_eq!(s.extra.get("vignette_style"), Some(&serde_json::json!(30)));

        let round_tripped: serde_json::Value = serde_json::from_str(&s.to_json()).unwrap();
        assert_eq!(round_tripped["vignette_style"], serde_json::json!(30));
        assert_eq!(round_tripped["exposure"], serde_json::json!(1.5));
    }

    #[test]
    fn rejects_out_of_range_values() {
        let mut s = Settings {
            contrast: 150,
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));

        s.contrast = 0;
        s.crop = Some(Crop {
            x: 0.5,
            y: 0.0,
            width: 0.8,
            height: 1.0,
        });
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn absent_camera_profile_validates() {
        Settings::default().validate().unwrap();
    }

    #[test]
    fn camera_profile_rejects_an_empty_path() {
        let s = Settings {
            camera_profile: Some(CameraProfile {
                enabled: true,
                path: "  ".to_owned(),
                checksum: format!("blake3:{}", "a".repeat(64)),
            }),
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn camera_profile_rejects_a_path_that_escapes_the_library() {
        // `settings_json` is user-editable and the path is joined onto the
        // library root: traversal and absolute paths would turn a revision
        // into an arbitrary-file read (`validate_library_relative_path`).
        for path in [
            "../outside.dcp",
            "Profiles/../../outside.dcp",
            "/tmp/x.dcp",
            "/etc/passwd",
            "C:\\x.dcp",
            "C:/x.dcp",
            "Profiles\\Camera\\mine.dcp",
            "./mine.dcp",
        ] {
            let s = Settings {
                camera_profile: Some(CameraProfile {
                    enabled: true,
                    path: path.to_owned(),
                    checksum: format!("blake3:{}", "a".repeat(64)),
                }),
                ..Settings::default()
            };
            assert!(
                matches!(s.validate(), Err(LeylineError::InvalidSettings(_))),
                "{path:?} should be rejected"
            );
        }
    }

    #[test]
    fn camera_profile_rejects_a_malformed_checksum() {
        for checksum in ["", "not-blake3:abcd", "blake3:tooshort", "blake3:zz"] {
            let s = Settings {
                camera_profile: Some(CameraProfile {
                    enabled: true,
                    path: "Profiles/Camera/mine.dcp".to_owned(),
                    checksum: checksum.to_owned(),
                }),
                ..Settings::default()
            };
            assert!(
                matches!(s.validate(), Err(LeylineError::InvalidSettings(_))),
                "{checksum:?} should be rejected"
            );
        }
    }

    #[test]
    fn camera_profile_accepts_a_well_formed_reference() {
        let s = Settings {
            camera_profile: Some(CameraProfile {
                enabled: true,
                path: "Profiles/Camera/mine.dcp".to_owned(),
                checksum: format!("blake3:{}", "a".repeat(64)),
            }),
            ..Settings::default()
        };
        s.validate().unwrap();
    }

    #[test]
    fn clarity_texture_dehaze_out_of_range_are_rejected() {
        for field in ["clarity", "texture", "dehaze"] {
            let mut s = Settings::default();
            match field {
                "clarity" => s.clarity = 101,
                "texture" => s.texture = -101,
                _ => s.dehaze = 200,
            }
            assert!(
                matches!(s.validate(), Err(LeylineError::InvalidSettings(_))),
                "{field} should be rejected"
            );
        }
    }

    #[test]
    fn neutral_hsl_and_color_grading_validate() {
        Settings::default().validate().unwrap();
    }

    #[test]
    fn hsl_band_out_of_range_is_rejected() {
        let mut s = Settings::default();
        s.hsl[0].hue = 150;
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn color_grading_hue_must_be_a_full_degree_circle() {
        let mut s = Settings::default();
        s.color_grading.shadows.hue = 360;
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
        s.color_grading.shadows.hue = 359;
        s.validate().unwrap();
    }

    #[test]
    fn color_grading_balance_and_blending_are_range_checked() {
        let mut s = Settings::default();
        s.color_grading.blending = -1;
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
        s.color_grading.blending = 0;
        s.color_grading.balance = 200;
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn tone_curve_accepts_empty_or_well_formed_points() {
        let mut s = Settings::default();
        s.validate().unwrap();

        s.tone_curve = ToneCurve {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.5, y: 0.6 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
            ..ToneCurve::default()
        };
        s.validate().unwrap();
    }

    #[test]
    fn tone_curve_rejects_a_single_point() {
        let s = Settings {
            tone_curve: ToneCurve {
                points: vec![CurvePoint { x: 0.5, y: 0.5 }],
                ..ToneCurve::default()
            },
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn tone_curve_rejects_out_of_range_coordinates() {
        let s = Settings {
            tone_curve: ToneCurve {
                points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.5, y: 1.0 }],
                ..ToneCurve::default()
            },
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn tone_curve_rejects_non_increasing_x() {
        let s = Settings {
            tone_curve: ToneCurve {
                points: vec![CurvePoint { x: 0.5, y: 0.0 }, CurvePoint { x: 0.5, y: 1.0 }],
                ..ToneCurve::default()
            },
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn spot_removal_accepts_a_well_formed_spot() {
        let s = Settings {
            spot_removal: vec![SpotRemoval {
                target: Point { x: 0.62, y: 0.31 },
                source: Point { x: 0.55, y: 0.29 },
                radius: 0.03,
                feather: 0.4,
                opacity: 1.0,
            }],
            ..Settings::default()
        };
        s.validate().unwrap();
    }

    #[test]
    fn spot_removal_rejects_out_of_range_points() {
        let s = Settings {
            spot_removal: vec![SpotRemoval {
                target: Point { x: 1.5, y: 0.31 },
                source: Point { x: 0.55, y: 0.29 },
                radius: 0.03,
                feather: 0.4,
                opacity: 1.0,
            }],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn spot_removal_rejects_non_positive_radius() {
        let s = Settings {
            spot_removal: vec![SpotRemoval {
                target: Point { x: 0.5, y: 0.5 },
                source: Point { x: 0.4, y: 0.4 },
                radius: 0.0,
                feather: 0.4,
                opacity: 1.0,
            }],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn spot_removal_rejects_out_of_range_feather_and_opacity() {
        let base = SpotRemoval {
            target: Point { x: 0.5, y: 0.5 },
            source: Point { x: 0.4, y: 0.4 },
            radius: 0.03,
            feather: 0.4,
            opacity: 1.0,
        };
        let mut s = Settings {
            spot_removal: vec![SpotRemoval {
                feather: 1.5,
                ..base
            }],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));

        s.spot_removal = vec![SpotRemoval {
            opacity: -0.1,
            ..base
        }];
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn spot_removal_round_trips_and_is_omitted_when_empty() {
        let s = Settings {
            spot_removal: vec![SpotRemoval {
                target: Point { x: 0.62, y: 0.31 },
                source: Point { x: 0.55, y: 0.29 },
                radius: 0.03,
                feather: 0.4,
                opacity: 1.0,
            }],
            ..Settings::default()
        };
        let reparsed = Settings::parse(&s.to_json()).unwrap();
        assert_eq!(s, reparsed);

        let neutral = Settings::default();
        let value: serde_json::Value = serde_json::from_str(&neutral.to_json()).unwrap();
        assert!(value.get("spot_removal").is_none());
    }

    // -------------------------------------------------------------------
    // Local adjustments (ADR 0029)
    // -------------------------------------------------------------------

    fn radial_adjustment() -> LocalAdjustment {
        LocalAdjustment {
            mask: Mask::Radial {
                cx: 0.5,
                cy: 0.42,
                rx: 0.30,
                ry: 0.22,
                angle: 0.0,
                feather: 0.40,
                inverted: false,
            },
            range: None,
            opacity: 1.0,
            adjustments: LocalAdjustmentValues {
                exposure: Some(0.6),
                contrast: Some(15),
                highlights: Some(-20),
                ..LocalAdjustmentValues::default()
            },
        }
    }

    #[test]
    fn local_adjustments_round_trip_and_are_omitted_when_empty() {
        let s = Settings {
            local_adjustments: vec![radial_adjustment()],
            ..Settings::default()
        };
        s.validate().unwrap();
        let reparsed = Settings::parse(&s.to_json()).unwrap();
        assert_eq!(s, reparsed);

        let neutral = Settings::default();
        let value: serde_json::Value = serde_json::from_str(&neutral.to_json()).unwrap();
        assert!(value.get("local_adjustments").is_none());
    }

    #[test]
    fn parses_the_adr_0029_example_json() {
        let json = r#"{
            "schema": 1,
            "stages": { "gains": 1, "vibrance": 1, "local_adjustments": 1 },
            "exposure": 0.35,
            "vibrance": 18,
            "local_adjustments": [
                {
                    "mask": {
                        "type": "radial",
                        "cx": 0.5, "cy": 0.42,
                        "rx": 0.30, "ry": 0.22,
                        "angle": 0.0,
                        "feather": 0.40,
                        "inverted": false
                    },
                    "opacity": 1.0,
                    "adjustments": { "exposure": 0.6, "contrast": 15, "highlights": -20 }
                },
                {
                    "mask": {
                        "type": "gradient",
                        "x0": 0.5, "y0": 0.0,
                        "x1": 0.5, "y1": 0.35
                    },
                    "opacity": 0.8,
                    "adjustments": { "exposure": -0.8, "whites": -10, "temperature": 5200, "tint": 6 }
                },
                {
                    "mask": {
                        "type": "brush",
                        "strokes": [
                            { "x": 0.20, "y": 0.60, "radius": 0.04, "flow": 1.0, "hardness": 0.5 },
                            { "x": 0.23, "y": 0.61, "radius": 0.04, "flow": 1.0, "hardness": 0.5 },
                            { "x": 0.26, "y": 0.62, "radius": 0.04, "flow": 1.0, "hardness": 0.5 }
                        ]
                    },
                    "opacity": 1.0,
                    "adjustments": { "saturation": -30, "shadows": 20 }
                }
            ]
        }"#;
        let s = Settings::parse(json).unwrap();
        assert_eq!(s.local_adjustments.len(), 3);
        s.validate().unwrap();
        match &s.local_adjustments[1].mask {
            Mask::Gradient { x0, y0, x1, y1 } => {
                assert_eq!((*x0, *y0, *x1, *y1), (0.5, 0.0, 0.5, 0.35));
            }
            other => panic!("expected a gradient mask, got {other:?}"),
        }
        assert_eq!(s.local_adjustments[1].adjustments.temperature, Some(5200));
        match &s.local_adjustments[2].mask {
            Mask::Brush { strokes } => assert_eq!(strokes.len(), 3),
            other => panic!("expected a brush mask, got {other:?}"),
        }
    }

    #[test]
    fn local_adjustments_rejects_out_of_range_opacity() {
        let mut adjustment = radial_adjustment();
        adjustment.opacity = 1.5;
        let s = Settings {
            local_adjustments: vec![adjustment],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    // -------------------------------------------------------------------
    // Range masks (ADR 0048)
    // -------------------------------------------------------------------

    fn ranged_adjustment() -> LocalAdjustment {
        LocalAdjustment {
            range: Some(RangeMask {
                luminance: Some(LuminanceRange::default()),
                color: Some(ColorRange::default()),
            }),
            ..radial_adjustment()
        }
    }

    /// ADR 0050 §5, the same capability rule one layer lower: the decoder
    /// configuration a revision pins decides whether the mode can be
    /// expressed at all.
    #[test]
    fn a_highlight_mode_on_a_revision_pinned_at_input_v1_is_refused() {
        let pinned_v1 = Settings {
            highlight_reconstruction: HighlightReconstruction::Rebuild,
            stages: StageVersions::from([("input".to_owned(), 1)]),
            ..Settings::default()
        };
        let error = pinned_v1.validate().unwrap_err().to_string();
        assert!(error.contains("input version 2"), "{error}");
        assert!(error.contains("reprocess"), "{error}");

        // Clipping is the neutral value, so it is expressible everywhere.
        let clipped = Settings {
            highlight_reconstruction: HighlightReconstruction::Clip,
            ..pinned_v1.clone()
        };
        assert!(clipped.validate().is_ok());

        // And so is any mode on a revision pinned at the version that reads
        // it, or at none at all (a fresh revision).
        let pinned_v2 = Settings {
            stages: StageVersions::from([("input".to_owned(), 2)]),
            ..pinned_v1.clone()
        };
        assert!(pinned_v2.validate().is_ok());
        let unpinned = Settings {
            stages: StageVersions::default(),
            ..pinned_v1
        };
        assert!(unpinned.validate().is_ok());
    }

    /// The neutral mode leaves no trace in a stored document, and a stored
    /// one round-trips (`docs/pipeline.md` §3.4).
    #[test]
    fn the_highlight_mode_is_omitted_when_neutral_and_round_trips_otherwise() {
        let neutral = Settings::default();
        let json: serde_json::Value = serde_json::from_str(&neutral.to_json()).unwrap();
        assert!(json.get("highlight_reconstruction").is_none());

        let asked = Settings {
            highlight_reconstruction: HighlightReconstruction::Blend,
            ..Settings::default()
        };
        let json = asked.to_json();
        assert!(
            json.contains("\"highlight_reconstruction\":\"blend\""),
            "{json}"
        );
        assert_eq!(
            Settings::parse(&json).unwrap().highlight_reconstruction,
            HighlightReconstruction::Blend
        );
    }

    /// The decision of ADR 0048 §5: a revision pinned at `local_adjustments`
    /// v1 cannot express a range, and the pinning rule keeps it at v1 — so the
    /// only honest outcome is a refusal, never a slider that does nothing.
    #[test]
    fn a_range_on_a_revision_pinned_at_v1_is_refused() {
        let s = Settings {
            local_adjustments: vec![ranged_adjustment()],
            stages: StageVersions::from([("local_adjustments".to_owned(), 1)]),
            ..Settings::default()
        };
        let message = match s.validate() {
            Err(LeylineError::InvalidSettings(message)) => message,
            other => panic!("expected a refusal, got {other:?}"),
        };
        // The message has to name the remedy, since the user cannot guess it.
        assert!(message.contains("reprocess"), "{message}");
    }

    #[test]
    fn a_range_is_accepted_at_v2_and_on_unpinned_settings() {
        let at_v2 = Settings {
            local_adjustments: vec![ranged_adjustment()],
            stages: StageVersions::from([("local_adjustments".to_owned(), 2)]),
            ..Settings::default()
        };
        at_v2.validate().unwrap();

        // Built in memory, nothing pinned yet: the engine will pin the
        // current version when the revision is written.
        let unpinned = Settings {
            local_adjustments: vec![ranged_adjustment()],
            ..Settings::default()
        };
        unpinned.validate().unwrap();
    }

    fn neighbourhood_adjustment() -> LocalAdjustment {
        LocalAdjustment {
            adjustments: LocalAdjustmentValues {
                texture: Some(-40),
                ..radial_adjustment().adjustments
            },
            ..radial_adjustment()
        }
    }

    /// ADR 0108 §5: the capability rule for the fourth time. Versions 1 to 3
    /// have no code for the five neighbourhood operators, so a revision
    /// pinned at any of them is refused rather than rendered with a slider
    /// that silently does nothing.
    #[test]
    fn a_neighbourhood_value_below_local_adjustments_v4_is_refused() {
        for pinned in [1, 2, 3] {
            let s = Settings {
                local_adjustments: vec![neighbourhood_adjustment()],
                stages: StageVersions::from([("local_adjustments".to_owned(), pinned)]),
                ..Settings::default()
            };
            let message = match s.validate() {
                Err(LeylineError::InvalidSettings(message)) => message,
                other => panic!("expected a refusal at v{pinned}, got {other:?}"),
            };
            // The message names the remedy, as ADR 0048 §5's does: the user
            // cannot guess that reprocessing is what unblocks them.
            assert!(message.contains("reprocess"), "v{pinned}: {message}");
            assert!(message.contains("version 4"), "v{pinned}: {message}");
        }
    }

    /// The sixth application of the same rule (ADR 0116 §4): the defringe
    /// pair needs `local_adjustments` v5, and an earlier pin is refused by
    /// name rather than rendered without it.
    #[test]
    fn a_local_defringe_on_a_revision_pinned_below_v5_is_refused() {
        let with_defringe = LocalAdjustment {
            mask: Mask::Everything,
            range: None,
            opacity: 1.0,
            adjustments: LocalAdjustmentValues {
                defringe_purple: Some(60),
                ..LocalAdjustmentValues::default()
            },
        };
        for pinned in [1u16, 2, 3, 4] {
            let refused = Settings {
                local_adjustments: vec![with_defringe.clone()],
                stages: StageVersions::from([("local_adjustments".to_owned(), pinned)]),
                ..Settings::default()
            };
            let message = match refused.validate() {
                Err(LeylineError::InvalidSettings(message)) => message,
                other => panic!("v{pinned}: expected a refusal, got {other:?}"),
            };
            assert!(message.contains("version 5"), "v{pinned}: {message}");
            assert!(message.contains("reprocess"), "v{pinned}: {message}");
        }

        // Accepted at v5, and on settings nothing has pinned yet.
        Settings {
            local_adjustments: vec![with_defringe.clone()],
            stages: StageVersions::from([("local_adjustments".to_owned(), 5)]),
            ..Settings::default()
        }
        .validate()
        .unwrap();
        Settings {
            local_adjustments: vec![with_defringe],
            ..Settings::default()
        }
        .validate()
        .unwrap();

        // And an adjustment that never asks for it stays valid at v4 — the
        // rule refuses an inexpressible setting, not an old version.
        Settings {
            local_adjustments: vec![LocalAdjustment {
                mask: Mask::Everything,
                range: None,
                opacity: 1.0,
                adjustments: LocalAdjustmentValues {
                    clarity: Some(20),
                    ..LocalAdjustmentValues::default()
                },
            }],
            stages: StageVersions::from([("local_adjustments".to_owned(), 4)]),
            ..Settings::default()
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn a_neighbourhood_value_is_accepted_at_v4_and_on_unpinned_settings() {
        let at_v4 = Settings {
            local_adjustments: vec![neighbourhood_adjustment()],
            stages: StageVersions::from([("local_adjustments".to_owned(), 4)]),
            ..Settings::default()
        };
        at_v4.validate().unwrap();

        let unpinned = Settings {
            local_adjustments: vec![neighbourhood_adjustment()],
            ..Settings::default()
        };
        unpinned.validate().unwrap();
    }

    /// A revision pinned at an older version that never asks for one of the
    /// five stays valid — the refusal is about the *setting*, not about the
    /// pinning. The mirror of `masking_zero_...` for ADR 0096.
    #[test]
    fn an_old_pin_without_any_neighbourhood_value_stays_valid() {
        for pinned in [1, 2, 3] {
            let s = Settings {
                local_adjustments: vec![radial_adjustment()],
                stages: StageVersions::from([("local_adjustments".to_owned(), pinned)]),
                ..Settings::default()
            };
            s.validate().unwrap();
        }
    }

    /// Noise reduction has no meaningful negative, exactly as its global
    /// sliders have none — while texture, clarity and sharpness do, and the
    /// negative half is the point (ADR 0108 §1).
    #[test]
    fn local_noise_refuses_a_negative_while_texture_accepts_one() {
        let with = |values: LocalAdjustmentValues| Settings {
            local_adjustments: vec![LocalAdjustment {
                adjustments: values,
                ..radial_adjustment()
            }],
            ..Settings::default()
        };
        assert!(
            with(LocalAdjustmentValues {
                noise_luminance: Some(-1),
                ..LocalAdjustmentValues::default()
            })
            .validate()
            .is_err()
        );
        assert!(
            with(LocalAdjustmentValues {
                noise_color: Some(-1),
                ..LocalAdjustmentValues::default()
            })
            .validate()
            .is_err()
        );
        for values in [
            LocalAdjustmentValues {
                texture: Some(-100),
                ..LocalAdjustmentValues::default()
            },
            LocalAdjustmentValues {
                clarity: Some(-100),
                ..LocalAdjustmentValues::default()
            },
            LocalAdjustmentValues {
                sharpness: Some(-100),
                ..LocalAdjustmentValues::default()
            },
        ] {
            with(values).validate().unwrap();
        }
    }

    /// The five are omitted from a stored document when neutral, like every
    /// other optional local value (`docs/pipeline.md` §3.4).
    #[test]
    fn the_five_neighbourhood_values_round_trip_and_are_omitted_when_absent() {
        let plain = Settings {
            local_adjustments: vec![radial_adjustment()],
            ..Settings::default()
        };
        // Read the entry's own `adjustments` object rather than the whole
        // document: `clarity` and `texture` are *also* global sliders, always
        // serialized, so a substring search over the document proves nothing.
        let json: serde_json::Value = serde_json::from_str(&plain.to_json()).unwrap();
        let stored = &json["local_adjustments"][0]["adjustments"];
        for name in [
            "clarity",
            "texture",
            "sharpness",
            "noise_luminance",
            "noise_color",
        ] {
            assert!(
                stored.get(name).is_none(),
                "{name} leaked into a neutral entry: {stored}"
            );
        }

        let set = Settings {
            local_adjustments: vec![LocalAdjustment {
                adjustments: LocalAdjustmentValues {
                    clarity: Some(30),
                    texture: Some(-45),
                    sharpness: Some(20),
                    noise_luminance: Some(15),
                    noise_color: Some(10),
                    ..LocalAdjustmentValues::default()
                },
                ..radial_adjustment()
            }],
            ..Settings::default()
        };
        let parsed = Settings::parse(&set.to_json()).unwrap();
        assert_eq!(parsed.local_adjustments, set.local_adjustments);
    }

    /// ADR 0096 §3: the capability rule, applied to sharpening's edge mask.
    #[test]
    fn masking_on_a_revision_pinned_at_sharpen_v1_is_refused() {
        let masked = Sharpening {
            amount: 50,
            radius: 1.0,
            masking: 40,
        };
        let refused = Settings {
            sharpening: masked.clone(),
            stages: StageVersions::from([("sharpen".to_owned(), 1)]),
            ..Settings::default()
        };
        let message = match refused.validate() {
            Err(LeylineError::InvalidSettings(message)) => message,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert!(message.contains("sharpen version 2"), "{message}");
        // The message names the remedy, since the user cannot guess it.
        assert!(message.contains("reprocess"), "{message}");

        // Accepted at v2, and on settings nothing has pinned yet.
        Settings {
            sharpening: masked.clone(),
            stages: StageVersions::from([("sharpen".to_owned(), 2)]),
            ..Settings::default()
        }
        .validate()
        .unwrap();
        Settings {
            sharpening: masked,
            ..Settings::default()
        }
        .validate()
        .unwrap();

        // A revision pinned at v1 that never asks for masking stays valid:
        // the rule refuses an inexpressible setting, not an old version.
        Settings {
            sharpening: Sharpening {
                amount: 50,
                radius: 1.0,
                masking: 0,
            },
            stages: StageVersions::from([("sharpen".to_owned(), 1)]),
            ..Settings::default()
        }
        .validate()
        .unwrap();
    }

    /// The same rule for ADR 0118's coloured grain, and the same three
    /// cases: refused at v1, accepted at v2 and unpinned, and a v1 revision
    /// that never asks for colour left alone.
    #[test]
    fn coloured_grain_on_a_revision_pinned_at_grain_v1_is_refused() {
        let coloured = Grain {
            amount: 60,
            size: 25,
            roughness: 50,
            color: 40,
        };
        let message = match (Settings {
            grain: coloured,
            stages: StageVersions::from([("grain".to_owned(), 1)]),
            ..Settings::default()
        })
        .validate()
        {
            Err(LeylineError::InvalidSettings(message)) => message,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert!(message.contains("grain version 2"), "{message}");
        assert!(message.contains("reprocess"), "{message}");

        for stages in [
            StageVersions::from([("grain".to_owned(), 2)]),
            StageVersions::new(),
        ] {
            Settings {
                grain: coloured,
                stages,
                ..Settings::default()
            }
            .validate()
            .unwrap();
        }

        Settings {
            grain: Grain {
                color: 0,
                ..coloured
            },
            stages: StageVersions::from([("grain".to_owned(), 1)]),
            ..Settings::default()
        }
        .validate()
        .unwrap();
    }

    /// The same rule for ADR 0111's manual chromatic aberration, and the
    /// same three cases: refused at v1, accepted at v2 and unpinned, and a
    /// v1 revision that never asks for it left alone.
    #[test]
    fn manual_tca_on_a_revision_pinned_at_lens_v1_is_refused() {
        let measured = LensCorrection {
            enabled: false,
            tca_red: 0.12,
            ..LensCorrection::default()
        };
        let refused = Settings {
            lens_correction: measured.clone(),
            stages: StageVersions::from([("lens".to_owned(), 1)]),
            ..Settings::default()
        };
        let message = match refused.validate() {
            Err(LeylineError::InvalidSettings(message)) => message,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert!(message.contains("lens version 2"), "{message}");
        assert!(message.contains("reprocess"), "{message}");

        Settings {
            lens_correction: measured.clone(),
            stages: StageVersions::from([("lens".to_owned(), 2)]),
            ..Settings::default()
        }
        .validate()
        .unwrap();
        Settings {
            lens_correction: measured,
            ..Settings::default()
        }
        .validate()
        .unwrap();

        // A lens correction pinned at v1 that never asks for the manual map
        // stays valid — including with the Lensfun half switched on.
        Settings {
            lens_correction: LensCorrection {
                enabled: true,
                ..LensCorrection::default()
            },
            stages: StageVersions::from([("lens".to_owned(), 1)]),
            ..Settings::default()
        }
        .validate()
        .unwrap();
    }

    /// A percent of the radius, and one percent is already absurd for a real
    /// lens — the range is refused by name rather than clamped in silence.
    #[test]
    fn a_manual_tca_beyond_one_percent_is_refused() {
        for (red, blue) in [(1.5, 0.0), (0.0, -2.0), (f64::NAN, 0.0)] {
            let settings = Settings {
                lens_correction: LensCorrection {
                    tca_red: red,
                    tca_blue: blue,
                    ..LensCorrection::default()
                },
                ..Settings::default()
            };
            assert!(
                matches!(settings.validate(), Err(LeylineError::InvalidSettings(_))),
                "{red} / {blue} should be refused"
            );
        }
    }

    /// The two coefficients activate the correction on their own — the
    /// property ADR 0111 §5 turns into the stage's `active` predicate.
    #[test]
    fn manual_tca_is_asked_for_without_enabling_the_profile_correction() {
        let off = LensCorrection::default();
        assert!(!off.has_manual_tca());
        assert!(
            LensCorrection {
                tca_blue: -0.05,
                ..LensCorrection::default()
            }
            .has_manual_tca()
        );
    }

    #[test]
    fn a_range_rejects_an_inverted_luminance_band() {
        let mut adjustment = ranged_adjustment();
        adjustment.range = Some(RangeMask {
            luminance: Some(LuminanceRange {
                min: 0.8,
                max: 0.2,
                softness: 0.1,
            }),
            color: None,
        });
        let s = Settings {
            local_adjustments: vec![adjustment],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn a_range_rejects_a_hue_band_wider_than_the_circle() {
        for (width, softness) in [(0.0, 10.0), (181.0, 10.0), (30.0, 200.0)] {
            let mut adjustment = ranged_adjustment();
            adjustment.range = Some(RangeMask {
                luminance: None,
                color: Some(ColorRange {
                    center: 210.0,
                    width,
                    softness,
                }),
            });
            let s = Settings {
                local_adjustments: vec![adjustment],
                ..Settings::default()
            };
            assert!(
                matches!(s.validate(), Err(LeylineError::InvalidSettings(_))),
                "width {width}, softness {softness} should be refused"
            );
        }
    }

    /// A neutral range is absent, not present-and-empty: an adjustment without
    /// one must not gain a `range` key in `settings_json` (§3.4 compatibility).
    #[test]
    fn an_adjustment_without_a_range_serializes_without_the_field() {
        let s = Settings {
            local_adjustments: vec![radial_adjustment()],
            ..Settings::default()
        };
        let value: serde_json::Value = serde_json::from_str(&s.to_json()).unwrap();
        let entry = &value["local_adjustments"][0];
        assert!(entry.get("range").is_none(), "{entry}");
        // And it round-trips.
        assert_eq!(Settings::parse(&s.to_json()).unwrap(), s);
    }

    #[test]
    fn radial_mask_rejects_non_positive_radii() {
        let mut adjustment = radial_adjustment();
        adjustment.mask = Mask::Radial {
            cx: 0.5,
            cy: 0.5,
            rx: 0.0,
            ry: 0.2,
            angle: 0.0,
            feather: 0.4,
            inverted: false,
        };
        let s = Settings {
            local_adjustments: vec![adjustment],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn gradient_mask_rejects_coincident_endpoints() {
        let mut adjustment = radial_adjustment();
        adjustment.mask = Mask::Gradient {
            x0: 0.5,
            y0: 0.5,
            x1: 0.5,
            y1: 0.5,
        };
        let s = Settings {
            local_adjustments: vec![adjustment],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn brush_mask_rejects_an_empty_stroke_list() {
        let mut adjustment = radial_adjustment();
        adjustment.mask = Mask::Brush { strokes: vec![] };
        let s = Settings {
            local_adjustments: vec![adjustment],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn brush_mask_rejects_a_non_positive_stroke_radius() {
        let mut adjustment = radial_adjustment();
        adjustment.mask = Mask::Brush {
            strokes: vec![BrushStroke {
                x: 0.2,
                y: 0.6,
                radius: 0.0,
                flow: 1.0,
                hardness: 0.5,
            }],
        };
        let s = Settings {
            local_adjustments: vec![adjustment],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn local_adjustments_rejects_out_of_range_slider_values() {
        let mut adjustment = radial_adjustment();
        adjustment.adjustments.saturation = Some(200);
        let s = Settings {
            local_adjustments: vec![adjustment],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn local_adjustments_rejects_zero_temperature() {
        let mut adjustment = radial_adjustment();
        adjustment.adjustments.temperature = Some(0);
        let s = Settings {
            local_adjustments: vec![adjustment],
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(matches!(
            Settings::parse("{ not json"),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn preset_captures_only_the_requested_groups() {
        let s = Settings::parse(SPEC_EXAMPLE).unwrap();
        let preset = PresetSettings::capture(&s, &[SettingsGroup::Tone, SettingsGroup::Presence]);
        assert_eq!(preset.schema, 1);
        assert_eq!(preset.exposure, Some(0.35));
        assert_eq!(preset.contrast, Some(12));
        assert_eq!(preset.vibrance, Some(18));
        assert_eq!(preset.saturation, Some(0));
        // Not requested: absent, not neutral.
        assert_eq!(preset.white_balance, None);
        assert_eq!(preset.lens_correction, None);
        assert_eq!(preset.rotation, None);
        assert_eq!(preset.crop, None);
    }

    #[test]
    fn preset_json_omits_fields_of_excluded_groups() {
        let s = Settings::parse(SPEC_EXAMPLE).unwrap();
        let preset = PresetSettings::capture(&s, &[SettingsGroup::WhiteBalance]);
        let json = preset.to_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("white_balance").is_some());
        assert!(value.get("exposure").is_none());
        assert!(value.get("crop").is_none());
    }

    #[test]
    fn preset_round_trips_losslessly() {
        let s = Settings::parse(SPEC_EXAMPLE).unwrap();
        let preset = PresetSettings::capture(&s, &[SettingsGroup::Detail, SettingsGroup::Geometry]);
        let reparsed = PresetSettings::parse(&preset.to_json()).unwrap();
        assert_eq!(preset, reparsed);
    }
}

#[cfg(test)]
mod specification {
    use super::*;

    /// `docs/pipeline.md` §3.2 owns the list of `settings_json` keys and
    /// their neutral values. A field that exists here and is named nowhere
    /// there is a parameter a user can find in a stored revision and cannot
    /// look up — which is how `demosaic` spent two weeks unlisted after
    /// [ADR 0061](../../../docs/adr/0061-demosaic-algorithm.md) added it.
    ///
    /// Serialization is what is compared, not the Rust identifiers: the JSON
    /// keys are the contract, and `serde` may rename one.
    #[test]
    fn every_settings_key_is_named_in_the_pipeline_specification() {
        // Everything set away from neutral, so nothing is skipped by
        // `skip_serializing_if` — the whole surface, in one document.
        let all = Settings {
            // Every field that `skip_serializing_if` can drop, pushed off its
            // neutral value — the first version of this test left `demosaic`
            // at its default and therefore did not notice it was serializing
            // nothing to check.
            demosaic: Demosaic::Dcb,
            highlight_reconstruction: HighlightReconstruction::Rebuild,
            defringe: Defringe {
                purple: 40,
                green: 20,
            },
            reshape: vec![ReshapePoint {
                from: Point { x: 0.4, y: 0.4 },
                to: Point { x: 0.45, y: 0.42 },
                radius: 0.1,
                strength: 1.0,
            }],
            spot_removal: vec![SpotRemoval {
                target: Point { x: 0.5, y: 0.5 },
                source: Point { x: 0.4, y: 0.4 },
                radius: 0.05,
                feather: 0.5,
                opacity: 1.0,
            }],
            local_adjustments: vec![LocalAdjustment {
                mask: Mask::Everything,
                range: Some(RangeMask::default()),
                opacity: 1.0,
                adjustments: LocalAdjustmentValues::default(),
            }],
            white_balance: Some(WhiteBalance::default()),
            camera_profile: Some(CameraProfile {
                enabled: true,
                path: "Profiles/Camera/x.dcp".to_owned(),
                checksum: "blake3:0".to_owned(),
            }),
            lut: Some(Lut {
                enabled: true,
                path: "Profiles/LUT/x.cube".to_owned(),
                checksum: "blake3:0".to_owned(),
                strength: 100,
            }),
            tone_curve: ToneCurve {
                points: vec![CurvePoint { x: 0.0, y: 0.0 }],
                ..ToneCurve::default()
            },
            perspective: Some(Perspective::default()),
            crop: Some(Crop {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            }),
            ..Settings::default()
        };
        let serde_json::Value::Object(document) =
            serde_json::from_str::<serde_json::Value>(&all.to_json()).unwrap()
        else {
            panic!("settings serialize to an object");
        };

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/pipeline.md");
        let spec = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

        let unlisted: Vec<&String> = document
            .keys()
            // `schema` and `stages` are the two reserved fields, documented
            // in their own table rather than among the parameters.
            .filter(|key| !spec.contains(&format!("`{key}`")))
            .collect();
        assert!(
            unlisted.is_empty(),
            "{} settings key(s) are stored in revisions and named nowhere in \
             docs/pipeline.md — a parameter a reader cannot look up:\n  {:?}",
            unlisted.len(),
            unlisted
        );
    }
}
