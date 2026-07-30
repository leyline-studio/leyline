//! Creative LUT v1 (ADR 0053) — rank 165, the last color decision.
//!
//! A `.cube` is written for display-referred values in `[0, 1]`: its author set
//! it up looking at an image, not at a linear buffer. So the samples go to the
//! display axis, through the table, and back (ADR 0053 §3) — the same axis the
//! tone operators and the range masks work on.
//!
//! What that costs, deliberately: headroom above white is clamped to 1 on the
//! way in, because the table says nothing about what lies beyond it. A LUT is an
//! output look; the highlights above white belong to `output_rendering`, which
//! runs after.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1) — the
//! tetrahedral interpolation ADR 0053 §5 leaves open would be exactly that.

use leyline_color::CubeLut;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{in_display, par_rows};

/// Applies `lut` at `strength` percent.
pub(crate) fn apply(px: &mut Pixels, lut: &CubeLut, strength: i32) {
    let strength = (f64::from(strength.clamp(0, 100)) / 100.0) as f32;
    if strength == 0.0 {
        return;
    }
    in_display(px, |px| {
        par_rows(px, |row| {
            for rgb in row.chunks_exact_mut(3) {
                // Clamped to the display range: see the module docs.
                let input = [
                    rgb[0].clamp(0.0, 1.0),
                    rgb[1].clamp(0.0, 1.0),
                    rgb[2].clamp(0.0, 1.0),
                ];
                let looked = lut.sample(input);
                for (channel, (sample, graded)) in rgb.iter_mut().zip(looked).enumerate() {
                    // The blend is against the clamped input rather than the
                    // raw sample, so a highlight above white does not come back
                    // as a mix of "above white" and "what the LUT said about
                    // white" — it lands where the LUT put white, scaled by the
                    // dose (ADR 0053 §2).
                    *sample = input[channel] * (1.0 - strength) + graded * strength;
                }
            }
        });
    });
}
