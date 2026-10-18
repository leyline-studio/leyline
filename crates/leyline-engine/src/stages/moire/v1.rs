//! Colour moiré reduction v1 (ADR 0165) — rank 7, after the measured noise
//! reduction and before anything that resamples the image.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).
//!
//! Each channel's deviation from luma goes through a guided filter whose
//! guide is the luma plane (He, Sun & Tang, 2010): chroma the luma explains —
//! a colour edge that is also a brightness edge — is kept, and chroma that
//! swings over a flat luma, which is what colour moiré is, is replaced by
//! its local mean. The filtered chroma is then believed in proportion to how
//! much of the window's detail is chroma rather than luma, which is what
//! keeps a thin coloured branch from turning grey (ADR 0165 §2).

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{in_display, luma_plane};

/// Window radius at full strength on a full-resolution render, in pixels
/// (ADR 0165 §2). Frozen with the version.
const MAX_RADIUS: f32 = 12.0;

/// The guided filter's regulariser on the display axis (ADR 0165 §2): the
/// luma variance below which a window is treated as flat. Frozen.
const EPSILON: f64 = 1e-3;

/// How much more a window's luma detail must weigh than its chroma detail
/// for the window to be read as scene rather than moiré (ADR 0165 §2).
/// Frozen.
const LUMA_WEIGHT: f64 = 4.0;

/// Reduces colour moiré at `strength` in `[1, 100]`.
pub(crate) fn moire(px: &mut Pixels, strength: i32, scale: f32) {
    let k = strength.clamp(0, 100) as f32 / 100.0;
    let radius = (MAX_RADIUS * k * scale).round().max(1.0) as usize;
    in_display(px, |px| reduce(px, radius));
}

/// The filter itself, on a display-axis buffer.
fn reduce(px: &mut Pixels, radius: usize) {
    let (w, h) = (px.width as usize, px.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    let guide: Vec<f64> = luma_plane(px).into_iter().map(f64::from).collect();
    let guide_mean = box_mean(&guide, w, h, radius);
    let guide_squared: Vec<f64> = guide.iter().map(|y| y * y).collect();
    let guide_variance: Vec<f64> = box_mean(&guide_squared, w, h, radius)
        .iter()
        .zip(&guide_mean)
        .map(|(mean_square, mean)| mean_square - mean * mean)
        .collect();

    for channel in 0..3 {
        let chroma: Vec<f64> = (0..w * h)
            .map(|i| f64::from(px.data[i * 3 + channel]) - guide[i])
            .collect();
        let chroma_mean = box_mean(&chroma, w, h, radius);
        let product: Vec<f64> = chroma.iter().zip(&guide).map(|(c, y)| c * y).collect();
        let product_mean = box_mean(&product, w, h, radius);

        // The per-window least-squares fit `chroma ≈ a·luma + b`.
        let a: Vec<f64> = (0..w * h)
            .map(|i| {
                let covariance = product_mean[i] - guide_mean[i] * chroma_mean[i];
                covariance / (guide_variance[i] + EPSILON)
            })
            .collect();
        let b: Vec<f64> = (0..w * h)
            .map(|i| chroma_mean[i] - a[i] * guide_mean[i])
            .collect();
        let a_mean = box_mean(&a, w, h, radius);
        let b_mean = box_mean(&b, w, h, radius);
        // Where to believe the filter: moiré is chroma detail *without*
        // luma detail. A thin coloured stem has edges in both and keeps its
        // colour; bands over a flat grey have chroma detail only.
        let chroma_squared: Vec<f64> = chroma.iter().map(|c| c * c).collect();
        let chroma_variance: Vec<f64> = box_mean(&chroma_squared, w, h, radius)
            .iter()
            .zip(&chroma_mean)
            .map(|(mean_square, mean)| (mean_square - mean * mean).max(0.0))
            .collect();

        px.data
            .par_chunks_mut(w * 3)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..w {
                    let i = y * w + x;
                    let filtered = a_mean[i] * guide[i] + b_mean[i];
                    let luma_detail = LUMA_WEIGHT * guide_variance[i].max(0.0);
                    let belief =
                        chroma_variance[i] / (chroma_variance[i] + luma_detail + f64::MIN_POSITIVE);
                    let corrected = chroma[i] + belief * (filtered - chroma[i]);
                    row[x * 3 + channel] = (guide[i] + corrected).max(0.0) as f32;
                }
            });
    }
}

