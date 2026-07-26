//! Color grading v1 (ADR 0032) — rank 150, right after the HSL mixer.
//!
//! Introduced by process 9. Three tonal zones (shadows, midtones,
//! highlights), each tinted by a hue/saturation pair, weighted by luma with
//! a balance and a blending control.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::{ColorGrading, ColorGradingZone};

use crate::pixels::{Pixels, luma};
use crate::stages::hsl::v1::MAX_LUM_SHIFT;
use crate::stages::kernel::v1::{
    hsl_to_rgb, in_display, par_rows, preserving_headroom, smoothstep01,
};

/// How strongly a color grading zone's fully-saturated color tints a pixel
/// fully weighted into that zone.
pub(crate) const GRADING_TINT_STRENGTH: f32 = 0.6;

/// Applies shadows/midtones/highlights color grading: every pixel's own
/// Rec. 709 luma decides its membership weight in each of the three zones
/// ([`zone_weights`], a partition of unity), each zone's color tints the
/// pixel proportional to that weight and the zone's own saturation
/// ([`zone_tint`]), and each zone's luminance offset blends the same way.
/// A pixel's hue/lightness relationship to the mixer above is coincidental
/// — this operates on luma, the mixer above on HSL lightness, per ADR 0031.
pub(crate) fn color_grading(px: &mut Pixels, grading: &ColorGrading) {
    let tints = [
        zone_tint(&grading.shadows),
        zone_tint(&grading.midtones),
        zone_tint(&grading.highlights),
    ];
    let lum_shifts = [
        f32::from(grading.shadows.luminance as i16) / 100.0,
        f32::from(grading.midtones.luminance as i16) / 100.0,
        f32::from(grading.highlights.luminance as i16) / 100.0,
    ];
    let balance = f32::from(grading.balance as i16) / 100.0;
    let blending = f32::from(grading.blending as i16) / 100.0;

    in_display(px, |px| {
        par_rows(px, |row| {
            for rgb in row.chunks_exact_mut(3) {
                preserving_headroom(rgb, |rgb| {
                    let weights = zone_weights(luma(rgb), balance, blending);
                    let mut tint = [0.0f32; 3];
                    let mut lum_shift = 0.0f32;
                    for zone in 0..3 {
                        for c in 0..3 {
                            tint[c] += tints[zone][c] * weights[zone];
                        }
                        lum_shift += lum_shifts[zone] * weights[zone];
                    }
                    let delta_l = lum_shift * MAX_LUM_SHIFT;
                    for c in 0..3 {
                        rgb[c] =
                            (rgb[c] + delta_l + tint[c] * GRADING_TINT_STRENGTH).clamp(0.0, 1.0);
                    }
                });
            }
        });
    });
}

/// The color a zone tints toward: a delta from neutral gray, scaled by the
/// zone's saturation — `[0, 0, 0]` at `saturation = 0`, so a zone with no
/// saturation set contributes nothing regardless of how much weight a pixel
/// gives it (required for the neutral-settings-skip-the-operator rule).
pub(crate) fn zone_tint(zone: &ColorGradingZone) -> [f32; 3] {
    let sat = f32::from(zone.saturation as i16) / 100.0;
    if sat <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    let pure = hsl_to_rgb(f32::from(zone.hue as i16), 1.0, 0.5);
    [
        (pure[0] - 0.5) * sat,
        (pure[1] - 0.5) * sat,
        (pure[2] - 0.5) * sat,
    ]
}

/// Shadow/midtone/highlight membership weights for a pixel of luma `l`,
/// always summing to 1. Two smoothstep transitions (shadow→mid, mid→
/// highlight) separated by a fixed gap so they never overlap; `balance`
/// (already scaled to [-1, 1]) shifts where the gap sits, `blending`
/// (already scaled to [0, 1]) widens both transitions.
pub(crate) fn zone_weights(l: f32, balance: f32, blending: f32) -> [f32; 3] {
    const GAP: f32 = 0.3;
    let center = (0.5 - balance * 0.25).clamp(0.05, 0.95);
    let half_width = 0.02 + blending * 0.13;
    let b1 = center - GAP / 2.0;
    let b2 = center + GAP / 2.0;
    let shadow = 1.0 - smoothstep01(((l - (b1 - half_width)) / (2.0 * half_width)).clamp(0.0, 1.0));
    let highlight = smoothstep01(((l - (b2 - half_width)) / (2.0 * half_width)).clamp(0.0, 1.0));
    let midtone = (1.0 - shadow - highlight).max(0.0);
    let sum = shadow + midtone + highlight;
    if sum <= 1e-6 {
        [0.0, 1.0, 0.0]
    } else {
        [shadow / sum, midtone / sum, highlight / sum]
    }
}
