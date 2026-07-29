//! Color noise reduction v2 — rank 180. Edge-preserving denoising by à trous
//! wavelet soft thresholding (ADR 0046).
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).
//!
//! Same substitution as `noise_luminance::v2` — a Gaussian blur becomes a
//! multi-scale shrinkage — on the same quantity `v1` operated on: each
//! channel's deviation from luma. The chroma blotches of a high-ISO frame are
//! several pixels wide, which is exactly what a single-sigma blur could not
//! reach without dragging the whole image with it.

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{in_display, luma_plane};
use crate::stages::kernel::v2::{levels_at_scale, wavelet_denoise};

/// Threshold scale for the chroma planes at full strength (ADR 0046 §3).
/// More than double `noise_luminance`'s: chrominance is spatially smooth
/// almost everywhere, so an aggressive threshold costs little there.
const BASE_CHROMA: f32 = 0.12;

/// Denoises the three chroma planes and recomposes with the untouched luma.
pub(crate) fn color_noise_reduction(px: &mut Pixels, strength: i32, scale: f32) {
    let k = f32::from(strength as i16) / 100.0;
    let levels = levels_at_scale(scale);
    in_display(px, |px| denoise_chroma(px, k * BASE_CHROMA, levels));
}

/// The shrinkage itself, on a display-axis buffer: chroma is a channel's
/// deviation from luma, which is only a meaningful quantity on a bounded
/// axis.
fn denoise_chroma(px: &mut Pixels, base: f32, levels: usize) {
    let (w, h) = (px.width as usize, px.height as usize);
    let plane = luma_plane(px);
    for channel in 0..3 {
        let mut chroma: Vec<f32> = (0..w * h)
            .map(|i| px.data[i * 3 + channel] - plane[i])
            .collect();
        wavelet_denoise(&mut chroma, w, h, base, levels);
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
