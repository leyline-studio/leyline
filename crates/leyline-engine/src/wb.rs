//! White balance from a sample (ADR 0091): the picker and Auto.
//!
//! **This module renders nothing that is kept.** Like `auto_tone` (ADR 0088
//! §1), its whole output is a [`WhiteBalance`] a client then writes through
//! an ordinary `EditSession`: no stage, no stage version, nothing new in
//! `settings_json` — `docs/pipeline.md` §5.1 is untouched by construction.
//!
//! The solver inverts the gains stage's model: gains are the ratio of two
//! blackbody colors (6500 K reference over the slider's temperature),
//! normalized on green, tint a power of two on the green channel. The model
//! is restated here rather than exported from the frozen stage file
//! (ADR 0042 §1 keeps that file untouched); the
//! [`tests::gains_agree_with_the_frozen_stage`] test holds the two in
//! agreement.

use leyline_core::WhiteBalance;
use leyline_preview::Rgb8;

use crate::stages::kernel::v1::blackbody_rgb;

/// The slider's own range, which is also the search interval.
const TEMP_MIN: f64 = 2000.0;
const TEMP_MAX: f64 = 12000.0;

/// How many render/sample/solve rounds the picker runs at most. The later
/// stages preserve neutrality but distort ratios, so each round lands
/// closer; in practice two or three suffice.
pub(crate) const SEARCH_STEPS: usize = 6;

/// A sample counts as neutral when its channel spread is within this
/// fraction of its mean — well under a visible cast.
const NEUTRAL_TOLERANCE: f64 = 0.01;

/// Below this linear luminance a sample is noise, above the clip guard it
/// is a wall: both are refused rather than guessed (ADR 0091 §2).
const DARK_FLOOR: f64 = 0.005;
const CLIP_CEILING: f64 = 0.98;

/// The per-channel gains the gains stage derives from a white balance —
/// the forward model this module inverts.
pub(crate) fn wb_gains(wb: &WhiteBalance) -> [f64; 3] {
    let reference = blackbody_rgb(6500.0);
    let target = blackbody_rgb(f64::from(wb.temperature));
    let mut gains = [1.0f64; 3];
    for c in 0..3 {
        gains[c] = (reference[c] / target[c]).clamp(0.1, 10.0);
    }
    let green = gains[1];
    for gain in &mut gains {
        *gain /= green;
    }
    gains[1] *= 2.0f64.powf(-f64::from(wb.tint) / 200.0);
    gains
}

