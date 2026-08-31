//! Sharpening v2 — rank 190. The unsharp mask of v1, confined to edges
//! (ADR 0096).
//!
//! **At `masking = 0` this renders exactly what v1 renders**: the edge mask
//! is all-ones and the delta passes through untouched (ADR 0096 §1). That
//! is what makes the version bump safe rather than merely legal — a
//! revision that never asked for masking sees the same pixels whichever
//! version it pins, and `masking_zero_matches_v1` holds the two together.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{
    add_luma_delta, gaussian_blur, in_display, luma_plane, smoothstep01,
};

/// Unsharp mask on the luma plane, multiplied by an edge mask.
pub(crate) fn sharpen(px: &mut Pixels, amount: i32, radius: f64, masking: i32) {
    let k = f32::from(amount as i16) / 100.0;
    in_display(px, |px| {
        let (width, height) = (px.width as usize, px.height as usize);
        let plane = luma_plane(px);
        let blurred = gaussian_blur(&plane, width, height, radius as f32);
        let mask = edge_mask(&plane, width, height, masking, radius as f32);
        add_luma_delta(px, |i| mask[i] * k * (plane[i] - blurred[i]));
    });
}

/// Per-pixel weight in [0, 1]: 1 where the luma plane has an edge worth
/// sharpening, 0 in the flat areas an unsharp mask would only make noisy.
///
/// `masking` 0 returns all ones — the v1 behavior exactly, and the reason
/// it short-circuits rather than computing a mask of ones the slow way.
fn edge_mask(plane: &[f32], width: usize, height: usize, masking: i32, radius: f32) -> Vec<f32> {
    if masking <= 0 {
        return vec![1.0; plane.len()];
    }
    // The slider's whole range maps onto gradient magnitudes: the threshold
    // rises with it, so 100 keeps only the strongest edges. The scale is
    // empirical — a luma step of 0.25 across two pixels is a firm edge in
    // an 8-bit-derived plane — and frozen with this version.
    let threshold = f32::from(masking as i16) / 100.0 * 0.25;
    // A band rather than a step: a hard-edged mask is visible as a contour
    // as soon as noise makes a pixel cross the threshold (ADR 0096 §2, the
    // reasoning of ADR 0048 §2 applied to another mask).
    let softness = (threshold * 0.5).max(1e-4);

    let at = |x: usize, y: usize| plane[y * width + x];
    let mut mask = vec![0.0f32; plane.len()];
    for y in 0..height {
        for x in 0..width {
            // Sobel on the clamped neighbourhood: an edge pixel reads its
            // own row rather than wrapping onto the far side of the image.
            let (x0, x1) = (x.saturating_sub(1), (x + 1).min(width - 1));
            let (y0, y1) = (y.saturating_sub(1), (y + 1).min(height - 1));
            let gx = (at(x1, y0) + 2.0 * at(x1, y) + at(x1, y1))
                - (at(x0, y0) + 2.0 * at(x0, y) + at(x0, y1));
            let gy = (at(x0, y1) + 2.0 * at(x, y1) + at(x1, y1))
                - (at(x0, y0) + 2.0 * at(x, y0) + at(x1, y0));
            let magnitude = (gx * gx + gy * gy).sqrt() / 4.0;
            let t = ((magnitude - threshold + softness) / (2.0 * softness)).clamp(0.0, 1.0);
            mask[y * width + x] = smoothstep01(t);
        }
    }
    // Blurred by the sharpening radius before it multiplies, so an edge
    // keeps its halo instead of being cut at the pixel where the gradient
    // falls off (ADR 0096 §2).
    gaussian_blur(&mask, width, height, radius)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stages::sharpen::v1;

    /// A ramp on the left half, flat on the right: an edge and a plateau in
    /// one image, which is what the mask has to tell apart.
    fn image(width: u32, height: u32) -> Pixels {
        let mut data = vec![0.0f32; (width * height * 3) as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let value = if x < width as usize / 2 {
                    0.2 + 0.6 * (x as f32 / (width as f32 / 2.0))
                } else {
                    0.5
                };
                let offset = (y * width as usize + x) * 3;
                data[offset] = value;
                data[offset + 1] = value * 0.9;
                data[offset + 2] = value * 0.8;
            }
        }
        Pixels {
            width,
            height,
            data,
        }
    }

    /// ADR 0096 §1: the promise that makes the version bump safe.
    #[test]
    fn masking_zero_matches_v1() {
        let mut old = image(24, 16);
        let mut new = image(24, 16);
        v1::sharpen(&mut old, 60, 1.2);
        sharpen(&mut new, 60, 1.2, 0);
        assert_eq!(
            old.data, new.data,
            "v2 at masking 0 must render exactly what v1 renders"
        );
    }

    /// The point of the slider: sharpening survives on the edge and stops
    /// in the flat area.
    #[test]
    fn masking_keeps_the_edge_and_spares_the_plateau() {
        let source = image(24, 16);
        let mut unmasked = source.clone();
        let mut masked = source.clone();
        sharpen(&mut unmasked, 100, 1.0, 0);
        sharpen(&mut masked, 100, 1.0, 60);

        let moved = |a: &Pixels, b: &Pixels, x: usize| {
            let offset = (8 * a.width as usize + x) * 3;
            (a.data[offset] - b.data[offset]).abs()
        };
        // In the flat right half, masking must have suppressed nearly all of
        // what the unsharp mask did.
        let flat_unmasked = moved(&source, &unmasked, 20);
        let flat_masked = moved(&source, &masked, 20);
        assert!(
            flat_masked <= flat_unmasked * 0.25 + 1e-6,
            "flat area: masked {flat_masked} vs unmasked {flat_unmasked}"
        );
        // Somewhere on the ramp, sharpening still happens.
        let edge_masked: f32 = (1..11).map(|x| moved(&source, &masked, x)).sum();
        assert!(edge_masked > 1e-4, "the edge must still be sharpened");
    }

    /// The mask is a weight, never a gain: it can only reduce what the
    /// unsharp mask did, so no pixel moves further than it would at 0.
    #[test]
    fn masking_never_amplifies() {
        let source = image(24, 16);
        let mut unmasked = source.clone();
        let mut masked = source.clone();
        sharpen(&mut unmasked, 80, 1.0, 0);
        sharpen(&mut masked, 80, 1.0, 40);
        for ((s, u), m) in source
            .data
            .iter()
            .zip(unmasked.data.iter())
            .zip(masked.data.iter())
        {
            assert!(
                (m - s).abs() <= (u - s).abs() + 1e-5,
                "masked moved {} where unmasked moved {}",
                (m - s).abs(),
                (u - s).abs()
            );
        }
    }
}
