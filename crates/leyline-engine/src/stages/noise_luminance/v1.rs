//! Luminance noise reduction v1 — rank 170. Process 1's luma smoothing.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{add_luma_delta, gaussian_blur, luma_plane};

/// Blends the luma plane toward its Gaussian blur; chroma is untouched.
/// `scale` is the proxy reduction factor (ADR 0041): the sigma is in
/// pixels, so it shrinks with the buffer.
pub(crate) fn luminance_noise_reduction(px: &mut Pixels, strength: i32, scale: f32) {
    let k = f32::from(strength as i16) / 100.0;
    let plane = luma_plane(px);
    let blurred = gaussian_blur(
        &plane,
        px.width as usize,
        px.height as usize,
        k * 2.0 * scale,
    );
    add_luma_delta(px, |i| k * (blurred[i] - plane[i]));
}
