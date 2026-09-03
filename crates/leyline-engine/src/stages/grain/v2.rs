//! `grain` v2 (ADR 0118): v1's grain, plus layers that disagree.
//!
//! **Frozen.** Published, therefore immutable: a revision citing
//! `grain: 2` renders through this code forever (`docs/pipeline.md` §5.1).
//! Changing the field means a `v3`.
//!
//! **With `color` at 0, this calls v1.** Not "computes the same thing" —
//! calls it, so bit-identity is a property of the control flow rather than
//! an argument about floating point (ADR 0118 §3). That is
//! [`super::super::tone_curve::v2`]'s arrangement, for the same reason, and
//! the golden manifest holds the two together: the `grain` case carries the
//! same digest under v1 and under v2.
//!
//! The monochrome field comes from v1's own `value_noise`, reused rather
//! than restated — the two versions must agree on it exactly, and the
//! surest way to agree with code is to run it. The three chroma fields do
//! *not* have to agree with anything, only to be independent of each other
//! and of the mono field, so they get their own salted hash here.

use leyline_core::Grain;
use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel;

use super::v1::{MAX_AMPLITUDE, value_noise};

/// Adds grain whose three layers disagree by `settings.color`.
///
/// Everything about *where* the grain lands is v1's and unchanged: the
/// display axis, the lattice measured in full-resolution pixels, the
/// `4·L·(1 − L)` triangular fade that keeps it out of blacks and out of
/// anything at or above white.
///
/// What is new is the offset itself. The mono field keeps its full
/// amplitude at every setting, and a chroma-only deviation is added on top:
///
/// ```text
/// offset_c = mono + k · (n_c − (n_r + n_g + n_b)/3)
/// ```
///
/// The added term sums to zero across the three channels by construction,
/// so it carries no luminance whatever `k` is. That is the decision of
/// ADR 0118 §2, and it is what makes the slider mean *how much the layers
/// disagree* rather than *how much grey I traded for colour* — a cross-fade
/// between one shared field and three independent ones would be 30 %
/// quieter in the middle of its travel than at either end.
pub(crate) fn grain(px: &mut Pixels, settings: &Grain, scale: f32) {
    if settings.color == 0 {
        // The identity of ADR 0118 §3, made structural.
        super::v1::grain(px, settings, scale);
        return;
    }

    let amplitude = (settings.amount as f32 / 100.0) * MAX_AMPLITUDE;
    // 1 to 16 full-resolution pixels between lattice points.
    let cell = 1.0 + (settings.size as f32 / 100.0).clamp(0.0, 1.0) * 15.0;
    let fine_weight = (settings.roughness as f32 / 100.0).clamp(0.0, 1.0);
    let colour = (settings.color as f32 / 100.0).clamp(0.0, 1.0);
    let scale = if scale > 0.0 { scale } else { 1.0 };

    kernel::v1::in_display(px, |px| {
        let width = px.width;
        px.data
            .par_chunks_mut(width as usize * 3)
            .enumerate()
            .for_each(|(y, row)| {
                // Back to full-resolution coordinates before anything is
                // hashed, exactly as in v1: that is what makes the preview
                // and the export sample one field.
                let fy = (y as f32 + 0.5) / scale;
                for (x, rgb) in row.chunks_exact_mut(3).enumerate() {
                    let fx = (x as f32 + 0.5) / scale;
                    let (u, v) = (fx / cell, fy / cell);

                    let mono = octaves(u, v, fine_weight, value_noise);
                    let layers: [f32; 3] = std::array::from_fn(|c| {
                        octaves(u, v, fine_weight, |x, y| salted_noise(x, y, c as u32))
                    });
                    let mean = (layers[0] + layers[1] + layers[2]) / 3.0;

                    for (c, sample) in rgb.iter_mut().enumerate() {
                        let noise = mono + colour * (layers[c] - mean);
                        // Triangular fade, peaking at mid-grey, zero at
                        // black and at (or above) white.
                        let weight = (4.0 * *sample * (1.0 - *sample)).max(0.0);
                        *sample += noise * amplitude * weight;
                    }
                }
            });
    });
}

/// The two-octave mix `roughness` controls, over whichever field is handed
/// in — v1's for the grey, a salted one for each layer.
fn octaves(u: f32, v: f32, fine_weight: f32, field: impl Fn(f32, f32) -> f32) -> f32 {
    let coarse = field(u, v);
    let fine = field(2.0 * u, 2.0 * v);
    coarse * (1.0 - fine_weight) + fine * fine_weight
}

/// One layer's field: v1's interpolation over a lattice hashed with a
/// per-channel salt.
///
/// The salt goes into the *hash*, not into the coordinates. Offsetting the
/// coordinates instead would have been shorter and would have cost real
/// precision: a lattice coordinate large enough to be independent of the
/// mono field is large enough for `f32` to quantise the smoothstep weights
/// that make the field smooth.
fn salted_noise(x: f32, y: f32, channel: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (wx, wy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let top = lerp(lattice(ix, iy, channel), lattice(ix + 1, iy, channel), wx);
    let bottom = lerp(
        lattice(ix, iy + 1, channel),
        lattice(ix + 1, iy + 1, channel),
        wx,
    );
    lerp(top, bottom, wy)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The value at one lattice point of one layer: a pure function of its
/// coordinates and the layer's index.
///
/// v1's avalanche hash with the salt folded in by a golden-ratio multiply
/// before the finalizer, so two layers of the same lattice point are as
/// unrelated as two different lattice points. Integer arithmetic
/// throughout, wrapping deliberately, so the field is bit-identical on
/// every platform (`docs/pipeline.md` §5.1).
fn lattice(x: i32, y: i32, channel: u32) -> f32 {
    let mut h = (x as u32 as u64) | ((y as u32 as u64) << 32);
    h ^= (channel as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    ((h >> 40) as f32 / 8_388_608.0) - 1.0
}
