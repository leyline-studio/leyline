//! Luminance noise reduction v2 — rank 170. Edge-preserving denoising by à
//! trous wavelet soft thresholding (ADR 0046).
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).
//!
//! What changed against `v1`, which stays exactly where it is: `v1` blends
//! the luma plane toward a Gaussian blur of it, so a contour and a flat area
//! get the same treatment and the slider trades detail for noise one for one.
//! Here the plane is split into scales and each scale is shrunk by a
//! threshold, which leaves the large coefficients — contours, texture —
//! standing.

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{add_luma_delta, in_display, luma_plane};
use crate::stages::kernel::v2::{levels_at_scale, wavelet_denoise};

/// Threshold scale for the luminance plane at full strength (ADR 0046 §3).
/// Half of `noise_color`'s, luminance detail being what the eye reads.
const BASE_LUMA: f32 = 0.05;

/// Denoises the luma plane and shifts the pixels by the difference; chroma is
/// untouched, as in `v1`. `scale` is the proxy reduction factor (ADR 0041):
/// it sets how many detail levels are analysed rather than a radius, since
/// this operator's scales are dyadic (ADR 0046 §5).
pub(crate) fn luminance_noise_reduction(px: &mut Pixels, strength: i32, scale: f32) {
    let k = f32::from(strength as i16) / 100.0;
    in_display(px, |px| {
        let (w, h) = (px.width as usize, px.height as usize);
        let plane = luma_plane(px);
        let mut denoised = plane.clone();
        wavelet_denoise(&mut denoised, w, h, k * BASE_LUMA, levels_at_scale(scale));
        add_luma_delta(px, |i| denoised[i] - plane[i]);
    });
}
