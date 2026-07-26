//! Color noise reduction v1 — rank 180. Process 1's chroma smoothing.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{gaussian_blur, in_display, luma_plane};

/// Blends the chroma planes (per-channel deviation from luma) toward their
/// Gaussian blur.
pub(crate) fn color_noise_reduction(px: &mut Pixels, strength: i32, scale: f32) {
    let k = f32::from(strength as i16) / 100.0;
    in_display(px, |px| smooth_chroma(px, k, scale));
}

/// The smoothing itself, on a display-axis buffer: chroma is a channel's
/// deviation from luma, which is only a meaningful quantity on a bounded
/// axis.
fn smooth_chroma(px: &mut Pixels, k: f32, scale: f32) {
    let (w, h) = (px.width as usize, px.height as usize);
    let plane = luma_plane(px);
    for channel in 0..3 {
        let chroma: Vec<f32> = (0..w * h)
            .map(|i| px.data[i * 3 + channel] - plane[i])
            .collect();
        let blurred = gaussian_blur(&chroma, w, h, k * 3.0 * scale);
        px.data
            .par_chunks_mut(w * 3)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..w {
                    let i = y * w + x;
                    let smoothed = chroma[i] + k * (blurred[i] - chroma[i]);
                    row[x * 3 + channel] = (plane[i] + smoothed).max(0.0);
                }
            });
    }
}
