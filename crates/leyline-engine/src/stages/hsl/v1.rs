//! HSL mixer v1 (ADR 0032) — rank 140.
//!
//! Introduced by process 9. Eight hue bands, each with hue/saturation/
//! luminance offsets, blended smoothly between neighboring band centers so
//! no band edge is visible in a gradient.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::HslBand;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{hsl_to_rgb, par_rows, rgb_to_hsl, smoothstep01};

/// Fixed hue-band centers in degrees, in the module's declared band order
/// (red, orange, yellow, green, aqua, blue, purple, magenta) — chosen to
/// match each band's everyday position on the hue wheel, not evenly spaced.
pub(crate) const HSL_BAND_CENTERS_DEG: [f32; 8] =
    [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 275.0, 315.0];

/// Maximum hue shift, in degrees, a slider at its ±100 extreme applies.
pub(crate) const MAX_HUE_SHIFT_DEG: f32 = 30.0;

/// Maximum luminance shift a slider at its ±100 extreme applies, as a
/// fraction of the full [0, 1] lightness range.
pub(crate) const MAX_LUM_SHIFT: f32 = 0.25;

/// Applies the 8-band HSL mixer: every pixel's hue picks up a weighted
/// blend of its two nearest bands' hue/saturation/luminance offsets. The
/// two weights always sum to 1 ([`hue_band_neighbors`]/[`smoothstep01`]), so
/// a pixel exactly on a band's center is fully that band and a pixel
/// between two centers blends smoothly, never double-counting or losing
/// coverage. A neutral `bands` (checked by the caller) is skipped entirely,
/// so this is only ever called when at least one slider is non-zero.
pub(crate) fn hsl_mixer(px: &mut Pixels, bands: &[HslBand; 8]) {
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let (h, s, l) = rgb_to_hsl(rgb);
            let (i0, i1, t) = hue_band_neighbors(h);
            let w1 = smoothstep01(t);
            let w0 = 1.0 - w1;
            let hue_shift = (f32::from(bands[i0].hue as i16) * w0
                + f32::from(bands[i1].hue as i16) * w1)
                / 100.0
                * MAX_HUE_SHIFT_DEG;
            let sat_shift = (f32::from(bands[i0].saturation as i16) * w0
                + f32::from(bands[i1].saturation as i16) * w1)
                / 100.0;
            let lum_shift = (f32::from(bands[i0].luminance as i16) * w0
                + f32::from(bands[i1].luminance as i16) * w1)
                / 100.0;
            let new_h = (h + hue_shift).rem_euclid(360.0);
            let new_s = (s * (1.0 + sat_shift)).clamp(0.0, 1.0);
            let new_l = (l + lum_shift * MAX_LUM_SHIFT).clamp(0.0, 1.0);
            rgb.copy_from_slice(&hsl_to_rgb(new_h, new_s, new_l));
        }
    });
}

/// Finds the two hue bands adjacent to `hue_deg` and how far between them it
/// falls: `(i0, i1, t)`, indices into [`HSL_BAND_CENTERS_DEG`] and `t` in
/// `[0, 1]` from `i0`'s center (`t = 0`) to `i1`'s (`t = 1`), the short way
/// around the hue circle. The bands' spans wrap exactly once around the
/// full circle, so exactly one pair always matches.
pub(crate) fn hue_band_neighbors(hue_deg: f32) -> (usize, usize, f32) {
    let h = hue_deg.rem_euclid(360.0);
    let bands = HSL_BAND_CENTERS_DEG.len();
    for (i, &start) in HSL_BAND_CENTERS_DEG.iter().enumerate() {
        let j = (i + 1) % bands;
        let end = if j == 0 {
            HSL_BAND_CENTERS_DEG[j] + 360.0
        } else {
            HSL_BAND_CENTERS_DEG[j]
        };
        let probe = if h < start { h + 360.0 } else { h };
        if probe >= start && probe <= end {
            return (i, j, (probe - start) / (end - start));
        }
    }
    unreachable!("HSL_BAND_CENTERS_DEG spans exactly one full turn of the hue circle")
}