/// The white balance whose gains make `rgb` (linear, pre-gains) neutral.
///
/// Temperature by binary search: raising it lowers the blackbody's red and
/// raises its blue, so the red/blue balance of the corrected sample is
/// monotone in temperature. Tint then falls out in closed form on green.
/// Both land clamped to the slider's own range — the picker can never
/// produce a value the sliders cannot show.
pub(crate) fn solve(rgb: [f64; 3]) -> WhiteBalance {
    let [r, g, b] = rgb;
    let balance = |kelvin: f64| -> f64 {
        let reference = blackbody_rgb(6500.0);
        let target = blackbody_rgb(kelvin);
        let gain_r = (reference[0] / target[0]).clamp(0.1, 10.0);
        let gain_b = (reference[2] / target[2]).clamp(0.1, 10.0);
        gain_r * r - gain_b * b
    };
    let (mut lo, mut hi) = (TEMP_MIN, TEMP_MAX);
    if balance(lo) >= 0.0 {
        hi = lo;
    } else if balance(hi) <= 0.0 {
        lo = hi;
    } else {
        while hi - lo > 1.0 {
            let mid = (lo + hi) / 2.0;
            if balance(mid) < 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let temperature = ((lo + hi) / 2.0).round() as u32;

    // With red and blue balanced, the green gain that meets them is a pure
    // tint: g · 2^(−tint/200) = r · gain_r.
    let reference = blackbody_rgb(6500.0);
    let target = blackbody_rgb(f64::from(temperature));
    let gain_r = {
        let mut gains = [0.0f64; 3];
        for c in 0..3 {
            gains[c] = (reference[c] / target[c]).clamp(0.1, 10.0);
        }
        gains[0] / gains[1]
    };
    let tint = if g > 0.0 && r > 0.0 {
        (-200.0 * (r * gain_r / g).log2()).clamp(-100.0, 100.0)
    } else {
        0.0
    };
    #[allow(clippy::cast_possible_truncation)]
    WhiteBalance {
        temperature,
        tint: tint.round() as i32,
    }
}

/// sRGB electro-optical transfer, one channel, `[0, 255]` → linear `[0, 1]`.
fn srgb_to_linear(v: u8) -> f64 {
    let v = f64::from(v) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Mean linear color of the 5×5 neighbourhood around `(x, y)` (unit
/// coordinates of the image), clamped at the borders. Averaged in linear
/// light, where a mean means something.
pub(crate) fn sample_mean(image: &Rgb8, x: f64, y: f64) -> [f64; 3] {
    let (width, height) = (image.width() as i64, image.height() as i64);
    #[allow(clippy::cast_possible_truncation)]
    let cx = ((x.clamp(0.0, 1.0) * (width - 1) as f64).round()) as i64;
    #[allow(clippy::cast_possible_truncation)]
    let cy = ((y.clamp(0.0, 1.0) * (height - 1) as f64).round()) as i64;
    let mut sum = [0.0f64; 3];
    let mut count = 0.0f64;
    for dy in -2..=2 {
        for dx in -2..=2 {
            let (px, py) = (
                (cx + dx).clamp(0, width - 1),
                (cy + dy).clamp(0, height - 1),
            );
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let offset = ((py * width + px) * 3) as usize;
            let rgb = &image.data()[offset..offset + 3];
            for c in 0..3 {
                sum[c] += srgb_to_linear(rgb[c]);
            }
            count += 1.0;
        }
    }
    sum.map(|s| s / count)
}

/// Mean linear color of the whole frame, clipped pixels excluded so a
/// blown sky does not vote (ADR 0091 §3). Falls back to the unfiltered
/// mean when everything is clipped.
pub(crate) fn frame_mean(image: &Rgb8) -> [f64; 3] {
    let mut sum = [0.0f64; 3];
    let mut count = 0.0f64;
    let mut all_sum = [0.0f64; 3];
    let mut all_count = 0.0f64;
    for rgb in image.data().chunks_exact(3) {
        let linear = [
            srgb_to_linear(rgb[0]),
            srgb_to_linear(rgb[1]),
            srgb_to_linear(rgb[2]),
        ];
        for c in 0..3 {
            all_sum[c] += linear[c];
        }
        all_count += 1.0;
        if rgb.iter().all(|&v| v < 250) {
            for c in 0..3 {
                sum[c] += linear[c];
            }
            count += 1.0;
        }
    }
    if count > 0.0 {
        sum.map(|s| s / count)
    } else if all_count > 0.0 {
        all_sum.map(|s| s / all_count)
    } else {
        [0.0; 3]
    }
}

/// Whether a linear sample is already neutral within tolerance.
pub(crate) fn neutral_enough(rgb: [f64; 3]) -> bool {
    let mean = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    if mean <= 0.0 {
        return true;
    }
    let spread = rgb.iter().fold(0.0f64, |m, &v| m.max((v - mean).abs()));
    spread / mean < NEUTRAL_TOLERANCE
}

/// Why a sample cannot answer for a white balance, if it cannot.
pub(crate) fn refuse(rgb: [f64; 3]) -> Option<&'static str> {
    let mean = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    if mean < DARK_FLOOR {
        return Some("the sample is too dark to read a white balance from");
    }
    if rgb.iter().any(|&v| v > CLIP_CEILING) {
        return Some("the sample is clipped; a blown channel has no color left");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixels::Pixels;
    use crate::stages::gains::v1::linear_gains;

    /// The restated model matches the frozen stage: applying
    /// [`wb_gains`] by hand equals running the stage on one pixel.
    #[test]
    fn gains_agree_with_the_frozen_stage() {
        for wb in [
            WhiteBalance::default(),
            WhiteBalance {
                temperature: 3200,
                tint: -40,
            },
            WhiteBalance {
                temperature: 9000,
                tint: 25,
            },
        ] {
            let mut px = Pixels {
                width: 1,
                height: 1,
                data: vec![0.5, 0.5, 0.5],
            };
            linear_gains(&mut px, Some(&wb), 0.0);
            let gains = wb_gains(&wb);
            for (c, gain) in gains.iter().enumerate() {
                #[allow(clippy::cast_possible_truncation)]
                let expected = (0.5 * gain) as f32;
                assert!(
                    (px.data[c] - expected).abs() < 1e-6,
                    "channel {c} of {wb:?}: stage {} vs model {expected}",
                    px.data[c]
                );
            }
        }
    }

    /// Round trip: the gains of the solved balance make the sample neutral.
    #[test]
    fn solve_neutralizes_its_sample() {
        for rgb in [
            [0.30, 0.25, 0.20], // warm cast
            [0.20, 0.20, 0.23], // cool cast
            [0.20, 0.24, 0.20], // green cast
            [0.25, 0.25, 0.25], // already neutral
        ] {
            let wb = solve(rgb);
            let gains = wb_gains(&wb);
            let out = [rgb[0] * gains[0], rgb[1] * gains[1], rgb[2] * gains[2]];
            assert!(
                neutral_enough(out),
                "{rgb:?} solved to {wb:?} but renders {out:?}"
            );
        }
    }

    /// An already-neutral sample answers with the neutral slider values.
    #[test]
    fn neutral_sample_solves_to_the_default() {
        let wb = solve([0.4, 0.4, 0.4]);
        assert_eq!(wb, WhiteBalance::default());
    }

    /// A cast stronger than the slider range lands clamped, never outside.
    #[test]
    fn extreme_casts_clamp_to_the_slider_range() {
        let hot = solve([1.0, 0.3, 0.05]);
        assert!((2000..=12000).contains(&hot.temperature));
        assert!((-100..=100).contains(&hot.tint));
    }

    #[test]
    fn dark_and_clipped_samples_are_refused() {
        assert!(refuse([0.001, 0.001, 0.001]).is_some());
        assert!(refuse([0.99, 0.5, 0.5]).is_some());
        assert!(refuse([0.4, 0.4, 0.4]).is_none());
    }
}
