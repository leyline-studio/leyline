//! Kernel v2 — the à trous wavelet transform and its soft thresholding, the
//! body `noise_luminance::v2` and `noise_color::v2` share (ADR 0046 §6).
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing a stage version that calls
//! this code renders through exactly this code, forever. A change of
//! rendering is a new version module next to this one, never an edit here
//! (ADR 0042 §1).
//!
//! `kernel::v1` is *not* superseded — nothing here replaces a primitive that
//! lives there, and the v2 stages keep calling `in_display`, `luma_plane`
//! and `add_luma_delta` from it. This module holds only what did not exist
//! before.

use rayon::prelude::*;

/// Number of detail levels analysed on a full-resolution render (ADR 0046 §3).
pub(crate) const LEVELS: usize = 4;

/// Standard deviation of the detail coefficients each level of the transform
/// below produces from unit-variance white Gaussian noise. The threshold
/// profile of ADR 0046 §3: noise energy collapses by roughly a factor four
/// per level, so a flat threshold would either spare the finest grain or
/// flatten the coarse structure.
const LEVEL_SIGMA: [f32; LEVELS] = [0.890, 0.201, 0.086, 0.041];

/// The B3-spline analysis kernel, `[1, 4, 6, 4, 1] / 16`, as five weights
/// applied at spacings `-2h, -h, 0, +h, +h*2`.
const B3: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];

/// How many detail levels a plane reduced by `scale` gets (ADR 0046 §5).
///
/// Level `l` analyses structures of about `2^l` pixels, so a preview reduced
/// four times must drop two levels to look at the same structures *of the
/// photograph* as the full render does. A radius-based stage expresses the
/// same idea by multiplying by `scale`; a multi-scale one expresses it by
/// counting levels.
pub(crate) fn levels_at_scale(scale: f32) -> usize {
    if !scale.is_finite() || scale <= 0.0 || scale >= 1.0 {
        return LEVELS;
    }
    let dropped = (-scale.log2()).floor() as usize;
    LEVELS.saturating_sub(dropped).max(1)
}

/// Denoises one plane in place by à trous wavelet soft thresholding
/// (ADR 0046 §2): decompose into `levels` detail planes plus a residual,
/// shrink each detail by `base * LEVEL_SIGMA[l]`, recompose.
///
/// The residual is never thresholded, so the operator cannot move the
/// image's tonality — only the detail it is handed.
pub(crate) fn wavelet_denoise(
    plane: &mut [f32],
    width: usize,
    height: usize,
    base: f32,
    levels: usize,
) {
    if base <= 0.0 || levels == 0 || width == 0 || height == 0 {
        return;
    }
    // `current` is the running approximation a_l; `plane` accumulates the
    // thresholded details and finally has the residual added back.
    let mut current = plane.to_vec();
    for value in plane.iter_mut() {
        *value = 0.0;
    }
    for level in 0..levels {
        let spacing = 1usize << level;
        let next = convolve_b3(&current, width, height, spacing);
        let threshold = base * LEVEL_SIGMA[level.min(LEVELS - 1)];
        plane
            .par_iter_mut()
            .zip(current.par_iter())
            .zip(next.par_iter())
            .for_each(|((out, &coarse_in), &coarse_out)| {
                *out += soft_threshold(coarse_in - coarse_out, threshold);
            });
        current = next;
    }
    plane
        .par_iter_mut()
        .zip(current.par_iter())
        .for_each(|(out, &residual)| *out += residual);
}

/// `sign(d) · max(|d| − t, 0)` — the shrinkage itself.
///
/// Soft rather than hard: a coefficient just above the threshold keeps a
/// value close to zero instead of jumping to its full magnitude, which is
/// what stops the operator from stippling a smooth gradient with the few
/// coefficients that happened to survive.
fn soft_threshold(d: f32, t: f32) -> f32 {
    let magnitude = d.abs() - t;
    if magnitude <= 0.0 {
        0.0
    } else if d < 0.0 {
        -magnitude
    } else {
        magnitude
    }
}

