//! Measuring transverse chromatic aberration in the photograph that has it
//! (ADR 0111 §3).
//!
//! **This module renders nothing that is kept**, exactly like
//! [`crate::auto_tone`]: its whole output is two numbers a client writes
//! through an ordinary `EditSession`, so a measured correction lands in the
//! history as one revision like any other and `docs/pipeline.md` §5.1 is
//! untouched by construction. No stage calls this file, and none may: an
//! analysis run inside a render would measure whichever buffer size the
//! render was asked for, and the same revision would then look different in
//! the loupe and in the export (ADR 0111 §2).
//!
//! The model is one number per channel: lateral CA is a radial magnification
//! error, so the displacement of a channel against green grows linearly with
//! the radius, and the slope of that line is the whole measurement.

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::lens_bilinear_channel;

/// What one measurement found (ADR 0111 §1, §6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TcaEstimate {
    /// Red channel magnification error, percent of the radius — the value
    /// `LensCorrection::tca_red` takes as it is.
    pub red: f64,
    /// Blue channel magnification error, percent of the radius.
    pub blue: f64,
    /// How many edge samples survived the fit, the smaller of the two
    /// channels. Below [`MIN_SAMPLES`] the two values above are **zero**
    /// rather than a line fitted to noise (ADR 0111 §6).
    pub samples: usize,
}

/// Radii below this fraction of the corner distance are skipped: lateral CA
/// grows with the radius, so the middle of the frame carries the signal
/// nowhere and the noise everywhere.
const MIN_RADIUS_RATIO: f32 = 0.35;

/// One candidate every `STRIDE` pixels on each axis. Two, not one: the
/// samples that matter are edges, and an edge is several pixels wide.
const STRIDE: usize = 2;

/// The strongest radial gradients kept. Enough that one bad region cannot
/// carry the fit, few enough that the profiles cost nothing.
const MAX_SAMPLES: usize = 20_000;

/// Half-length of the radial profile matched at each sample, in pixels. Four
/// rather than three because two differentiations eat two taps each.
const PROFILE_HALF: i32 = 4;

/// Lucas-Kanade refinement steps. One is the classic linearization; the
/// second buys back the accuracy the first loses when the displacement
/// approaches a pixel, which is exactly where a real aberration sits.
const REFINEMENTS: usize = 4;

/// A displacement further than this is not an aberration, it is a mismatched
/// profile — an occlusion, a specular edge, a saturated highlight.
const MAX_SHIFT: f32 = 2.0;

/// The least gradient energy a profile must carry to be worth aligning — a
/// flat profile is not an edge.
const MIN_CONTRAST: f32 = 1e-6;

/// How much of a profile's gradient energy has to be curvature before its
/// displacement means anything: a straight ramp can be translated without
/// changing shape, so a shift measured on one is a shift measured on
/// nothing.
const MIN_CURVATURE: f32 = 0.02;

/// How well the two channels' profiles must agree, once gain and offset are
/// fitted, for the displacement between them to mean anything. Below this,
/// they are not looking at the same edge: an occlusion, a specular
/// highlight, a channel clipped where the other is not.
const MIN_CORRELATION: f32 = 0.8;

/// Below this many surviving samples the measurement says nothing, and says
/// so (ADR 0111 §6).
pub const MIN_SAMPLES: usize = 200;

/// The widest magnification error the fit will report, percent of the
/// radius — the range `Settings::validate` accepts, and already far beyond
/// any real lens.
const MAX_PERCENT: f64 = 1.0;

/// One edge sample: where it is, and which way "outward" points there.
#[derive(Clone, Copy)]
struct Sample {
    x: f32,
    y: f32,
    /// Unit radial direction, pointing away from the centre.
    ux: f32,
    uy: f32,
    /// Distance from the frame centre, in pixels.
    radius: f32,
}

