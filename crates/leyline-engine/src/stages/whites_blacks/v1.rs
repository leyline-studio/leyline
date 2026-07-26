//! Whites and blacks v1 — rank 70. Process 1's endpoint remapping.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{display_curve, in_display};

/// Endpoint remapping: positive `whites` brightens by lowering the white
/// point, positive `blacks` lifts the black point (negative values crush).
pub(crate) fn whites_blacks(px: &mut Pixels, whites: i32, blacks: i32) {
    let white = 1.0 - f32::from(whites as i16) / 100.0 * 0.25;
    let black = -f32::from(blacks as i16) / 100.0 * 0.25;
    let scale = 1.0 / (white - black);
    in_display(px, |px| {
        display_curve(px, |x| ((x - black) * scale).max(0.0));
    });
}
