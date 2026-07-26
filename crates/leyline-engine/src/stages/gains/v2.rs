//! White balance and exposure v2 (ADR 0013) — rank 40.
//!
//! Identical to [`super::v1`] except that both conversions to and from
//! linear light go through the interpolated lookup tables of
//! [`crate::stages::kernel::v1::tables`]. Current since process 2, and the
//! only stage version in the whole history that changed the rendering of a
//! setting that already existed.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::WhiteBalance;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{blackbody_rgb, lookup, par_rows, tables};

/// Applies white balance and exposure as per-channel gains in linear light.
pub(crate) fn linear_gains(px: &mut Pixels, wb: Option<&WhiteBalance>, exposure_ev: f64) {
    let mut gains = [1.0f64; 3];
    if let Some(wb) = wb {
        let reference = blackbody_rgb(6500.0);
        let target = blackbody_rgb(f64::from(wb.temperature));
        for c in 0..3 {
            gains[c] = (reference[c] / target[c]).clamp(0.1, 10.0);
        }
        // Normalize on green so white balance alone does not change exposure.
        let green = gains[1];
        for gain in &mut gains {
            *gain /= green;
        }
        gains[1] *= 2.0f64.powf(-f64::from(wb.tint) / 200.0);
    }
    let gain = 2.0f64.powf(exposure_ev);
    let gains = gains.map(|g| (g * gain) as f32);

    let (to_linear, to_srgb) = tables();
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            for (sample, gain) in rgb.iter_mut().zip(gains) {
                *sample = lookup(to_srgb, (lookup(to_linear, *sample) * gain).clamp(0.0, 1.0));
            }
        }
    });
}