/// Measures the red and blue magnification error of `px`, in percent of the
/// radius (ADR 0111 §3).
///
/// `px` is the decoded photograph before any stage runs — the frame the
/// aberration lives in, and the frame the correction is applied in.
pub(crate) fn estimate(px: &Pixels) -> TcaEstimate {
    let nothing = TcaEstimate {
        red: 0.0,
        blue: 0.0,
        samples: 0,
    };
    if px.width < 64 || px.height < 64 {
        return nothing;
    }
    let samples = edge_samples(px);
    if samples.len() < MIN_SAMPLES {
        return TcaEstimate {
            samples: samples.len(),
            ..nothing
        };
    }

    let (red, red_used) = fit_channel(px, &samples, 0);
    let (blue, blue_used) = fit_channel(px, &samples, 2);
    let used = red_used.min(blue_used);
    if used < MIN_SAMPLES {
        return TcaEstimate {
            samples: used,
            ..nothing
        };
    }
    TcaEstimate {
        red: red.clamp(-MAX_PERCENT, MAX_PERCENT),
        blue: blue.clamp(-MAX_PERCENT, MAX_PERCENT),
        samples: used,
    }
}

/// The strongest radial green gradients outside the inner disc, in a fixed
/// order: scanned by row, sorted by magnitude with a **stable** sort, so the
/// same image always yields the same set and the same measurement.
fn edge_samples(px: &Pixels) -> Vec<Sample> {
    let cx = (px.width as f32 - 1.0) / 2.0;
    let cy = (px.height as f32 - 1.0) / 2.0;
    let min_radius = cx.hypot(cy) * MIN_RADIUS_RATIO;
    let margin = PROFILE_HALF + 1;

    let mut candidates: Vec<(f32, Sample)> = Vec::new();
    let mut y = margin;
    while y < px.height as i32 - margin {
        let mut x = margin;
        while x < px.width as i32 - margin {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            let radius = dx.hypot(dy);
            if radius >= min_radius {
                let (ux, uy) = (dx / radius, dy / radius);
                // The gradient along the radial direction, which is the only
                // direction a radial magnification moves anything.
                let ahead = lens_bilinear_channel(px, x as f32 + ux, y as f32 + uy, 1);
                let behind = lens_bilinear_channel(px, x as f32 - ux, y as f32 - uy, 1);
                if let (Some(ahead), Some(behind)) = (ahead, behind) {
                    candidates.push((
                        (ahead - behind).abs(),
                        Sample {
                            x: x as f32,
                            y: y as f32,
                            ux,
                            uy,
                            radius,
                        },
                    ));
                }
            }
            x += STRIDE as i32;
        }
        y += STRIDE as i32;
    }

    candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    candidates.truncate(MAX_SAMPLES);
    candidates.into_iter().map(|(_, sample)| sample).collect()
}

/// Measures the displacement of `channel` against green at every sample, then
/// fits `d = k·r` through the origin — twice, dropping the samples beyond two
/// sigma the second time (ADR 0111 §3).
///
/// Returns `(percent, samples that survived)`.
fn fit_channel(px: &Pixels, samples: &[Sample], channel: usize) -> (f64, usize) {
    let observations: Vec<(f64, f64)> = samples
        .par_iter()
        .filter_map(|sample| {
            displacement(px, sample, channel).map(|d| (sample.radius as f64, d as f64))
        })
        .collect();
    if observations.is_empty() {
        return (0.0, 0);
    }

    let slope = |rows: &[(f64, f64)]| -> f64 {
        let (num, den) = rows
            .iter()
            .fold((0.0, 0.0), |(num, den), (r, d)| (num + r * d, den + r * r));
        if den > 0.0 { num / den } else { 0.0 }
    };

    let first = slope(&observations);
    let variance = observations
        .iter()
        .map(|(r, d)| (d - first * r).powi(2))
        .sum::<f64>()
        / observations.len() as f64;
    let limit = 2.0 * variance.sqrt();
    let kept: Vec<(f64, f64)> = observations
        .into_iter()
        .filter(|(r, d)| (d - first * r).abs() <= limit)
        .collect();
    if kept.is_empty() {
        return (0.0, 0);
    }
    (slope(&kept) * 100.0, kept.len())
}

