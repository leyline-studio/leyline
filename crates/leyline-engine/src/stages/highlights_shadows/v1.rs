//! Highlights and shadows v1 — rank 60. Process 1's luma-masked pair.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use crate::pixels::{Pixels, luma};
use crate::stages::kernel::v1::{in_display, par_rows};

/// Luma-masked tone adjustments: `shadows` acts on dark pixels with weight
/// `(1 − L)²`, `highlights` on bright pixels with weight `L²`.
pub(crate) fn highlights_shadows(px: &mut Pixels, highlights: i32, shadows: i32) {
    let h = f32::from(highlights as i16) / 100.0;
    let s = f32::from(shadows as i16) / 100.0;
    in_display(px, |px| {
        par_rows(px, |row| {
            for rgb in row.chunks_exact_mut(3) {
                // The masks are weights on a [0, 1] axis; a highlight in
                // the headroom is simply "as bright as white" for the
                // purpose of choosing them, and keeps its own value below.
                let l = luma(rgb).clamp(0.0, 1.0);
                let delta = 0.5 * (s * (1.0 - l) * (1.0 - l) + h * l * l);
                for sample in rgb {
                    let x = *sample;
                    let moved = if x > 1.0 {
                        // Same gain the curve gives white, so the operator
                        // is continuous at 1 and headroom survives it.
                        x * if delta >= 0.0 { 1.0 } else { 1.0 + delta }
                    } else if delta >= 0.0 {
                        x + delta * (1.0 - x)
                    } else {
                        x + delta * x
                    };
                    *sample = moved.max(0.0);
                }
            }
        });
    });
}
