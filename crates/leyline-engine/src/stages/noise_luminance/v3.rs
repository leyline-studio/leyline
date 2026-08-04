//! Luminance noise reduction v3 — rank 5, in front of the whole pipeline.
//! The threshold comes from the sensor's measured noise profile (ADR 0072).
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).
//!
//! Two things changed against `v2`, and the second follows from the first:
//!
//! * the threshold is no longer three constants for every camera on earth
//!   but `k · 6 σ_l · σ(x)`, where σ comes from the measured profile of this
//!   body at this sensitivity and grows with the light the photosite
//!   received;
//! * the stage runs at rank 5 rather than 170, and in linear light rather
//!   than on the display axis. A model measured on sensor counts means
//!   nothing once exposure, contrast and the tone curve have run, and the
//!   display axis was only ever a proxy for the level dependence the model
//!   now carries explicitly (ADR 0072 §4).

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{add_luma_delta, luma_plane};
use crate::stages::kernel::v2::levels_at_scale;
use crate::stages::kernel::v3::{
    NoiseModel, SIGMAS_LUMA, luma_terms, sigma_plane, wavelet_denoise_adaptive,
};

/// Denoises the luma plane and shifts the pixels by the difference; chroma
/// is untouched, as in `v1` and `v2`. `scale` is the proxy reduction factor
/// (ADR 0041), which sets both how many levels are analysed (ADR 0046 §5)
/// and how much of the measured noise survived the reduction (ADR 0072 §3).
pub(crate) fn luminance_noise_reduction(
    px: &mut Pixels,
    strength: i32,
    scale: f32,
    model: &NoiseModel,
) {
    let k = f32::from(strength as i16) / 100.0;
    let (w, h) = (px.width as usize, px.height as usize);
    let plane = luma_plane(px);
    let (a, b) = luma_terms(model);
    let sigma = sigma_plane(&plane, a, b, scale);
    let mut denoised = plane.clone();
    wavelet_denoise_adaptive(
        &mut denoised,
        w,
        h,
        k * SIGMAS_LUMA,
        &sigma,
        levels_at_scale(scale),
    );
    add_luma_delta(px, |i| denoised[i] - plane[i]);
}
