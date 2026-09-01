//! Local adjustments v4 (ADR 0108) — rank 160. `v3` plus the five
//! neighbourhood operators: clarity, texture, sharpness and the two noise
//! reductions, which until now existed globally and nowhere else.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).
//!
//! Everything a `v3` revision expresses renders here **exactly as `v3`
//! renders it**: each of the five is guarded by its own `Option`, and an
//! entry that sets none of them runs the identical operator sequence. The
//! reference renders record that — every `v3` entry has a `v4` twin with the
//! same hash (ADR 0108 §6).
//!
//! Why a neighbourhood operator needs no new architecture: `v3` already
//! develops a **full copy** of the buffer and blends it back by coverage, so
//! an operator that reads its neighbours is simply one more pass over that
//! copy. What it means is stated rather than hidden (ADR 0108 §2) — the
//! operator runs on the whole image, and near a mask edge a pixel's new
//! value was computed from neighbours the mask excludes. The alternative, a
//! bounding box, manufactures a halo where the user happened to draw.
//!
//! The range terms and the operator sequence are copied from `v3` rather
//! than called into it, for the reason ADR 0048 §4 gave: a frozen module
//! never receives a fix, only a successor, so two copies cannot drift.
//!
//! Everything a `v2` revision expresses renders here **exactly as `v2`
//! renders it**: the coverage map is consulted only for `Mask::Coverage`,
//! which `v2` revisions cannot carry (`Settings::validate` refuses it,
//! ADR 0070 §4). The reference renders record that — every `v2` entry has a
//! `v3` twin with the same hash.
//!
//! The range terms and the operator sequence are copied from `v2` rather than
//! called into it. That is what freezing costs, and it costs nothing real: a
//! frozen module never receives a fix, only a successor, so two copies can
//! never drift apart the way shared mutable code would (ADR 0048 §4).

use leyline_core::{ColorRange, LocalAdjustment, LuminanceRange, RangeMask, WhiteBalance};

use crate::mask;
use crate::mask_coverage::MaskCoverages;
use crate::pixels::{Pixels, luma};
use crate::stages::clarity::v1::CLARITY_RADIUS;
use crate::stages::contrast::v1::contrast;
use crate::stages::gains::v1::linear_gains;
use crate::stages::highlights_shadows::v1::highlights_shadows;
use crate::stages::kernel::v1::{display, local_contrast, rgb_to_hsl, saturate, smoothstep01};
use crate::stages::noise_color::v2::color_noise_reduction;
use crate::stages::noise_luminance::v2::luminance_noise_reduction;
use crate::stages::sharpen::v2::sharpen;
use crate::stages::texture::v1::TEXTURE_RADIUS;
use crate::stages::whites_blacks::v1::whites_blacks;

/// Unsharp-mask radius for a local `sharpness`, in pixels at full
/// resolution — `Sharpening::default()`'s radius, frozen here rather than
/// read from the revision (ADR 0108 §3).
const LOCAL_SHARPEN_RADIUS: f64 = 1.0;

/// Applies every local adjustment, in list order: later entries composite on
/// top of what earlier ones wrote.
pub(crate) fn local_adjustments(
    px: &mut Pixels,
    adjustments: &[LocalAdjustment],
    rotation_degrees: f64,
    coverages: &MaskCoverages,
    scale: f32,
) {
    for adjustment in adjustments {
        apply_local_adjustment(px, adjustment, rotation_degrees, coverages, scale);
    }
}

