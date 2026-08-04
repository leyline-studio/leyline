//! Color noise reduction v3 — rank 6, just behind `noise_luminance::v3`.
//! The threshold comes from the sensor's measured noise profile (ADR 0072).
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).
//!
//! Same substitution as `noise_luminance::v3` — a flat threshold becomes a
//! measured one — on the same quantity `v1` and `v2` operated on: each
//! channel's deviation from luma. Two differences with `v2` deserve naming:
//!
//! * this runs in **linear light**. `v2` needed the display axis because a
//!   deviation from luma is only comparable across the tonal range on a
//!   bounded one; here the threshold itself follows the signal, which is the
//!   dependence that axis was standing in for (ADR 0072 §4);
//! * the σ of a chroma plane is not the σ of a channel. It is what the
//!   subtraction `y_k − L` makes of the three channels' variances, which
//!   `kernel::v3::chroma_terms` derives rather than approximates.

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::luma_plane;
use crate::stages::kernel::v2::levels_at_scale;
use crate::stages::kernel::v3::{
    NoiseModel, SIGMAS_CHROMA, chroma_terms, sigma_plane, wavelet_denoise_adaptive,
};

/// Denoises the three chroma planes and recomposes with the untouched luma.
pub(crate) fn color_noise_reduction(
    px: &mut Pixels,
    strength: i32,
    scale: f32,
    model: &NoiseModel,
) {
    let k = f32::from(strength as i16) / 100.0;
    let gain = k * SIGMAS_CHROMA;
    if gain <= 0.0 {
        return;
    }
    let levels = levels_at_scale(scale);
    let (w, h) = (px.width as usize, px.height as usize);
    let plane = luma_plane(px);
    for channel in 0..3 {
        let (a, b) = chroma_terms(model, channel);
        let sigma = sigma_plane(&plane, a, b, scale);
        let mut chroma: Vec<f32> = (0..w * h)
            .map(|i| px.data[i * 3 + channel] - plane[i])
            .collect();
        wavelet_denoise_adaptive(&mut chroma, w, h, gain, &sigma, levels);
        px.data
            .par_chunks_mut(w * 3)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..w {
                    let i = y * w + x;
                    row[x * 3 + channel] = (plane[i] + chroma[i]).max(0.0);
                }
            });
    }
}
