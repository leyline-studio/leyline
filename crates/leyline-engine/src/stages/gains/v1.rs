//! White balance and exposure v1 — rank 40.
//!
//! Per-channel gains in linear light: white balance as the ratio of two
//! blackbody colors, exposure as a power of two.
//!
//! Since ADR 0044 the working buffer *is* linear light, so this is what it
//! always should have been — one multiply per sample. The conversions in
//! and out of linear that used to bracket it, and the clamp at 1 that
//! destroyed a stop of highlights on the way, are both gone: +1 EV followed
//! by −1 EV now returns the image it started from.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::WhiteBalance;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{blackbody_rgb, par_rows};

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

    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            for (sample, gain) in rgb.iter_mut().zip(gains) {
                *sample = (*sample * gain).max(0.0);
            }
        }
    });
}