/// Same composition as `v3` — develop a full copy through the re-parameterized
/// global operators, blend it back by coverage × opacity, narrowed by the
/// entry's range mask when it has one — with five more operators available on
/// that copy, the ones that read a neighbourhood.
pub(crate) fn apply_local_adjustment(
    px: &mut Pixels,
    adjustment: &LocalAdjustment,
    rotation_degrees: f64,
    coverages: &MaskCoverages,
    scale: f32,
) {
    let mut coverage = mask::rasterize_coverage(
        &adjustment.mask,
        coverages,
        px.width,
        px.height,
        rotation_degrees,
    );
    if let Some(range) = &adjustment.range {
        narrow_by_range(&mut coverage, px, range);
    }
    let values = &adjustment.adjustments;
    let mut adjusted = px.clone();
    if values.temperature.is_some() || values.tint.is_some() || values.exposure.is_some() {
        let wb = if values.temperature.is_some() || values.tint.is_some() {
            Some(WhiteBalance {
                temperature: values.temperature.unwrap_or(6500),
                tint: values.tint.unwrap_or(0),
            })
        } else {
            None
        };
        linear_gains(&mut adjusted, wb.as_ref(), values.exposure.unwrap_or(0.0));
    }
    if let Some(v) = values.contrast {
        contrast(&mut adjusted, v);
    }
    if values.highlights.is_some() || values.shadows.is_some() {
        highlights_shadows(
            &mut adjusted,
            values.highlights.unwrap_or(0),
            values.shadows.unwrap_or(0),
        );
    }
    if values.whites.is_some() || values.blacks.is_some() {
        whites_blacks(
            &mut adjusted,
            values.whites.unwrap_or(0),
            values.blacks.unwrap_or(0),
        );
    }
    if let Some(v) = values.vibrance {
        saturate(&mut adjusted, v, true);
    }
    if let Some(v) = values.saturation {
        saturate(&mut adjusted, v, false);
    }
    // From here on, operators that read more than one pixel. They run in the
    // order of their own ranks in the pipeline — clarity 90, texture 100,
    // noise 170 and 180, sharpen 190 — which is the rule the ten values above
    // have followed since ADR 0029 without anyone writing it down
    // (ADR 0108 §4).
    //
    // Every radius is a constant bound to *this* stage version and scaled by
    // the render scale, never read from the revision's global settings
    // (ADR 0108 §3): a local slider whose meaning moved when a global one was
    // touched would be unreasonable about.
    if let Some(v) = values.clarity {
        local_contrast(&mut adjusted, v, CLARITY_RADIUS * scale);
    }
    if let Some(v) = values.texture {
        local_contrast(&mut adjusted, v, TEXTURE_RADIUS * scale);
    }
    // The edge-preserving operators of ranks 170 and 180, never the measured
    // ones of ranks 5 and 6: those read sensor counts, and past exposure and
    // the tone curve a measured threshold means nothing (ADR 0072 §5, the
    // reason it moved them to the head of the pipeline).
    if let Some(v) = values.noise_luminance {
        luminance_noise_reduction(&mut adjusted, v, scale);
    }
    if let Some(v) = values.noise_color {
        color_noise_reduction(&mut adjusted, v, scale);
    }
    // One slider, not `Sharpening`'s three: the radius is the default the
    // global stage carries and the edge mask is off, which makes this the
    // unsharp mask of `sharpen::v1` exactly. Below zero it subtracts its own
    // detail, and that softening is the one thing this develop module could
    // not express at any strength before (ADR 0108 §1).
    if let Some(v) = values.sharpness {
        sharpen(&mut adjusted, v, LOCAL_SHARPEN_RADIUS * f64::from(scale), 0);
    }
    mask::blend_by_coverage(px, &adjusted, &coverage, adjustment.opacity);
}

/// Multiplies `coverage` in place by the range terms — so a range can only
/// ever narrow a mask, never widen it (ADR 0048 §1).
///
/// The pixel is read on the display axis (ADR 0048 §3): a luminance band is a
/// statement about the histogram the user is looking at, and the same numbers
/// in linear light would put their lower edge in near-blackness. Values above
/// white are clamped into the band's own space, headroom being outside what a
/// `[0, 1]` band can talk about.
fn narrow_by_range(coverage: &mut [f32], px: &Pixels, range: &RangeMask) {
    for (i, cell) in coverage.iter_mut().enumerate() {
        if *cell <= 0.0 {
            // Outside the geometry already: nothing to narrow, and no reason
            // to pay for the conversion.
            continue;
        }
        let rgb: [f32; 3] = [
            display(px.data[i * 3]),
            display(px.data[i * 3 + 1]),
            display(px.data[i * 3 + 2]),
        ];
        if let Some(band) = &range.luminance {
            *cell *= luminance_term(luma(&rgb), band);
        }
        if let Some(band) = &range.color {
            *cell *= color_term(&rgb, band);
        }
    }
}

