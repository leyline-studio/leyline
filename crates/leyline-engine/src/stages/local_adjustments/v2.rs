//! Local adjustments v2 (ADR 0048) — rank 160. `v1` plus range masks: the
//! geometric coverage may be narrowed by a band of luminance and a band of
//! hue.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).
//!
//! Two things live here rather than in `crate::mask`, deliberately. The
//! range terms are a **formula on pixels**, and `mask.rs` is shared by every
//! stage version precisely because it holds none: putting them there would
//! make `v1`'s frozen render depend on mutable code (ADR 0048 §4). The
//! operator calls are pinned the same way `v1` pins them — `gains::v1`,
//! `contrast::v1`, … — so a future `contrast::v2` leaves this module alone.
//!
//! An adjustment without a `range` renders exactly as `v1` renders it: the
//! multiplication by the range terms is skipped, not multiplied by one.

use leyline_core::{ColorRange, LocalAdjustment, LuminanceRange, RangeMask, WhiteBalance};

use crate::mask;
use crate::pixels::{Pixels, luma};
use crate::stages::contrast::v1::contrast;
use crate::stages::gains::v1::linear_gains;
use crate::stages::highlights_shadows::v1::highlights_shadows;
use crate::stages::kernel::v1::{display, rgb_to_hsl, saturate, smoothstep01};
use crate::stages::whites_blacks::v1::whites_blacks;

/// Applies every local adjustment, in list order: later entries composite on
/// top of what earlier ones wrote.
pub(crate) fn local_adjustments(
    px: &mut Pixels,
    adjustments: &[LocalAdjustment],
    rotation_degrees: f64,
) {
    for adjustment in adjustments {
        apply_local_adjustment(px, adjustment, rotation_degrees);
    }
}

/// Same composition as `v1` — develop a full copy through the re-parameterized
/// global operators, blend it back by coverage × opacity — with the coverage
/// narrowed by the entry's range mask when it has one.
pub(crate) fn apply_local_adjustment(
    px: &mut Pixels,
    adjustment: &LocalAdjustment,
    rotation_degrees: f64,
) {
    let mut coverage =
        mask::rasterize_coverage(&adjustment.mask, px.width, px.height, rotation_degrees);
    if let Some(range) = &adjustment.range {
        narrow_by_range(&mut coverage, px, range);
    }
    let values = &adjustment.adjustments;
    let mut adjusted = px.clone();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance_band() -> LuminanceRange {
        LuminanceRange {
            min: 0.4,
            max: 0.6,
            softness: 0.1,
        }
    }

    #[test]
    fn a_luminance_band_is_full_inside_and_zero_past_the_falloff() {
        let band = luminance_band();
        assert_eq!(luminance_term(0.5, &band), 1.0);
        assert_eq!(luminance_term(0.4, &band), 1.0);
        assert_eq!(luminance_term(0.6, &band), 1.0);
        assert_eq!(luminance_term(0.29, &band), 0.0);
        assert_eq!(luminance_term(0.71, &band), 0.0);
        // Inside the falloff: strictly between, and monotonic.
        let near = luminance_term(0.65, &band);
        let far = luminance_term(0.68, &band);
        assert!(0.0 < far && far < near && near < 1.0, "{near} {far}");
    }

    #[test]
    fn a_zero_softness_luminance_band_is_a_hard_edge() {
        let band = LuminanceRange {
            min: 0.4,
            max: 0.6,
            softness: 0.0,
        };
        // Just inside and just outside, with room for the f32→f64 widening
        // of the sample value: the property is the cliff, not its exact
        // placement to the last bit.
        assert_eq!(luminance_term(0.59, &band), 1.0);
        assert_eq!(luminance_term(0.61, &band), 0.0);
    }

    #[test]
    fn a_hue_band_wraps_around_the_circle() {
        let band = ColorRange {
            center: 0.0,
            width: 30.0,
            softness: 10.0,
        };
        // Saturated red at hue 350 is 10° from center: inside the band.
        let red = color_term(&[1.0, 0.2, 0.35], &band);
        assert!(red > 0.9, "{red}");
        // Saturated blue is far outside it.
        let blue = color_term(&[0.2, 0.3, 1.0], &band);
        assert_eq!(blue, 0.0);
    }

    /// The point of the saturation weight: a grey pixel has no hue, so no hue
    /// band may select it (ADR 0048 §2).
    #[test]
    fn a_neutral_pixel_is_never_inside_a_hue_band() {
        for band_center in [0.0, 60.0, 120.0, 210.0, 300.0] {
            let band = ColorRange {
                center: band_center,
                width: 180.0,
                softness: 0.0,
            };
            for grey in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let term = color_term(&[grey, grey, grey], &band);
                assert_eq!(term, 0.0, "grey {grey} entered the band at {band_center}");
            }
        }
    }

    /// A range narrows and never widens: whatever the terms say, no pixel
    /// comes out with more coverage than the geometry gave it.
    #[test]
    fn a_range_can_only_narrow_the_geometry() {
        let px = Pixels {
            width: 4,
            height: 1,
            data: vec![
                0.0, 0.0, 0.0, // black
                0.2, 0.2, 0.2, // dark grey
                0.5, 0.5, 0.5, // mid grey
                1.0, 0.0, 0.0, // saturated red
            ],
        };
        let range = RangeMask {
            luminance: Some(LuminanceRange {
                min: 0.0,
                max: 1.0,
                softness: 0.0,
            }),
            color: Some(ColorRange {
                center: 0.0,
                width: 30.0,
                softness: 5.0,
            }),
        };
        let mut coverage = vec![0.7f32; 4];
        narrow_by_range(&mut coverage, &px, &range);
        for value in &coverage {
            assert!(*value <= 0.7 + f32::EPSILON, "{value} exceeds the geometry");
        }
        // Only the red pixel has a hue in the band.
        assert_eq!(coverage[0], 0.0);
        assert_eq!(coverage[1], 0.0);
        assert_eq!(coverage[2], 0.0);
        assert!(coverage[3] > 0.6, "{}", coverage[3]);
    }

    /// Zero geometric coverage stays zero, and is not even evaluated: a range
    /// refines a mask, it never introduces coverage of its own.
    #[test]
    fn uncovered_pixels_stay_uncovered() {
        let px = Pixels {
            width: 1,
            height: 1,
            data: vec![1.0, 0.0, 0.0],
        };
        let mut coverage = vec![0.0f32];
        narrow_by_range(
            &mut coverage,
            &px,
            &RangeMask {
                luminance: None,
                color: Some(ColorRange::default()),
            },
        );
        assert_eq!(coverage[0], 0.0);
    }
}