/// The sub-pixel displacement of `channel` against green along the sample's
/// radial direction, positive outward — Lucas-Kanade on the **derivative**
/// profiles, refined.
///
/// Working on the derivatives is the load-bearing choice, and it took two
/// wrong turns to find (ADR 0111 §3). A channel differs from green by an
/// exposure gain *and* a level offset, so both have to be neutralized; but
/// any method that fits a free offset — subtracting the mean, fitting
/// `a·green + b` — also eats the displacement, because over a short window a
/// translation of a locally straight profile *is* an offset. Differentiating
/// kills the offset outright, and the remaining gain is fitted through the
/// origin, which a translation cannot hide in.
///
/// What is left carrying the signal is the profile's curvature — which is
/// exactly right: a perfectly straight profile holds no displacement
/// information at all, and [`MIN_CURVATURE`] rejects it rather than
/// returning the noise.
fn displacement(px: &Pixels, sample: &Sample, channel: usize) -> Option<f32> {
    let green = derivative(&profile(px, sample, 1, 0.0)?);
    let green_energy: f32 = green.iter().map(|g| g * g).sum();
    if green_energy < MIN_CONTRAST {
        return None; // a flat profile: no edge here
    }
    // The second derivative of the profile, i.e. the curvature the fit
    // actually reads.
    let curvature = derivative(&green);
    let curvature_energy: f32 = curvature.iter().map(|c| c * c).sum();
    if curvature_energy < MIN_CURVATURE * green_energy {
        return None; // a straight ramp: a shift on it is indistinguishable
    }

    let mut shift = 0.0f32;
    for _ in 0..REFINEMENTS {
        let other = derivative(&profile(px, sample, channel, shift)?);
        let other_energy: f32 = other.iter().map(|o| o * o).sum();
        if other_energy < MIN_CONTRAST {
            return None;
        }
        // `other ≈ gain · green(t − d)`: the gain is fitted through the
        // origin — no offset, that is the whole point — and the correlation
        // says whether the two are looking at the same edge at all.
        let cross: f32 = green.iter().zip(other.iter()).map(|(g, o)| g * o).sum();
        let gain = cross / green_energy;
        let correlation = cross / (green_energy * other_energy).sqrt();
        if gain <= 0.0 || correlation < MIN_CORRELATION {
            return None;
        }

        // `other − gain·green ≈ −d · gain · green'`.
        let (mut num, mut den) = (0.0f32, 0.0f32);
        for (i, slope) in curvature.iter().enumerate() {
            num += slope * (other[i + 1] - gain * green[i + 1]);
            den += slope * slope;
        }
        let den = den * gain;
        if den <= f32::MIN_POSITIVE {
            return None;
        }
        shift += -num / den;
        if !shift.is_finite() || shift.abs() > MAX_SHIFT {
            return None;
        }
    }
    Some(shift)
}

/// One radial profile: `2·PROFILE_HALF + 1` taps one pixel apart, centred on
/// the sample and slid along the radius by `shift`.
fn profile(px: &Pixels, sample: &Sample, channel: usize, shift: f32) -> Option<Vec<f32>> {
    (-PROFILE_HALF..=PROFILE_HALF)
        .map(|t| {
            let t = t as f32 + shift;
            lens_bilinear_channel(
                px,
                sample.x + sample.ux * t,
                sample.y + sample.uy * t,
                channel,
            )
        })
        .collect()
}