/// Coverage of a luminance band: 1 inside `[min, max]`, easing to 0 over
/// `softness` on each side.
fn luminance_term(value: f32, band: &LuminanceRange) -> f32 {
    let value = value.clamp(0.0, 1.0) as f64;
    let softness = band.softness.max(0.0);
    let below = edge_falloff(band.min - value, softness);
    let above = edge_falloff(value - band.max, softness);
    (below * above) as f32
}

/// Coverage of a hue band, weighted by the pixel's saturation.
///
/// The saturation weight is what makes the band usable: hue is undefined on a
/// neutral pixel, and letting greys through would select the clouds along with
/// the blue of the sky (ADR 0048 §2). Full weight is reached at the same
/// saturation the falloff uses for hue, which keeps one knob instead of two.
fn color_term(rgb: &[f32; 3], band: &ColorRange) -> f32 {
    let (hue, saturation, _) = rgb_to_hsl(rgb);
    let distance = hue_distance(f64::from(hue), band.center);
    let softness = band.softness.max(0.0);
    let hue_term = edge_falloff(distance - band.width.max(f64::EPSILON), softness);
    // Below this saturation a pixel has no hue worth speaking of; the ramp
    // avoids a hard boundary in near-neutral areas.
    const SATURATION_FLOOR: f32 = 0.10;
    let saturation_term = smoothstep01((saturation / SATURATION_FLOOR).clamp(0.0, 1.0));
    hue_term as f32 * saturation_term
}

/// Shortest angular distance between two hues, in degrees — hue is circular,
/// so 350° and 10° are 20° apart.
fn hue_distance(a: f64, b: f64) -> f64 {
    let delta = (a - b).rem_euclid(360.0);
    delta.min(360.0 - delta)
}

/// How much survives at `overshoot` past an edge: 1 at or before the edge, 0
/// at `softness` beyond it, smoothly in between. A `softness` of 0 is the hard
/// edge the caller asked for.
fn edge_falloff(overshoot: f64, softness: f64) -> f64 {
    if overshoot <= 0.0 {
        return 1.0;
    }
    if softness <= 0.0 || overshoot >= softness {
        return 0.0;
    }
    f64::from(smoothstep01(1.0 - (overshoot / softness) as f32))
}

#[cfg(test)]
mod tests {
    use leyline_core::{LocalAdjustmentValues, Mask};

    use super::*;

    fn luminance_band() -> LuminanceRange {
        LuminanceRange {
            min: 0.4,
            max: 0.6,
            softness: 0.1,
        }
    }

    #[test]
    fn a_luminance_band_is_full_inside_and_zero_past_the_falloff() {
        let band = luminance_band();
        assert_eq!(luminance_term(0.5, &band), 1.0);
        assert_eq!(luminance_term(0.4, &band), 1.0);
        assert_eq!(luminance_term(0.6, &band), 1.0);
        assert_eq!(luminance_term(0.29, &band), 0.0);
        assert_eq!(luminance_term(0.71, &band), 0.0);
        // Inside the falloff: strictly between, and monotonic.
        let near = luminance_term(0.65, &band);
        let far = luminance_term(0.68, &band);
        assert!(0.0 < far && far < near && near < 1.0, "{near} {far}");
    }

    #[test]
    fn a_zero_softness_luminance_band_is_a_hard_edge() {
        let band = LuminanceRange {
            min: 0.4,
            max: 0.6,
            softness: 0.0,
        };
        // Just inside and just outside, with room for the f32→f64 widening
        // of the sample value: the property is the cliff, not its exact
        // placement to the last bit.
        assert_eq!(luminance_term(0.59, &band), 1.0);
        assert_eq!(luminance_term(0.61, &band), 0.0);
    }

    #[test]
    fn a_hue_band_wraps_around_the_circle() {
        let band = ColorRange {
            center: 0.0,
            width: 30.0,
            softness: 10.0,
        };
        // Saturated red at hue 350 is 10° from center: inside the band.
        let red = color_term(&[1.0, 0.2, 0.35], &band);
        assert!(red > 0.9, "{red}");
        // Saturated blue is far outside it.
        let blue = color_term(&[0.2, 0.3, 1.0], &band);
        assert_eq!(blue, 0.0);
    }

