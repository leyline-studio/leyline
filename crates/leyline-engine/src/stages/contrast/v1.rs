//! Contrast v1 — rank 50. Process 1's S-curve around middle gray.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{display_curve, in_display};

/// S-curve around middle gray, per channel, on the display axis
/// ([`in_display`]): a contrast slider is a statement about perceived
/// lightness, and the same curve applied to linear light would crush the
/// shadows and barely reach the highlights. The curve itself is unchanged,
/// so a given slider value means what it always meant.
pub(crate) fn contrast(px: &mut Pixels, amount: i32) {
    let k = f32::from(amount as i16) / 100.0;
    in_display(px, |px| {
        display_curve(px, |x| {
            if k >= 0.0 {
                let s = x * x * (3.0 - 2.0 * x);
                ((1.0 - k) * x + k * s).max(0.0)
            } else {
                let flat = 0.25 + 0.5 * x;
                ((1.0 + k) * x - k * flat).max(0.0)
            }
        });
    });
}
