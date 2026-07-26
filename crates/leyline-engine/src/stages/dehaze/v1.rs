//! Dehaze v1 (ADR 0033) — rank 110.
//!
//! Introduced by process 10. Dark-channel prior: estimate the atmospheric
//! light, estimate transmission from the dark channel, invert the haze
//! model. A negative amount runs it backwards and adds haze.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use rayon::prelude::*;

use crate::pixels::Pixels;

/// Neighborhood radius (pixels) of the dark channel's local-minimum filter
/// — He et al.'s "patch size", conventionally an odd window around 15px.
pub(crate) const DEHAZE_PATCH_RADIUS: usize = 7;

/// Fraction of the brightest dark-channel pixels considered when picking
/// the atmospheric-light candidate — a closed-form top-percentile select,
/// never an iterative search.
pub(crate) const DEHAZE_TOP_FRACTION: f32 = 0.001;

/// How aggressively the transmission estimate assumes haze is present;
/// `1.0` would fully trust the dark-channel prior, slightly under that
/// (the conventional choice) keeps a touch of natural haze at infinity.
pub(crate) const DEHAZE_OMEGA: f32 = 0.95;

/// Floor on the estimated transmission: guards the recovered-radiance
/// division from blowing up (and amplifying noise) in dense-haze regions.
pub(crate) const DEHAZE_MIN_TRANSMISSION: f32 = 0.1;

/// Dark-channel-prior haze removal (ADR 0033): `amount > 0` removes
/// atmospheric haze, `amount < 0` re-adds it. Every step — the dark
/// channel's local-minimum filter, the atmospheric-light selection, the
/// transmission estimate — is closed-form and deterministic: no iterative
/// solver, nothing whose result depends on an initial guess or a
/// convergence tolerance (`docs/pipeline.md` §5).
pub(crate) fn dehaze(px: &mut Pixels, amount: i32, scale: f32) {
    let width = px.width as usize;
    let height = px.height as usize;
    let k = f32::from(amount as i16) / 100.0;
    // The dark channel's local-minimum window is a pixel neighbourhood:
    // on a proxy it must cover the same share of the subject, but never
    // collapse to zero, which would make the min-filter a no-op and the
    // transmission map degenerate (ADR 0041).
    let patch = ((DEHAZE_PATCH_RADIUS as f32) * scale).round().max(1.0) as usize;

    let per_pixel_min: Vec<f32> = px
        .data
        .chunks_exact(3)
        .map(|rgb| rgb[0].min(rgb[1]).min(rgb[2]))
        .collect();
    let dark = min_filter(&per_pixel_min, width, height, patch);
    let atmosphere = atmospheric_light(px, &dark);

    let normalized_min: Vec<f32> = px
        .data
        .chunks_exact(3)
        .map(|rgb| {
            (rgb[0] / atmosphere[0].max(1e-3))
                .min(rgb[1] / atmosphere[1].max(1e-3))
                .min(rgb[2] / atmosphere[2].max(1e-3))
        })
        .collect();
    let normalized_dark = min_filter(&normalized_min, width, height, patch);

    let row_bytes = width * 3;
    px.data
        .par_chunks_mut(row_bytes)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, rgb) in row.chunks_exact_mut(3).enumerate() {
                let i = y * width + x;
                let transmission =
                    (1.0 - DEHAZE_OMEGA * normalized_dark[i]).max(DEHAZE_MIN_TRANSMISSION);
                if k >= 0.0 {
                    for (sample, &a) in rgb.iter_mut().zip(atmosphere.iter()) {
                        let recovered = ((*sample - a) / transmission + a).clamp(0.0, 1.0);
                        *sample = (*sample + (recovered - *sample) * k).clamp(0.0, 1.0);
                    }
                } else {
                    let haze_amount = (1.0 - transmission) * -k;
                    for (sample, &a) in rgb.iter_mut().zip(atmosphere.iter()) {
                        *sample = (*sample + (a - *sample) * haze_amount).clamp(0.0, 1.0);
                    }
                }
            }
        });
}

/// Separable local-minimum (morphological erosion) filter over a
/// `(2·radius+1)`-wide square window — the same horizontal-then-vertical
/// factoring [`crate::stages::kernel::v1::gaussian_blur`] uses, valid for min/max exactly as it is for
/// weighted sums.
pub(crate) fn min_filter(plane: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let mut horizontal = vec![0.0f32; width * height];
    horizontal
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, out) in row.iter_mut().enumerate() {
                let lo = x.saturating_sub(radius);
                let hi = (x + radius).min(width - 1);
                *out = plane[y * width + lo..=y * width + hi]
                    .iter()
                    .copied()
                    .fold(f32::INFINITY, f32::min);
            }
        });

    let mut out = vec![0.0f32; width * height];
    out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        let lo = y.saturating_sub(radius);
        let hi = (y + radius).min(height - 1);
        for (x, out) in row.iter_mut().enumerate() {
            let mut m = f32::INFINITY;
            for yy in lo..=hi {
                m = m.min(horizontal[yy * width + x]);
            }
            *out = m;
        }
    });
    out
}

/// Picks the atmospheric light color (ADR 0033): among the brightest
/// [`DEHAZE_TOP_FRACTION`] of pixels by dark-channel value — a closed-form
/// top-percentile selection (`select_nth_unstable_by`, deterministic for a
/// given input despite the name), never an iterative search — the one with
/// the highest original-image intensity, He et al.'s selection rule.
pub(crate) fn atmospheric_light(px: &Pixels, dark: &[f32]) -> [f32; 3] {
    let total = dark.len();
    let take = ((total as f32 * DEHAZE_TOP_FRACTION).ceil() as usize).clamp(1, total);
    let mut indices: Vec<usize> = (0..total).collect();
    indices.select_nth_unstable_by(total - take, |&a, &b| {
        dark[a]
            .partial_cmp(&dark[b])
            .expect("luma samples are finite")
    });
    let brightest = indices[total - take..]
        .iter()
        .copied()
        .max_by(|&a, &b| {
            let sum_a: f32 = px.data[a * 3..a * 3 + 3].iter().sum();
            let sum_b: f32 = px.data[b * 3..b * 3 + 3].iter().sum();
            sum_a.partial_cmp(&sum_b).expect("rgb samples are finite")
        })
        .expect("take is always at least 1");
    [
        px.data[brightest * 3],
        px.data[brightest * 3 + 1],
        px.data[brightest * 3 + 2],
    ]
}
