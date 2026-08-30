//! Automatic tone (ADR 0088 §1–§2): looking at a photo and deciding where
//! its tone sliders should sit.
//!
//! **This module renders nothing that is kept.** Its whole output is five
//! numbers a client then writes through an ordinary `EditSession`, so an
//! automatic tone lands in the history as one revision like any other, and
//! `docs/pipeline.md` §5.1 is untouched by construction: no stage, no stage
//! version, nothing new in `settings_json`. Improving what this file decides
//! changes what the *next* press produces and leaves every revision already
//! written exactly where it is.

use leyline_core::Settings;
use leyline_preview::Rgb8;

/// What one press of Auto proposes (ADR 0088 §2).
///
/// `contrast` is deliberately absent. It is the one tone slider whose right
/// value depends on intent rather than on the histogram, and an Auto that
/// guesses it is an Auto people stop pressing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoTone {
    /// Exposure compensation, EV.
    pub exposure: f64,
    /// Highlight recovery, [-100, 100].
    pub highlights: i32,
    /// Shadow lift, [-100, 100].
    pub shadows: i32,
    /// White point, [-100, 100].
    pub whites: i32,
    /// Black point, [-100, 100].
    pub blacks: i32,
}

/// Middle grey, linear — where a correctly exposed photo's median sits.
const MIDDLE_GREY: f64 = 0.18;

/// The most Auto will move the exposure, in EV either way.
///
/// A cap and not a target: a photo whose median is eight stops out is a
/// night shot or a studio high key, i.e. a deliberate exposure, and an Auto
/// that "corrects" it to middle grey has destroyed the picture rather than
/// developed it.
const MAX_EV: f64 = 2.0;

/// Where the top of the range should land, display-referred: below clipping,
/// with enough room that `output_rendering` has something to roll off.
const WHITE_TARGET: f64 = 0.97;

/// Where the bottom should land: above zero, so the blacks keep separation.
const BLACK_TARGET: f64 = 0.02;

/// The percentiles the two ends are measured at. Not the extremes: one
/// specular highlight or one dead pixel must not decide where the whole
/// photo's white point goes.
const WHITE_PERCENTILE: f64 = 0.995;
const BLACK_PERCENTILE: f64 = 0.002;

/// How much of the histogram has to sit in the top (bottom) decile before
/// recovery is proposed at all, and how much means "as much as I have".
const CROWDED_FROM: f64 = 0.02;
const CROWDED_TO: f64 = 0.25;

/// The most recovery Auto proposes. Recovery is the part of this that is a
/// taste rather than a measurement, so it stays conservative: a photograph
/// that needs +100 shadows needs a person, not a button.
const MAX_RECOVERY: i32 = 60;

/// Luma histogram of a render, 256 bins, display-referred.
///
/// Rec. 709's coefficients and not Rec. 2020's, deliberately: this reads an
/// `Rgb8`, which is the *output* of `output_rendering` and therefore sRGB —
/// the working space's own weights would be the wrong ones here.
fn histogram(image: &Rgb8) -> [u64; 256] {
    let mut bins = [0u64; 256];
    for rgb in image.data().chunks_exact(3) {
        let luma =
            0.2126 * f64::from(rgb[0]) + 0.7152 * f64::from(rgb[1]) + 0.0722 * f64::from(rgb[2]);
        bins[(luma.round() as usize).min(255)] += 1;
    }
    bins
}

/// The value at `p` of a histogram, in [0, 1].
fn percentile(bins: &[u64; 256], p: f64) -> f64 {
    let total: u64 = bins.iter().sum();
    if total == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let wanted = (total as f64 * p) as u64;
    let mut seen = 0u64;
    for (value, &count) in bins.iter().enumerate() {
        seen += count;
        if seen >= wanted {
            #[allow(clippy::cast_precision_loss)]
            return value as f64 / 255.0;
        }
    }
    1.0
}

/// The share of the histogram at or above `from`, in [0, 1].
fn mass_above(bins: &[u64; 256], from: f64) -> f64 {
    share(bins, |value| value >= from)
}