/// The mean of `plane` over a `(2r + 1)²` window clipped at the border,
/// as exact sums in `f64` — separable, rows in parallel, then columns.
fn box_mean(plane: &[f64], w: usize, h: usize, r: usize) -> Vec<f64> {
    // Horizontal sums, each row on its own: independent of thread count.
    let mut rows = vec![0.0f64; w * h];
    rows.par_chunks_mut(w)
        .zip(plane.par_chunks(w))
        .for_each(|(out, src)| {
            let mut prefix = vec![0.0f64; w + 1];
            for x in 0..w {
                prefix[x + 1] = prefix[x] + src[x];
            }
            for (x, value) in out.iter_mut().enumerate() {
                let lo = x.saturating_sub(r);
                let hi = (x + r).min(w - 1);
                *value = prefix[hi + 1] - prefix[lo];
            }
        });
    // Vertical sums and the division, each column on its own.
    let mut out = vec![0.0f64; w * h];
    let columns: Vec<Vec<f64>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let mut prefix = vec![0.0f64; h + 1];
            for y in 0..h {
                prefix[y + 1] = prefix[y] + rows[y * w + x];
            }
            let width = ((x + r).min(w - 1) - x.saturating_sub(r) + 1) as f64;
            (0..h)
                .map(|y| {
                    let lo = y.saturating_sub(r);
                    let hi = (y + r).min(h - 1);
                    (prefix[hi + 1] - prefix[lo]) / (width * (hi - lo + 1) as f64)
                })
                .collect()
        })
        .collect();
    for (x, column) in columns.iter().enumerate() {
        for (y, value) in column.iter().enumerate() {
            out[y * w + x] = *value;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 3]) -> Pixels {
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                data.extend_from_slice(&f(x, y));
            }
        }
        Pixels {
            width: w,
            height: h,
            data,
        }
    }

    /// The spread of `channel − luma` across the image.
    fn chroma_spread(px: &Pixels, channel: usize) -> f32 {
        let luma = luma_plane(px);
        let values: Vec<f32> = (0..luma.len())
            .map(|i| px.data[i * 3 + channel] - luma[i])
            .collect();
        let max = values.iter().copied().fold(f32::MIN, f32::max);
        let min = values.iter().copied().fold(f32::MAX, f32::min);
        max - min
    }

    #[test]
    fn the_box_mean_is_exact_and_clipped_at_the_border() {
        let plane: Vec<f64> = (0..12).map(f64::from).collect(); // 4 x 3
        let mean = box_mean(&plane, 4, 3, 1);
        // Corner (0, 0): the window holds 0, 1, 4, 5.
        assert!((mean[0] - 2.5).abs() < 1e-12);
        // Centre (1, 1): the window holds 0..=2, 4..=6, 8..=10.
        assert!((mean[5] - 5.0).abs() < 1e-12);
    }

    #[test]
    fn colour_bands_over_a_flat_grey_are_flattened() {
        // A mid grey with a red/cyan alternation every two columns: the
        // luma is (almost) flat, the chroma swings — colour moiré.
        let mut px = pixels(64, 32, |x, _| {
            if (x / 2) % 2 == 0 {
                [0.24, 0.16, 0.16]
            } else {
                [0.16, 0.20, 0.20]
            }
        });
        let before = chroma_spread(&px, 0);
        moire(&mut px, 100, 1.0);
        let after = chroma_spread(&px, 0);
        assert!(after < before * 0.25, "{before} -> {after}");
    }

    #[test]
    fn a_thin_coloured_branch_keeps_its_colour() {
        // The case a real photograph exposed (ADR 0165 §2): a two-pixel
        // brown branch across a dark background, beside bright green leaves.
        let make = || {
            pixels(64, 32, |x, _| match x {
                30 | 31 => [0.30, 0.15, 0.06],
                0..=15 => [0.25, 0.60, 0.10],
                _ => [0.02, 0.03, 0.02],
            })
        };
        let mut px = make();
        moire(&mut px, 100, 1.0);
        let original = make();
        let i = (16 * 64 + 30) * 3;
        let red_excess = |data: &[f32]| data[i] - data[i + 1];
        assert!(
            red_excess(&px.data) > 0.8 * red_excess(&original.data),
            "{:?} -> {:?}",
            &original.data[i..i + 3],
            &px.data[i..i + 3]
        );
    }

    #[test]
    fn a_colour_edge_that_is_also_a_brightness_edge_survives() {
        // Dark blue on the left, bright yellow on the right.
        let make = || {
            pixels(64, 16, |x, _| {
                if x < 32 {
                    [0.02, 0.03, 0.20]
                } else {
                    [0.80, 0.75, 0.10]
                }
            })
        };
        let mut px = make();
        moire(&mut px, 100, 1.0);
        let original = make();
        // Far from the edge nothing moves; at the edge the colours stay
        // where they were to within a few percent.
        for x in [4usize, 28, 35, 60] {
            for channel in 0..3 {
                let i = (8 * 64 + x) * 3 + channel;
                assert!(
                    (px.data[i] - original.data[i]).abs() < 0.03,
                    "x {x} channel {channel}: {} -> {}",
                    original.data[i],
                    px.data[i]
                );
            }
        }
    }
}
