//! Local adjustments v5 (ADR 0116) — rank 160. `v4` plus the defringe pair:
//! the coloured halo of axial aberration, taken out inside a mask instead of
//! across the frame.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).
//!
//! Everything a `v4` revision expresses renders here **exactly as `v4`
//! renders it**: both amounts are guarded by their own `Option`, and an
//! entry that sets neither runs the identical operator sequence. The
//! reference renders record that — every `v4` entry has a `v5` twin with the
//! same hash (ADR 0116 §4).
//!
//! Two things this is **not** (ADR 0116 §3), and they are properties rather
//! than defects: it runs at rank 160, so its edge mask reads a contrast the
//! tone curve has stretched, unlike the global stage at rank 22; and, like
//! every operator here, it reads neighbours the mask excludes — which for a
//! fringe on a mask boundary is what one wants.
//!
//! The operator sequence is copied from `v4` rather than called into it, for
//! the reason ADR 0048 §4 gave: a frozen module never receives a fix, only a
//! successor, so two copies cannot drift.

use leyline_core::{ColorRange, LocalAdjustment, LuminanceRange, RangeMask, WhiteBalance};

use crate::mask;
use crate::mask_coverage::MaskCoverages;
use crate::pixels::{Pixels, luma};
use crate::stages::clarity::v1::CLARITY_RADIUS;
use crate::stages::contrast::v1::contrast;
use crate::stages::gains::v1::linear_gains;
use crate::stages::highlights_shadows::v1::highlights_shadows;
use crate::stages::kernel::v1::{display, local_contrast, rgb_to_hsl, saturate, smoothstep01};
use crate::stages::noise_color::v2::color_noise_reduction;
use crate::stages::noise_luminance::v2::luminance_noise_reduction;
use crate::stages::sharpen::v2::sharpen;
use crate::stages::texture::v1::TEXTURE_RADIUS;
use crate::stages::whites_blacks::v1::whites_blacks;

/// Unsharp-mask radius for a local `sharpness`, in pixels at full
/// resolution — `Sharpening::default()`'s radius, frozen here rather than
/// read from the revision (ADR 0108 §3).
const LOCAL_SHARPEN_RADIUS: f64 = 1.0;

/// Applies every local adjustment, in list order: later entries composite on
/// top of what earlier ones wrote.
pub(crate) fn local_adjustments(
    px: &mut Pixels,
    adjustments: &[LocalAdjustment],
    rotation_degrees: f64,
    coverages: &MaskCoverages,
    scale: f32,
) {
    for adjustment in adjustments {
        apply_local_adjustment(px, adjustment, rotation_degrees, coverages, scale);
    }
}

/// Same composition as `v3` — develop a full copy through the re-parameterized
/// global operators, blend it back by coverage × opacity, narrowed by the
/// entry's range mask when it has one — with five more operators available on
/// that copy, the ones that read a neighbourhood.
pub(crate) fn apply_local_adjustment(
    px: &mut Pixels,
    adjustment: &LocalAdjustment,
    rotation_degrees: f64,
    coverages: &MaskCoverages,
    scale: f32,
) {
    let mut coverage = mask::rasterize_coverage(
        &adjustment.mask,
        coverages,
        px.width,
        px.height,
        rotation_degrees,
    );
    if let Some(range) = &adjustment.range {
        narrow_by_range(&mut coverage, px, range);
    }
    let values = &adjustment.adjustments;
    let mut adjusted = px.clone();
    // Defringe first, before the per-pixel values: inside a local
    // adjustment operators run in the order of their own ranks (ADR 0108
    // §4), and this one's stage is at rank 22 — ahead of everything else a
    // mask can carry (ADR 0116 §2). The dilation radius is this version's
    // constant scaled by the render scale, never the revision's global
    // defringe (ADR 0108 §3).
    if values.uses_defringe() {
        crate::stages::defringe::v1::defringe(
            &mut adjusted,
            values.defringe_purple.unwrap_or(0),
            values.defringe_green.unwrap_or(0),
            scale,
        );
    }
    if values.temperature.is_some() || values.tint.is_some() || values.exposure.is_some() {
        let wb = if values.temperature.is_some() || values.tint.is_some() {
            Some(WhiteBalance {
                temperature: values.temperature.unwrap_or(6500),
                tint: values.tint.unwrap_or(0),
            })
        } else {
            None
        };
        linear_gains(&mut adjusted, wb.as_ref(), values.exposure.unwrap_or(0.0));
    }
    if let Some(v) = values.contrast {
        contrast(&mut adjusted, v);
    }
    if values.highlights.is_some() || values.shadows.is_some() {
        highlights_shadows(
            &mut adjusted,
            values.highlights.unwrap_or(0),
            values.shadows.unwrap_or(0),
        );
    }
    if values.whites.is_some() || values.blacks.is_some() {
        whites_blacks(
            &mut adjusted,
            values.whites.unwrap_or(0),
            values.blacks.unwrap_or(0),
        );
    }
    if let Some(v) = values.vibrance {
        saturate(&mut adjusted, v, true);
    }
    if let Some(v) = values.saturation {
        saturate(&mut adjusted, v, false);
    }
    // From here on, operators that read more than one pixel. They run in the
    // order of their own ranks in the pipeline — clarity 90, texture 100,
    // noise 170 and 180, sharpen 190 — which is the rule the ten values above
    // have followed since ADR 0029 without anyone writing it down
    // (ADR 0108 §4).
    //
    // Every radius is a constant bound to *this* stage version and scaled by
    // the render scale, never read from the revision's global settings
    // (ADR 0108 §3): a local slider whose meaning moved when a global one was
    // touched would be unreasonable about.
    if let Some(v) = values.clarity {
        local_contrast(&mut adjusted, v, CLARITY_RADIUS * scale);
    }
    if let Some(v) = values.texture {
        local_contrast(&mut adjusted, v, TEXTURE_RADIUS * scale);
    }
    // The edge-preserving operators of ranks 170 and 180, never the measured
    // ones of ranks 5 and 6: those read sensor counts, and past exposure and
    // the tone curve a measured threshold means nothing (ADR 0072 §5, the
    // reason it moved them to the head of the pipeline).
    if let Some(v) = values.noise_luminance {
        luminance_noise_reduction(&mut adjusted, v, scale);
    }
    if let Some(v) = values.noise_color {
        color_noise_reduction(&mut adjusted, v, scale);
    }
    // One slider, not `Sharpening`'s three: the radius is the default the
    // global stage carries and the edge mask is off, which makes this the
    // unsharp mask of `sharpen::v1` exactly. Below zero it subtracts its own
    // detail, and that softening is the one thing this develop module could
    // not express at any strength before (ADR 0108 §1).
    if let Some(v) = values.sharpness {
        sharpen(&mut adjusted, v, LOCAL_SHARPEN_RADIUS * f64::from(scale), 0);
    }
    mask::blend_by_coverage(px, &adjusted, &coverage, adjustment.opacity);
}