/// Separable B3-spline convolution with holes: taps land `spacing` pixels
/// apart, which is what makes the transform non-decimated (no resampling, so
/// every level stays at full resolution and nothing has to be interpolated
/// back up). Edges replicate, like every other kernel in the pipeline.
fn convolve_b3(plane: &[f32], width: usize, height: usize, spacing: usize) -> Vec<f32> {
    let tap = |i: usize, k: usize, length: usize| -> usize {
        // k runs 0..5 for taps at -2h, -h, 0, +h, +2h.
        let offset = (k as isize - 2) * spacing as isize;
        (i as isize + offset).clamp(0, length as isize - 1) as usize
    };

    let mut horizontal = vec![0.0f32; plane.len()];
    horizontal
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            let base = y * width;
            for (x, value) in row.iter_mut().enumerate() {
                let mut acc = 0.0;
                for (k, &weight) in B3.iter().enumerate() {
                    acc += weight * plane[base + tap(x, k, width)];
                }
                *value = acc;
            }
        });
    let mut out = vec![0.0f32; plane.len()];
    out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        for (x, value) in row.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (k, &weight) in B3.iter().enumerate() {
                acc += weight * horizontal[tap(y, k, height) * width + x];
            }
            *value = acc;
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The decomposition is a partition of the plane: details plus residual
    /// add back to what came in. Without thresholding, the transform has to
    /// be the identity — otherwise the operator would move pixels even at
    /// strength zero.
    #[test]
    fn a_zero_threshold_reconstructs_the_plane_exactly() {
        let (w, h) = (16, 12);
        let source: Vec<f32> = (0..w * h)
            .map(|i| ((i * 37) % 101) as f32 / 101.0)
            .collect();
        let mut plane = source.clone();
        // base = 0 short-circuits, so go through the levels with a threshold
        // that rounds to nothing instead.
        wavelet_denoise(&mut plane, w, h, f32::MIN_POSITIVE, LEVELS);
        for (got, want) in plane.iter().zip(&source) {
            assert!((got - want).abs() < 1e-5, "{got} != {want}");
        }
    }

    /// A flat plane has no detail at any scale, so nothing to threshold: the
    /// residual is the plane and the operator is a no-op however hard it is
    /// pushed.
    #[test]
    fn a_flat_plane_survives_the_strongest_threshold() {
        let (w, h) = (9, 7);
        let mut plane = vec![0.25f32; w * h];
        wavelet_denoise(&mut plane, w, h, 10.0, LEVELS);
        for value in plane {
            assert!((value - 0.25).abs() < 1e-5, "{value}");
        }
    }

    /// The point of the whole ADR: a step edge keeps its amplitude while
    /// small-amplitude noise around it is removed. A Gaussian blur — what
    /// v1 does — cannot produce both halves of this assertion.
    #[test]
    fn an_edge_survives_while_noise_around_it_does_not() {
        let (w, h) = (32, 16);
        let mut plane = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let step = if x < w / 2 { 0.2 } else { 0.8 };
                // Deterministic ±0.01 checkerboard noise.
                let noise = if (x + y) % 2 == 0 { 0.01 } else { -0.01 };
                plane[y * w + x] = step + noise;
            }
        }
        let noisy = plane.clone();
        wavelet_denoise(&mut plane, w, h, 0.05, LEVELS);

        // Away from the edge the noise is gone.
        let flat = 4 * w + 4;
        assert!(
            (plane[flat] - 0.2).abs() < 0.005,
            "noise survived: {}",
            plane[flat]
        );
        // Across the edge the contrast is still there. A blur strong enough
        // to remove ±0.01 checkerboard noise would have visibly eaten it.
        let (left, right) = (8 * w + w / 2 - 3, 8 * w + w / 2 + 2);
        let kept = plane[right] - plane[left];
        let original = noisy[right] - noisy[left];
        assert!(
            kept > original * 0.9,
            "edge lost: {kept} of {original} kept"
        );
    }

    /// Levels track the proxy factor rather than a radius (ADR 0046 §5), and
    /// never fall to zero — a preview must still denoise.
    #[test]
    fn the_level_count_follows_the_proxy_scale() {
        assert_eq!(levels_at_scale(1.0), 4);
        assert_eq!(levels_at_scale(0.5), 3);
        assert_eq!(levels_at_scale(0.25), 2);
        assert_eq!(levels_at_scale(0.125), 1);
        assert_eq!(levels_at_scale(0.01), 1);
        // Degenerate inputs fall back to the full count rather than panic.
        assert_eq!(levels_at_scale(0.0), 4);
        assert_eq!(levels_at_scale(2.0), 4);
    }

    /// A hole spacing of one is the plain 5-tap kernel; larger spacings must
    /// still preserve a constant (the weights sum to one at every spacing),
    /// which is what keeps the residual unbiased.
    #[test]
    fn the_holed_kernel_preserves_a_constant_at_every_spacing() {
        let (w, h) = (11, 11);
        let plane = vec![0.6f32; w * h];
        for level in 0..LEVELS {
            let out = convolve_b3(&plane, w, h, 1 << level);
            for value in out {
                assert!((value - 0.6).abs() < 1e-6, "spacing {level}: {value}");
            }
        }
    }
}
