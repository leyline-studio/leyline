//! Defringe v1 — rank 22 (ADR 0113).
//!
//! Axial chromatic aberration and blooming leave a coloured halo *beside* a
//! high-contrast edge — purple in front of the focal plane, green behind it.
//! No resampling can move it, because nothing is displaced: the colour is
//! simply there, and the correction is to take it out.
//!
//! Three gates decide how much comes out of a pixel (ADR 0113 §3–§4): how
//! close its hue is to one of the two fixed bands, how strong the edge next
//! to it is, and the amount the photographer asked for. Only **saturation**
//! moves; hue and luminance are left where they were, which is what makes
//! the operator safe at 100 — the worst it can do to a subject that really
//! is purple is leave its edge grey.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever.

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{
    gaussian_blur, hsl_to_rgb, in_display, luma_plane, rgb_to_hsl, smoothstep01,
};

/// Centre of the purple band, in degrees — magenta-violet, where axial
/// aberration in front of the focal plane lands.
const PURPLE_HUE: f32 = 285.0;
/// Centre of the green band.
const GREEN_HUE: f32 = 120.0;
/// Half-width of a band at full strength, in degrees; beyond it the weight
/// falls off over [`BAND_FALLOFF`] more.
const BAND_HALF_WIDTH: f32 = 25.0;
/// Degrees over which a band's weight goes from one to zero — a soft edge,
/// so a hue crossing the boundary does not step (the reasoning ADR 0048 §2
/// wrote for another mask).
const BAND_FALLOFF: f32 = 35.0;

/// The luma gradient a fringe sits beside, in display units across two
/// pixels. Empirical, and frozen with this version.
const EDGE_FULL: f32 = 0.20;

/// How far the edge mask is spread before it gates anything, in pixels at
/// full resolution: the halo is beside the edge, so a mask that stopped at
/// the gradient would correct everything except the thing it exists for.
const EDGE_SPREAD_PX: f32 = 2.5;

/// Takes the fringe out of `px`, in place.
///
/// `scale` is the render's resolution relative to the full-size image, so a
/// proxy spreads its mask by the same *photographic* distance as an export
/// (the rule `sharpen`'s radius follows).
pub(crate) fn defringe(px: &mut Pixels, purple: i32, green: i32, scale: f32) {
    if purple <= 0 && green <= 0 {
        return;
    }
    let (width, height) = (px.width as usize, px.height as usize);
    if width < 3 || height < 3 {
        return;
    }
    let purple_amount = (purple as f32 / 100.0).clamp(0.0, 1.0);
    let green_amount = (green as f32 / 100.0).clamp(0.0, 1.0);

    // Hue, saturation and the edge mask all read the display axis: "hue" and
    // "saturation" mean here exactly what they mean to the mixer (ADR 0031),
    // and the gradient threshold is a display-space number.
    in_display(px, |px| {
        let mask = edge_mask(
            &luma_plane(px),
            width,
            height,
            (EDGE_SPREAD_PX * scale).max(0.5),
        );
        px.data
            .par_chunks_mut(width * 3)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, rgb) in row.chunks_exact_mut(3).enumerate() {
                    let edge = mask[y * width + x];
                    if edge <= 0.0 {
                        continue;
                    }
                    let (h, s, l) = rgb_to_hsl(rgb);
                    if s <= 0.0 {
                        continue;
                    }
                    let strength = (band(h, PURPLE_HUE) * purple_amount)
                        .max(band(h, GREEN_HUE) * green_amount)
                        * edge;
                    if strength <= 0.0 {
                        continue;
                    }
                    let corrected = hsl_to_rgb(h, s * (1.0 - strength), l);
                    rgb.copy_from_slice(&corrected);
                }
            });
    });
}

/// How much a hue belongs to the band centred on `centre`, in `[0, 1]`.
fn band(hue: f32, centre: f32) -> f32 {
    // Around the circle: 359° and 1° are two degrees apart.
    let mut distance = (hue - centre).abs();
    if distance > 180.0 {
        distance = 360.0 - distance;
    }
    if distance <= BAND_HALF_WIDTH {
        return 1.0;
    }
    let t = (distance - BAND_HALF_WIDTH) / BAND_FALLOFF;
    if t >= 1.0 { 0.0 } else { smoothstep01(1.0 - t) }
}

