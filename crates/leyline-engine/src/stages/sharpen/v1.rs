//! Sharpening v1 — rank 190. Process 1's unsharp mask on the luma plane.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{add_luma_delta, gaussian_blur, luma_plane};

/// Unsharp mask on the luma plane only, so sharpening never fringes colors.
pub(crate) fn sharpen(px: &mut Pixels, amount: i32, radius: f64) {
    let k = f32::from(amount as i16) / 100.0;
    let plane = luma_plane(px);
    let blurred = gaussian_blur(&plane, px.width as usize, px.height as usize, radius as f32);
    add_luma_delta(px, |i| k * (plane[i] - blurred[i]));
}