/// Multiplies `coverage` in place by the range terms — so a range can only
/// ever narrow a mask, never widen it (ADR 0048 §1).
///
/// The pixel is read on the display axis (ADR 0048 §3): a luminance band is a
/// statement about the histogram the user is looking at, and the same numbers
/// in linear light would put their lower edge in near-blackness. Values above
/// white are clamped into the band's own space, headroom being outside what a
/// `[0, 1]` band can talk about.
fn narrow_by_range(coverage: &mut [f32], px: &Pixels, range: &RangeMask) {
    for (i, cell) in coverage.iter_mut().enumerate() {
        if *cell <= 0.0 {
            // Outside the geometry already: nothing to narrow, and no reason
            // to pay for the conversion.
            continue;
        }
        let rgb: [f32; 3] = [
            display(px.data[i * 3]),
            display(px.data[i * 3 + 1]),
            display(px.data[i * 3 + 2]),
        ];
        if let Some(band) = &range.luminance {
            *cell *= luminance_term(luma(&rgb), band);
        }
        if let Some(band) = &range.color {
            *cell *= color_term(&rgb, band);
        }
    }
}

/// Coverage of a luminance band: 1 inside `[min, max]`, easing to 0 over
/// `softness` on each side.
fn luminance_term(value: f32, band: &LuminanceRange) -> f32 {
    let value = value.clamp(0.0, 1.0) as f64;
    let softness = band.softness.max(0.0);
    let below = edge_falloff(band.min - value, softness);
    let above = edge_falloff(value - band.max, softness);
    (below * above) as f32
}

/// Coverage of a hue band, weighted by the pixel's saturation.
///
/// The saturation weight is what makes the band usable: hue is undefined on a
/// neutral pixel, and letting greys through would select the clouds along with
/// the blue of the sky (ADR 0048 §2). Full weight is reached at the same
/// saturation the falloff uses for hue, which keeps one knob instead of two.
fn color_term(rgb: &[f32; 3], band: &ColorRange) -> f32 {
    let (hue, saturation, _) = rgb_to_hsl(rgb);
    let distance = hue_distance(f64::from(hue), band.center);
    let softness = band.softness.max(0.0);
    let hue_term = edge_falloff(distance - band.width.max(f64::EPSILON), softness);
    // Below this saturation a pixel has no hue worth speaking of; the ramp
    // avoids a hard boundary in near-neutral areas.
    const SATURATION_FLOOR: f32 = 0.10;
    let saturation_term = smoothstep01((saturation / SATURATION_FLOOR).clamp(0.0, 1.0));
    hue_term as f32 * saturation_term
}

/// Shortest angular distance between two hues, in degrees — hue is circular,
/// so 350° and 10° are 20° apart.
fn hue_distance(a: f64, b: f64) -> f64 {
    let delta = (a - b).rem_euclid(360.0);
    delta.min(360.0 - delta)
}

/// How much survives at `overshoot` past an edge: 1 at or before the edge, 0
/// at `softness` beyond it, smoothly in between. A `softness` of 0 is the hard
/// edge the caller asked for.
fn edge_falloff(overshoot: f64, softness: f64) -> f64 {
    if overshoot <= 0.0 {
        return 1.0;
    }
    if softness <= 0.0 || overshoot >= softness {
        return 0.0;
    }
    f64::from(smoothstep01(1.0 - (overshoot / softness) as f32))
}