/// The share of the histogram at or below `to`, in [0, 1].
fn mass_below(bins: &[u64; 256], to: f64) -> f64 {
    share(bins, |value| value <= to)
}

fn share(bins: &[u64; 256], keep: impl Fn(f64) -> bool) -> f64 {
    let total: u64 = bins.iter().sum();
    if total == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let counted: u64 = bins
        .iter()
        .enumerate()
        .filter(|(value, _)| keep(*value as f64 / 255.0))
        .map(|(_, &count)| count)
        .sum();
    #[allow(clippy::cast_precision_loss)]
    {
        counted as f64 / total as f64
    }
}

/// The sRGB EOTF, to read a display-referred median as the light it stands
/// for — an exposure correction is a gain on light, not on code values.
fn to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Maps a measured crowding into a recovery amount: nothing below
/// [`CROWDED_FROM`], [`MAX_RECOVERY`] at [`CROWDED_TO`], straight in between.
fn recovery(crowding: f64) -> i32 {
    if crowding <= CROWDED_FROM {
        return 0;
    }
    let t = ((crowding - CROWDED_FROM) / (CROWDED_TO - CROWDED_FROM)).min(1.0);
    #[allow(clippy::cast_possible_truncation)]
    {
        (t * f64::from(MAX_RECOVERY)).round() as i32
    }
}

/// The exposure correction a render's histogram asks for, in EV.
///
/// Exact rather than mapped: exposure *is* a gain, so the correction that
/// puts the median on middle grey is a logarithm and not a constant anyone
/// had to choose.
pub(crate) fn exposure_for(bins: &[u64; 256], current: f64) -> f64 {
    let median = to_linear(percentile(bins, 0.5));
    if median <= 0.0 {
        return current;
    }
    let delta = (MIDDLE_GREY / median).log2().clamp(-MAX_EV, MAX_EV);
    // Two decimals: the slider's own resolution, and a number a person can
    // read back off the panel and recognise.
    ((current + delta) * 100.0).round() / 100.0
}

/// The recovery pair a render's histogram asks for.
pub(crate) fn recovery_for(bins: &[u64; 256]) -> (i32, i32) {
    (
        recovery(mass_above(bins, 0.9)),
        recovery(mass_below(bins, 0.05)),
    )
}

/// Whether the white and black points still need moving, and in which
/// direction — the step of a bisection, not a mapping.
///
/// `whites` and `blacks` are the two sliders whose effect is exactly "where
/// does this end of the range land", so they are searched against the real
/// pipeline instead of being guessed through a transfer function nobody
/// wrote down. Everything else here is a measurement; these two are a
/// measurement repeated.
pub(crate) fn ends_error(bins: &[u64; 256]) -> (f64, f64) {
    (
        WHITE_TARGET - percentile(bins, WHITE_PERCENTILE),
        BLACK_TARGET - percentile(bins, BLACK_PERCENTILE),
    )
}

/// How far the search moves a slider per unit of measured error.
///
/// A **rate**, not a result: the two ends land where [`WHITE_TARGET`] and
/// [`BLACK_TARGET`] say, and this only decides how quickly. Too large
/// oscillates, too small does not arrive inside [`SEARCH_STEPS`].
pub(crate) const SEARCH_GAIN: f64 = 200.0;

/// How close to the target counts as arrived — a fifth of a display code
/// value out of 255, which no eye and no histogram reads as a difference.
pub(crate) const SEARCH_TOLERANCE: f64 = 0.01;

/// The most trial renders the two ends get. Each is a proxy render (a few
/// tens of milliseconds), so the ceiling is what keeps a button press a
/// button press.
pub(crate) const SEARCH_STEPS: usize = 5;

/// One step of the search: the slider value that measured error asks for.
pub(crate) fn stepped(current: i32, error: f64) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    let delta = (error * SEARCH_GAIN).round() as i32;
    (current + delta).clamp(-100, 100)
}

