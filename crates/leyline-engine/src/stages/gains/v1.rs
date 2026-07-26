//! White balance and exposure v1 — rank 40.
//!
//! Process 1's original: the sRGB transfer functions are evaluated exactly,
//! `powf` per sample. Superseded for rendering speed by [`super::v2`] from
//! process 2 on (ADR 0013), and kept because process-1 revisions still cite
//! it.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::WhiteBalance;

use crate::pixels::{Pixels, linear_to_srgb, srgb_to_linear};
use crate::stages::kernel::v1::{blackbody_rgb, par_rows};

/// Applies white balance and exposure as per-channel gains in linear light.
///
/// The white balance of process 1 is *relative*: gains are the ratio of the
/// blackbody colors of a fixed D65-like pivot (6500 K) and of the requested
/// temperature, normalized on green. `{ temperature: 6500, tint: 0 }` is
/// therefore a no-op — raising the temperature warms the image, lowering it
/// cools it. Positive tint shifts toward magenta, negative toward green.
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
                *sample = linear_to_srgb((srgb_to_linear(*sample) * gain).clamp(0.0, 1.0));
            }
        }
    });
}