/// Central differences of a profile, two taps shorter than what it is given.
fn derivative(profile: &[f32]) -> Vec<f32> {
    profile
        .windows(3)
        .map(|window| (window[2] - window[0]) / 2.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A smooth pattern with gradient in every direction — a sinusoid rather
    /// than a checkerboard, so sampling it at sub-pixel positions is exact
    /// enough to measure a fifth of a pixel.
    fn pattern(x: f32, y: f32) -> f32 {
        0.5 + 0.25 * (x * 0.62).sin() + 0.25 * (y * 0.54).sin()
    }

    /// An image whose red and blue channels are magnified against green by
    /// the given fractions — a synthetic lateral chromatic aberration.
    ///
    /// A channel magnified by `e` shows at radius `r·(1 + e)` what green
    /// shows at `r`, which is what this samples.
    fn aberrated(width: u32, height: u32, red: f64, blue: f64) -> Pixels {
        let cx = (width as f32 - 1.0) / 2.0;
        let cy = (height as f32 - 1.0) / 2.0;
        let scales = [1.0 / (1.0 + red as f32), 1.0, 1.0 / (1.0 + blue as f32)];
        let mut data = vec![0.0f32; width as usize * height as usize * 3];
        for y in 0..height {
            for x in 0..width {
                for (c, scale) in scales.iter().enumerate() {
                    let sx = cx + (x as f32 - cx) * scale;
                    let sy = cy + (y as f32 - cy) * scale;
                    data[(y as usize * width as usize + x as usize) * 3 + c] = pattern(sx, sy);
                }
            }
        }
        Pixels {
            width,
            height,
            data,
        }
    }

    #[test]
    fn a_magnified_channel_is_measured_with_the_sign_the_correction_expects() {
        let estimate = estimate(&aberrated(800, 600, 0.002, -0.001));
        assert!(estimate.samples >= MIN_SAMPLES, "{estimate:?}");
        assert!(
            (estimate.red - 0.2).abs() < 0.03,
            "red should measure ≈ +0.2 %, got {estimate:?}"
        );
        assert!(
            (estimate.blue + 0.1).abs() < 0.03,
            "blue should measure ≈ -0.1 %, got {estimate:?}"
        );
    }

    /// The whole reason the unit is a percentage (ADR 0111 §1): one number,
    /// every resolution.
    #[test]
    fn the_measurement_does_not_depend_on_the_resolution() {
        let big = estimate(&aberrated(800, 600, 0.002, 0.0));
        let small = estimate(&aberrated(400, 300, 0.002, 0.0));
        assert!(
            (big.red - small.red).abs() < 0.04,
            "half the pixels, the same aberration: {big:?} vs {small:?}"
        );
    }

    #[test]
    fn an_aligned_image_measures_zero() {
        let estimate = estimate(&aberrated(600, 400, 0.0, 0.0));
        assert!(estimate.samples >= MIN_SAMPLES);
        assert!(estimate.red.abs() < 0.02, "{estimate:?}");
        assert!(estimate.blue.abs() < 0.02, "{estimate:?}");
    }

    #[test]
    fn a_photograph_with_no_edges_says_so_instead_of_guessing() {
        let flat = Pixels {
            width: 400,
            height: 300,
            data: vec![0.4; 400 * 300 * 3],
        };
        let estimate = estimate(&flat);
        assert_eq!(estimate.red, 0.0);
        assert_eq!(estimate.blue, 0.0);
        assert!(estimate.samples < MIN_SAMPLES, "{estimate:?}");
    }

    #[test]
    fn an_image_too_small_to_measure_is_not_measured() {
        let tiny = Pixels {
            width: 32,
            height: 32,
            data: vec![0.5; 32 * 32 * 3],
        };
        assert_eq!(estimate(&tiny).samples, 0);
    }

    /// The measurement and the correction have to agree about the sign and
    /// the unit, and nothing but running both proves it: measure a synthetic
    /// aberration, apply what came back through `lens::v2`, and the channels
    /// line up again.
    #[test]
    fn measuring_then_correcting_puts_the_channels_back_together() {
        let aberration = aberrated(800, 600, 0.002, -0.0015);
        let estimate = estimate(&aberration);
        let corrected =
            crate::stages::lens::v2::correct_tca(&aberration, None, estimate.red, estimate.blue);

        // The residual is measured the same way, on the corrected buffer.
        let residual = self::estimate(&corrected);
        assert!(
            residual.red.abs() < 0.03 && residual.blue.abs() < 0.03,
            "correction left {residual:?} of {estimate:?}"
        );

        // And the channels agree pixel by pixel where they did not before,
        // away from the borders the resampling cannot fill.
        let misalignment = |px: &Pixels| -> f32 {
            let mut worst = 0.0f32;
            for y in (60..540).step_by(7) {
                for x in (60..740).step_by(7) {
                    let at = |c: usize| px.data[(y * 800 + x) * 3 + c];
                    worst = worst.max((at(0) - at(1)).abs()).max((at(2) - at(1)).abs());
                }
            }
            worst
        };
        assert!(
            misalignment(&corrected) < misalignment(&aberration) / 2.0,
            "corrected {} vs original {}",
            misalignment(&corrected),
            misalignment(&aberration)
        );
    }
}