/// The gradient of the luma plane, normalized, then **dilated** and softened
/// so it covers the halo beside the edge.
///
/// Dilated and not merely blurred, which is the mistake this function was
/// written with first: blurring a narrow ridge spreads it *and lowers its
/// peak*, so the fringe pixels ended up half-corrected at amount 100. A
/// maximum filter carries the edge's own strength outward unchanged, and the
/// blur that follows only takes the staircase off its boundary.
fn edge_mask(plane: &[f32], width: usize, height: usize, spread: f32) -> Vec<f32> {
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
            mask[y * width + x] = smoothstep01((magnitude / EDGE_FULL).clamp(0.0, 1.0));
        }
    }
    let reach = spread.round().max(1.0) as usize;
    gaussian_blur(
        &dilate(&mask, width, height, reach),
        width,
        height,
        spread * 0.5,
    )
}

/// Separable maximum filter: each pixel takes the strongest value within
/// `reach` pixels, horizontally then vertically.
fn dilate(plane: &[f32], width: usize, height: usize, reach: usize) -> Vec<f32> {
    let mut horizontal = vec![0.0f32; plane.len()];
    for y in 0..height {
        for x in 0..width {
            let (from, to) = (x.saturating_sub(reach), (x + reach).min(width - 1));
            horizontal[y * width + x] = plane[y * width + from..=y * width + to]
                .iter()
                .copied()
                .fold(0.0f32, f32::max);
        }
    }
    let mut out = vec![0.0f32; plane.len()];
    for y in 0..height {
        let (from, to) = (y.saturating_sub(reach), (y + reach).min(height - 1));
        for x in 0..width {
            out[y * width + x] = (from..=to)
                .map(|row| horizontal[row * width + x])
                .fold(0.0f32, f32::max);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dark half and a bright half, with a coloured halo painted on the
    /// bright side of the boundary — a fringe, in the place a fringe sits.
    /// The same colour is painted again in the middle of the flat right
    /// half, where no edge is: that patch is the control.
    fn fringed(hue: f32) -> Pixels {
        let (width, height) = (64u32, 32u32);
        let mut data = vec![0.0f32; width as usize * height as usize * 3];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = (y * width as usize + x) * 3;
                let value = if x < 20 { 0.05 } else { 0.85 };
                let rgb = if (20..24).contains(&x) {
                    // The fringe: saturated, at the edge.
                    hsl_to_rgb(hue, 0.8, 0.55)
                } else if (40..60).contains(&x) {
                    // The control: the same colour, wide enough that its
                    // middle is far from its own edges — a subject that
                    // really is purple, not a fringe.
                    hsl_to_rgb(hue, 0.8, 0.55)
                } else {
                    [value, value, value]
                };
                data[offset..offset + 3].copy_from_slice(&rgb);
            }
        }
        Pixels {
            width,
            height,
            data,
        }
    }

    /// Hue, saturation and lightness of one pixel **on the display axis** —
    /// the axis the operator works on, and the only one on which its
    /// promise about lightness is meant (ADR 0113 §3).
    ///
    /// Reading the linear buffer directly instead was the first version of
    /// these helpers, and it reported the lightness moving by 0.08 while the
    /// operator was preserving it exactly: HSL lightness is `(max + min) / 2`
    /// of whatever samples it is handed, and the transfer function between
    /// the two axes is not linear.
    fn hsl_at(px: &Pixels, x: usize) -> (f32, f32, f32) {
        let offset = (16 * px.width as usize + x) * 3;
        let rgb = &px.data[offset..offset + 3];
        let display = [
            crate::stages::kernel::v1::display(rgb[0]),
            crate::stages::kernel::v1::display(rgb[1]),
            crate::stages::kernel::v1::display(rgb[2]),
        ];
        rgb_to_hsl(&display)
    }

    fn saturation_at(px: &Pixels, x: usize) -> f32 {
        hsl_at(px, x).1
    }

    #[test]
    fn a_purple_fringe_at_an_edge_loses_its_saturation() {
        let mut px = fringed(285.0);
        let before = saturation_at(&px, 21);
        defringe(&mut px, 100, 0, 1.0);
        let after = saturation_at(&px, 21);
        assert!(
            after < before * 0.35,
            "the fringe should mostly go: {before} -> {after}"
        );
    }

    /// The gate that protects a photograph that really is purple.
    #[test]
    fn the_same_colour_away_from_an_edge_is_left_alone() {
        let mut px = fringed(285.0);
        let before = saturation_at(&px, 50);
        defringe(&mut px, 100, 0, 1.0);
        assert!(
            (saturation_at(&px, 50) - before).abs() < 0.02,
            "a flat purple patch is not a fringe"
        );
    }

    /// Hue and lightness stay where they were — measured at a *partial*
    /// amount, because at 100 the pixel is grey and a grey pixel has no hue
    /// to compare (which is the next test).
    #[test]
    fn only_saturation_moves_hue_and_lightness_stay() {
        let mut px = fringed(285.0);
        let (hue_before, saturation_before, lightness_before) = hsl_at(&px, 21);
        defringe(&mut px, 50, 0, 1.0);
        let (hue_after, saturation_after, lightness_after) = hsl_at(&px, 21);

        assert!(
            saturation_after < saturation_before * 0.7,
            "the fringe faded"
        );
        // A degree, not zero: `in_display` round-trips the samples through
        // the transfer tables, which is not bit-exact.
        assert!(
            (hue_after - hue_before).abs() < 1.0,
            "hue moved: {hue_before} -> {hue_after}"
        );
        assert!(
            (lightness_after - lightness_before).abs() < 0.01,
            "lightness moved: {lightness_before} -> {lightness_after}"
        );
    }

    /// The worst the operator can do, stated as a test: at 100 an edge goes
    /// grey. It never goes dark, and it never goes another colour
    /// (ADR 0113 §4).
    #[test]
    fn at_full_strength_the_fringe_is_grey_and_nothing_worse() {
        let mut px = fringed(285.0);
        let (_, _, lightness_before) = hsl_at(&px, 21);
        defringe(&mut px, 100, 0, 1.0);
        let (_, saturation, lightness) = hsl_at(&px, 21);
        assert!(saturation < 0.05, "grey, not merely paler: {saturation}");
        assert!(
            (lightness - lightness_before).abs() < 0.01,
            "and as bright as it was: {lightness_before} -> {lightness}"
        );
    }

    /// The two amounts are independent: purple does not touch a green
    /// fringe, and neither of them touches a colour in neither band.
    #[test]
    fn each_band_answers_only_to_its_own_amount() {
        let mut green_fringe = fringed(120.0);
        let before = saturation_at(&green_fringe, 21);
        defringe(&mut green_fringe, 100, 0, 1.0);
        assert!(
            (saturation_at(&green_fringe, 21) - before).abs() < 0.02,
            "the purple amount must not touch a green fringe"
        );
        defringe(&mut green_fringe, 0, 100, 1.0);
        assert!(saturation_at(&green_fringe, 21) < before * 0.35);

        // Orange is in neither band.
        let mut orange = fringed(30.0);
        let before = saturation_at(&orange, 21);
        defringe(&mut orange, 100, 100, 1.0);
        assert!((saturation_at(&orange, 21) - before).abs() < 0.02);
    }

    #[test]
    fn zero_is_exactly_the_photograph_it_was_given() {
        let original = fringed(285.0);
        let mut px = original.clone();
        defringe(&mut px, 0, 0, 1.0);
        assert_eq!(px, original);
    }

    #[test]
    fn the_amount_is_a_dose() {
        let full = {
            let mut px = fringed(285.0);
            defringe(&mut px, 100, 0, 1.0);
            saturation_at(&px, 21)
        };
        let half = {
            let mut px = fringed(285.0);
            defringe(&mut px, 50, 0, 1.0);
            saturation_at(&px, 21)
        };
        let none = saturation_at(&fringed(285.0), 21);
        assert!(full < half && half < none, "{full} < {half} < {none}");
    }
}