    /// The point of the saturation weight: a grey pixel has no hue, so no hue
    /// band may select it (ADR 0048 §2).
    #[test]
    fn a_neutral_pixel_is_never_inside_a_hue_band() {
        for band_center in [0.0, 60.0, 120.0, 210.0, 300.0] {
            let band = ColorRange {
                center: band_center,
                width: 180.0,
                softness: 0.0,
            };
            for grey in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let term = color_term(&[grey, grey, grey], &band);
                assert_eq!(term, 0.0, "grey {grey} entered the band at {band_center}");
            }
        }
    }

    /// A range narrows and never widens: whatever the terms say, no pixel
    /// comes out with more coverage than the geometry gave it.
    #[test]
    fn a_range_can_only_narrow_the_geometry() {
        let px = Pixels {
            width: 4,
            height: 1,
            data: vec![
                0.0, 0.0, 0.0, // black
                0.2, 0.2, 0.2, // dark grey
                0.5, 0.5, 0.5, // mid grey
                1.0, 0.0, 0.0, // saturated red
            ],
        };
        let range = RangeMask {
            luminance: Some(LuminanceRange {
                min: 0.0,
                max: 1.0,
                softness: 0.0,
            }),
            color: Some(ColorRange {
                center: 0.0,
                width: 30.0,
                softness: 5.0,
            }),
        };
        let mut coverage = vec![0.7f32; 4];
        narrow_by_range(&mut coverage, &px, &range);
        for value in &coverage {
            assert!(*value <= 0.7 + f32::EPSILON, "{value} exceeds the geometry");
        }
        // Only the red pixel has a hue in the band.
        assert_eq!(coverage[0], 0.0);
        assert_eq!(coverage[1], 0.0);
        assert_eq!(coverage[2], 0.0);
        assert!(coverage[3] > 0.6, "{}", coverage[3]);
    }

    /// Zero geometric coverage stays zero, and is not even evaluated: a range
    /// refines a mask, it never introduces coverage of its own.
    #[test]
    fn uncovered_pixels_stay_uncovered() {
        let px = Pixels {
            width: 1,
            height: 1,
            data: vec![1.0, 0.0, 0.0],
        };
        let mut coverage = vec![0.0f32];
        narrow_by_range(
            &mut coverage,
            &px,
            &RangeMask {
                luminance: None,
                color: Some(ColorRange::default()),
            },
        );
        assert_eq!(coverage[0], 0.0);
    }

    /// ADR 0108 §6, checked directly rather than only through the golden
    /// manifest: an entry setting none of the five is `v3`, bit for bit.
    ///
    /// The manifest proves it for the two cases that ship; this proves it for
    /// an arbitrary buffer, which is what makes the version bump safe rather
    /// than merely blessed.
    #[test]
    fn an_entry_without_the_five_renders_exactly_like_v3() {
        let adjustment = LocalAdjustment {
            mask: Mask::Radial {
                cx: 0.5,
                cy: 0.5,
                rx: 0.4,
                ry: 0.3,
                angle: 0.0,
                feather: 0.5,
                inverted: false,
            },
            range: None,
            opacity: 0.8,
            adjustments: LocalAdjustmentValues {
                exposure: Some(0.75),
                contrast: Some(25),
                saturation: Some(-30),
                ..Default::default()
            },
        };
        let coverages = MaskCoverages::default();

        let mut through_v3 = gradient_pixels();
        super::super::v3::local_adjustments(
            &mut through_v3,
            std::slice::from_ref(&adjustment),
            0.0,
            &coverages,
        );

        let mut through_v4 = gradient_pixels();
        local_adjustments(
            &mut through_v4,
            std::slice::from_ref(&adjustment),
            0.0,
            &coverages,
            1.0,
        );

        assert_eq!(through_v3.data, through_v4.data);
    }

    /// The softening ADR 0108 §1 names: a negative texture is the one thing
    /// the develop module could not express at any strength before, and it
    /// has to actually reduce fine detail.
    #[test]
    fn a_negative_texture_reduces_fine_detail() {
        let mut px = checkerboard_pixels();
        let before = fine_detail_energy(&px);
        local_adjustments(
            &mut px,
            &[LocalAdjustment {
                mask: Mask::Everything,
                range: None,
                opacity: 1.0,
                adjustments: LocalAdjustmentValues {
                    texture: Some(-100),
                    ..Default::default()
                },
            }],
            0.0,
            &MaskCoverages::default(),
            1.0,
        );
        let after = fine_detail_energy(&px);
        assert!(after < before, "texture -100 left {after} of {before}");
    }

    /// The safety property of every masked operator, and the one a
    /// neighbourhood operator could plausibly break: whatever it read, it
    /// writes nothing where the coverage is zero.
    ///
    /// ADR 0108 §2 says the operator reads outside the mask — this says it
    /// never *writes* there, which is the half that has to hold.
    #[test]
    fn nothing_is_written_where_the_coverage_is_zero() {
        let reference = checkerboard_pixels();
        let mut px = checkerboard_pixels();
        local_adjustments(
            &mut px,
            &[LocalAdjustment {
                // A radial confined to the left half, with no feather: the
                // right half is strictly outside it.
                mask: Mask::Radial {
                    cx: 0.2,
                    cy: 0.5,
                    rx: 0.15,
                    ry: 0.4,
                    angle: 0.0,
                    feather: 0.0,
                    inverted: false,
                },
                range: None,
                opacity: 1.0,
                adjustments: LocalAdjustmentValues {
                    clarity: Some(100),
                    texture: Some(-100),
                    sharpness: Some(100),
                    noise_luminance: Some(100),
                    noise_color: Some(100),
                    ..Default::default()
                },
            }],
            0.0,
            &MaskCoverages::default(),
            1.0,
        );
        let width = px.width as usize;
        for y in 0..px.height as usize {
            for x in (width * 3 / 4)..width {
                let i = (y * width + x) * 3;
                assert_eq!(
                    px.data[i..i + 3],
                    reference.data[i..i + 3],
                    "pixel ({x}, {y}) moved outside the mask"
                );
            }
        }
    }

    /// Each of the five is independent: absent means neutral, so an entry
    /// asking for one must not drag the other four in.
    #[test]
    fn an_absent_value_runs_no_operator() {
        let reference = checkerboard_pixels();
        let mut px = checkerboard_pixels();
        local_adjustments(
            &mut px,
            &[LocalAdjustment {
                mask: Mask::Everything,
                range: None,
                opacity: 1.0,
                adjustments: LocalAdjustmentValues::default(),
            }],
            0.0,
            &MaskCoverages::default(),
            1.0,
        );
        assert_eq!(px.data, reference.data);
    }

    /// A horizontal ramp: smooth, so the tonal operators have something to
    /// move and the neighbourhood ones have no fine detail to chew on.
    fn gradient_pixels() -> Pixels {
        let (width, height) = (24usize, 16usize);
        let mut data = Vec::with_capacity(width * height * 3);
        for _y in 0..height {
            for x in 0..width {
                let v = (x as f32) / (width as f32 - 1.0);
                data.extend_from_slice(&[v, v * 0.8 + 0.1, 1.0 - v]);
            }
        }
        Pixels {
            width: width as u32,
            height: height as u32,
            data,
        }
    }

    /// A two-pixel checkerboard over a mid grey: the highest fine-detail
    /// frequency there is, which is what a texture or a sharpness slider
    /// acts on.
    fn checkerboard_pixels() -> Pixels {
        let (width, height) = (24usize, 16usize);
        let mut data = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                let v = if (x + y) % 2 == 0 { 0.55 } else { 0.35 };
                data.extend_from_slice(&[v, v, v]);
            }
        }
        Pixels {
            width: width as u32,
            height: height as u32,
            data,
        }
    }

    /// Total absolute difference between horizontally adjacent samples — a
    /// crude but sufficient measure of how much fine detail survives.
    fn fine_detail_energy(px: &Pixels) -> f32 {
        let width = px.width as usize;
        let mut total = 0.0;
        for y in 0..px.height as usize {
            for x in 1..width {
                let i = (y * width + x) * 3;
                total += (px.data[i] - px.data[i - 3]).abs();
            }
        }
        total
    }
}
