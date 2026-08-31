//! `grain` v1 (ADR 0090 §3): film grain, deterministic by construction.
//!
//! **Frozen.** Published, therefore immutable: a revision citing
//! `grain: 1` renders through this code forever (`docs/pipeline.md` §5.1).
//! Changing the field means a `v2`.
//!
//! This is the one operator whose whole purpose is to add *randomness* to
//! pixels §5.1 promises will be identical in ten years, so the way it gets
//! its randomness is the design. There is no generator state, no seed, no
//! clock and no dependence on how rayon scheduled the rows: the value at a
//! lattice point is an integer hash of that point's coordinates, so the
//! field is *recomputed* identically rather than replayed.
//!
//! No seed is stored either, and that is a decision rather than an omission
//! (ADR 0090 §3): a seed's only possible meaning is "give me a different
//! randomness", and paying for it would mean a field in every revision,
//! every preset, every cache fingerprint and every golden entry.

use leyline_core::Grain;
use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel;

/// Adds monochromatic value noise on the display axis.
///
/// Three things about *where* it is added, all of them deliberate:
///
/// * **on the display axis** (`kernel::v1::in_display`), because grain is a
///   statement about perceived texture — added in linear light it would be
///   invisible in the shadows and enormous in the highlights;
/// * **monochromatically**, the same offset on all three channels: coloured
///   noise is what `noise_color` spends its time removing (ADR 0072);
/// * **weighted by `4·L·(1 − L)`, clamped at zero**, so it fades out into
///   blacks and into anything at or above white. Grain that survives into a
///   specular highlight reads as a hot sensor, not as film — and since the
///   working buffer is unbounded above (ADR 0044), that clamp is also what
///   keeps a highlight two stops over white from being dusted with noise.
///
/// The lattice is measured in **full-resolution pixels**: coordinates are
/// divided by ADR 0041's `scale` before hashing, so a display preview
/// samples the same field as the export instead of a different field at the
/// same nominal size. It samples it more coarsely — grain is judged at 1:1.
pub(crate) fn grain(px: &mut Pixels, settings: &Grain, scale: f32) {
    let amplitude = (settings.amount as f32 / 100.0) * MAX_AMPLITUDE;
    // 1 to 16 full-resolution pixels between lattice points.
    let cell = 1.0 + (settings.size as f32 / 100.0).clamp(0.0, 1.0) * 15.0;
    let fine_weight = (settings.roughness as f32 / 100.0).clamp(0.0, 1.0);
    let scale = if scale > 0.0 { scale } else { 1.0 };

    kernel::v1::in_display(px, |px| {
        let width = px.width;
        px.data
            .par_chunks_mut(width as usize * 3)
            .enumerate()
            .for_each(|(y, row)| {
                // Back to full-resolution coordinates before anything is
                // hashed: that is what makes the preview and the export
                // sample one field.
                let fy = (y as f32 + 0.5) / scale;
                for (x, rgb) in row.chunks_exact_mut(3).enumerate() {
                    let fx = (x as f32 + 0.5) / scale;
                    let coarse = value_noise(fx / cell, fy / cell);
                    let fine = value_noise(2.0 * fx / cell, 2.0 * fy / cell);
                    let noise = coarse * (1.0 - fine_weight) + fine * fine_weight;
                    for sample in rgb {
                        // Triangular fade, peaking at mid-grey, zero at
                        // black and at (or above) white.
                        let weight = (4.0 * *sample * (1.0 - *sample)).max(0.0);
                        *sample += noise * amplitude * weight;
                    }
                }
            });
    });
}

/// Grain amplitude on the display axis at `amount: 100`, mid-grey.
///
/// Frozen with the version, like every other constant here: it is what the
/// slider's top end *means*.
const MAX_AMPLITUDE: f32 = 0.09;

/// Value noise in [-1, 1]: bilinear interpolation between hashed lattice
/// points.
fn value_noise(x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    // Hermite weights, so the field has no visible lattice creases the way
    // straight bilinear interpolation does.
    let (wx, wy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let top = lerp(lattice(ix, iy), lattice(ix + 1, iy), wx);
    let bottom = lerp(lattice(ix, iy + 1), lattice(ix + 1, iy + 1), wx);
    lerp(top, bottom, wy)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The value at one lattice point: a pure function of its coordinates.
///
/// An integer avalanche hash (the finalizer shape `wyhash` and friends use),
/// then the top 24 bits mapped to [-1, 1]. Integer arithmetic throughout,
/// wrapping deliberately, so the field is bit-identical on every platform —
/// a float hash would not be (`docs/pipeline.md` §5.1).
fn lattice(x: i32, y: i32) -> f32 {
    let mut h = (x as u32 as u64) | ((y as u32 as u64) << 32);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    ((h >> 40) as f32 / 8_388_608.0) - 1.0
}