/// A settings clone carrying one proposal, for the search's trial renders.
pub(crate) fn with(settings: &Settings, tone: &AutoTone) -> Settings {
    let mut trial = settings.clone();
    trial.exposure = tone.exposure;
    trial.highlights = tone.highlights;
    trial.shadows = tone.shadows;
    trial.whites = tone.whites;
    trial.blacks = tone.blacks;
    trial
}

/// The histogram of one render.
pub(crate) fn histogram_of(image: &Rgb8) -> [u64; 256] {
    histogram(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(value: u8, count: u64) -> [u64; 256] {
        let mut bins = [0u64; 256];
        bins[value as usize] = count;
        bins
    }

    /// A photo already sitting on middle grey is left alone — the case that
    /// matters most, because an Auto that moves a correct photo is an Auto
    /// nobody trusts.
    #[test]
    fn a_correctly_exposed_photo_asks_for_no_exposure_change() {
        // sRGB code value of 0.18 linear.
        let bins = flat(118, 1000);
        assert!(exposure_for(&bins, 0.0).abs() < 0.05);
    }

    #[test]
    fn a_dark_photo_asks_to_be_brightened_and_a_bright_one_darkened() {
        assert!(exposure_for(&flat(40, 1000), 0.0) > 0.5);
        assert!(exposure_for(&flat(220, 1000), 0.0) < -0.5);
    }

    /// The cap is what stops a night shot being "corrected" into a grey one.
    #[test]
    fn the_exposure_correction_is_capped_both_ways() {
        assert_eq!(exposure_for(&flat(1, 1000), 0.0), 2.0);
        assert_eq!(exposure_for(&flat(255, 1000), 0.0), -2.0);
    }

    /// It adds to what is already set rather than replacing it: pressing
    /// Auto on a photo already pushed +1 EV must not throw that away.
    #[test]
    fn the_correction_is_relative_to_the_exposure_already_set() {
        let bins = flat(118, 1000);
        assert!((exposure_for(&bins, 1.0) - 1.0).abs() < 0.05);
    }

    /// An empty or black histogram must not divide by zero or propose
    /// something absurd.
    #[test]
    fn a_degenerate_histogram_proposes_nothing() {
        assert_eq!(exposure_for(&[0u64; 256], 0.3), 0.3);
        assert_eq!(recovery_for(&[0u64; 256]), (0, 0));
    }

    #[test]
    fn recovery_is_proposed_only_where_the_histogram_is_actually_crowded() {
        // Nothing near either end: nothing to recover.
        assert_eq!(recovery_for(&flat(128, 1000)), (0, 0));
        // Everything piled in the highlights: recovery, capped.
        let (highlights, shadows) = recovery_for(&flat(250, 1000));
        assert_eq!(highlights, MAX_RECOVERY);
        assert_eq!(shadows, 0);
        // And the same at the other end.
        let (highlights, shadows) = recovery_for(&flat(3, 1000));
        assert_eq!(highlights, 0);
        assert_eq!(shadows, MAX_RECOVERY);
    }

    /// The search moves toward the target and stops at the slider's range,
    /// never past it.
    #[test]
    fn a_search_step_moves_toward_the_target_and_clamps() {
        assert!(stepped(0, 0.05) > 0, "a dark top asks for more whites");
        assert!(stepped(0, -0.05) < 0, "a clipped top asks for fewer");
        assert_eq!(stepped(90, 1.0), 100);
        assert_eq!(stepped(-90, -1.0), -100);
        assert_eq!(stepped(12, 0.0), 12, "no error, no move");
    }

    /// A histogram whose ends already sit on target asks for no move, which
    /// is what stops the search below from oscillating around a good photo.
    #[test]
    fn ends_already_on_target_report_no_error() {
        let mut bins = [0u64; 256];
        bins[(WHITE_TARGET * 255.0) as usize] = 10;
        bins[(BLACK_TARGET * 255.0) as usize] = 10;
        bins[128] = 980;
        let (white, black) = ends_error(&bins);
        assert!(white.abs() < 0.02, "white error {white}");
        assert!(black.abs() < 0.02, "black error {black}");
    }
}
