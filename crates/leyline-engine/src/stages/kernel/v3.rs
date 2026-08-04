//! Kernel v3 — the measured noise profile and the shrinkage that follows it,
//! the body `noise_luminance::v3` and `noise_color::v3` share (ADR 0072).
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing a stage version that calls
//! this code renders through exactly this code, forever. A change of
//! rendering is a new version module next to this one, never an edit here
//! (ADR 0042 §1). **The table below is part of that freeze**: new
//! measurements mean a new data file *and* new stage versions, never an edit
//! of `noise_profiles_v1.json` (ADR 0072 §2).
//!
//! `kernel::v2` is not superseded — its transform is called from here
//! unchanged, and the `v2` stages keep using its flat-threshold entry point.
//! What this module adds is a threshold that varies pixel by pixel, and the
//! measured model that decides it.

use std::sync::OnceLock;

use rayon::prelude::*;
use serde::Deserialize;

use super::v2::{LEVEL_SIGMA, LEVELS, convolve_b3, soft_threshold};
use crate::render::SensorShot;
use crate::stages::SourceColor;

/// The measured table, frozen with this version (ADR 0072 §2).
const TABLE: &str = include_str!("../../../data/noise_profiles_v1.json");

/// Luma weights of [`crate::pixels::luma`], repeated here rather than
/// imported: they decide how much of each channel's variance ends up in the
/// plane this module thresholds, so they are part of what is frozen.
const LUMA_WEIGHTS: [f32; 3] = [0.2627, 0.6780, 0.0593];

/// Standard deviation assumed for a body the table does not know, in the
/// buffer's own units (ADR 0072 §7): a signal-independent noise of the order
/// the table gives an APS-C body around ISO 800. It is `v2`'s assumption,
/// restated in linear light — an unknown body keeps the denoising it had.
const FALLBACK_SIGMA: f32 = 0.003;

/// Threshold in standard deviations at full strength, luminance. Half
/// strength therefore lands on 3 σ, the textbook value for a soft shrinkage
/// (ADR 0072 §3).
pub(crate) const SIGMAS_LUMA: f32 = 6.0;

/// Same for chrominance, and deliberately *not* `v2`'s 2.5 ratio: chroma is
/// spatially smooth almost everywhere, so an aggressive threshold costs
/// little there — but part of what that ratio stood for is now carried by
/// the measured σ itself, which comes out around 1.7 times larger on a
/// chroma plane than on luma. Counting it twice would flatten real color.
pub(crate) const SIGMAS_CHROMA: f32 = 10.0;

/// A noise model: variance `a · x + b`, per channel of the buffer it
/// describes (ADR 0072 §Contexte).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NoiseModel {
    /// Photon term, proportional to the signal and to the sensitivity.
    pub a: [f32; 3],
    /// Read term, independent of the signal. May be negative in the table —
    /// the fit is allowed to say so, and [`sigma_plane`] floors the variance
    /// rather than the coefficient.
    pub b: [f32; 3],
}

impl NoiseModel {
    /// The model for an unknown body: flat noise, no photon term.
    pub(crate) fn fallback() -> Self {
        NoiseModel {
            a: [0.0; 3],
            b: [FALLBACK_SIGMA * FALLBACK_SIGMA; 3],
        }
    }
}

/// The model to threshold this render with, already expressed in the space
/// of the buffer the stage receives (ADR 0072 §5).
///
/// Everything that can go missing — no shot, no ISO, a body absent from the
/// table, a source that is not a RAW — lands on [`NoiseModel::fallback`],
/// which denoises rather than doing nothing (§7).
pub(crate) fn model_for(
    sensor: Option<&SensorShot>,
    source: SourceColor,
    has_camera_profile: bool,
) -> NoiseModel {
    let SourceColor::Camera { .. } = source else {
        // A JPEG has already been through the body's own denoising and its
        // transfer curve: the measured model describes nothing there.
        return NoiseModel::fallback();
    };
    let Some(measured) =
        sensor.and_then(|s| measured_model(&s.camera_make, &s.camera_model, s.iso))
    else {
        return NoiseModel::fallback();
    };
    transport(measured, source, has_camera_profile)
}

/// Looks the body up in the frozen table and interpolates between the two
/// sensitivities framing `iso` (ADR 0072 §6).
///
/// The comparison is exact up to case and surrounding space. Nothing
/// approximate: a near-miss on a model name would hand back another sensor's
/// measurements, which is worse than having none.
fn measured_model(make: &str, model: &str, iso: f32) -> Option<NoiseModel> {
    if !iso.is_finite() || iso <= 0.0 {
        return None;
    }
    let camera = table()
        .cameras
        .iter()
        .find(|c| same_name(&c.maker, make) && same_name(&c.model, model))?;
    let profiles = camera.profiles.as_slice();
    let (first, last) = (profiles.first()?, profiles.last()?);
    if iso <= first.iso {
        return Some(NoiseModel {
            a: first.a,
            b: first.b,
        });
    }
    if iso >= last.iso {
        return Some(NoiseModel {
            a: last.a,
            b: last.b,
        });
    }
    let upper = profiles.iter().position(|p| p.iso >= iso)?;
    let (low, high) = (&profiles[upper - 1], &profiles[upper]);
    let span = high.iso - low.iso;
    let t = if span > 0.0 {
        (iso - low.iso) / span
    } else {
        0.0
    };
    let mut model = NoiseModel {
        a: [0.0; 3],
        b: [0.0; 3],
    };
    for channel in 0..3 {
        model.a[channel] = low.a[channel] + t * (high.a[channel] - low.a[channel]);
        model.b[channel] = low.b[channel] + t * (high.b[channel] - low.b[channel]);
    }
    Some(model)
}

/// Case- and space-insensitive equality of two camera names.
fn same_name(left: &str, right: &str) -> bool {
    let normalized = |s: &str| {
        s.split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
            .join(" ")
    };
    normalized(left) == normalized(right)
}

/// Carries a model measured on raw sensor counts over to the buffer this
/// stage receives (ADR 0072 §5): the decoder's white balance first, then the
/// color matrix when `input` has already applied one.
fn transport(measured: NoiseModel, source: SourceColor, has_camera_profile: bool) -> NoiseModel {
    let mut model = measured;
    if let SourceColor::Camera {
        multipliers: Some(multipliers),
        ..
    } = source
        && let Some(gains) = channel_gains(multipliers)
    {
        // Scaling a sample by g scales its variance by g², and the model is
        // read at the pre-balance value y/g: a ← g·a, b ← g²·b.
        for (channel, gain) in gains.iter().enumerate() {
            model.a[channel] *= gain;
            model.b[channel] *= gain * gain;
        }
    }
    // With a camera profile in play the buffer is still camera-native at
    // this rank — `camera_profile` converts it later — so there is no matrix
    // to carry the model through (ADR 0035, and `input::v2`'s own test).
    if has_camera_profile {
        return model;
    }
    let SourceColor::Camera {
        to_xyz: Some(to_xyz),
        ..
    } = source
    else {
        return model;
    };
    let Some(matrix) = leyline_color::camera_to_rec2020(to_xyz) else {
        return model;
    };
    let mut carried = NoiseModel {
        a: [0.0; 3],
        b: [0.0; 3],
    };
    for ((a, b), row) in carried
        .a
        .iter_mut()
        .zip(carried.b.iter_mut())
        .zip(matrix.iter())
    {
        for (column, cell) in row.iter().enumerate() {
            // var(Σ m·x) = Σ m²·var(x) for independent channels; the signal
            // itself is taken as the same in all three (grey assumption),
            // which is exactly where the matrix is normalized to sum to one.
            let weight = (*cell as f32).powi(2);
            *a += weight * model.a[column];
            *b += weight * model.b[column];
        }
    }
    carried
}

/// The per-channel gain the decoder applied, normalized the way LibRaw
/// normalizes it — by the smallest positive multiplier.
///
/// Highlight reconstruction divides by the largest instead, and `input::v2`
/// puts that ratio back on the whole image (ADR 0050 §3), so the net gain
/// this returns holds in both modes.
fn channel_gains(multipliers: [f64; 4]) -> Option<[f32; 3]> {
    let smallest = multipliers
        .iter()
        .copied()
        .filter(|m| *m > 0.0)
        .fold(f64::MAX, f64::min);
    if !smallest.is_finite() || smallest <= 0.0 {
        return None;
    }
    let mut gains = [1.0f32; 3];
    for (gain, multiplier) in gains.iter_mut().zip(multipliers) {
        if multiplier <= 0.0 || !multiplier.is_finite() {
            return None;
        }
        *gain = (multiplier / smallest) as f32;
    }
    Some(gains)
}

/// The model of the luma plane: `L = Σ w·y`, so `var(L) = Σ w²·var(y)`.
pub(crate) fn luma_terms(model: &NoiseModel) -> (f32, f32) {
    let mut terms = (0.0, 0.0);
    for (channel, luma_weight) in LUMA_WEIGHTS.iter().enumerate() {
        let weight = luma_weight * luma_weight;
        terms.0 += weight * model.a[channel];
        terms.1 += weight * model.b[channel];
    }
    terms
}

/// The model of one chroma plane: `C_k = y_k − L`, so the channel's own
/// variance enters weighted by `(1 − w_k)²` and every other one by `w_j²`.
pub(crate) fn chroma_terms(model: &NoiseModel, channel: usize) -> (f32, f32) {
    let mut terms = (0.0, 0.0);
    for (other, luma_weight) in LUMA_WEIGHTS.iter().enumerate() {
        let weight = if other == channel {
            1.0 - luma_weight
        } else {
            *luma_weight
        };
        let weight = weight * weight;
        terms.0 += weight * model.a[other];
        terms.1 += weight * model.b[other];
    }
    terms
}

/// The expected noise standard deviation at every pixel, from the signal
/// estimate `signal` — the luma plane, computed once (ADR 0072 §3).
///
/// `scale` is the proxy reduction factor (ADR 0041): reducing an image by
/// `n` averages `n²` samples per output pixel and divides the noise by `n`,
/// so the measured σ has to follow, or a preview would be denoised harder
/// than the export it stands for.
pub(crate) fn sigma_plane(signal: &[f32], a: f32, b: f32, scale: f32) -> Vec<f32> {
    let reduction = if scale.is_finite() && scale > 0.0 && scale < 1.0 {
        scale
    } else {
        1.0
    };
    signal
        .par_iter()
        .map(|&value| reduction * (a * value + b).max(0.0).sqrt())
        .collect()
}

/// [`super::v2::wavelet_denoise`] with a threshold that varies pixel by
/// pixel: `gain · σ_l · sigma[i]` instead of `base · σ_l` (ADR 0072 §3).
///
/// Everything else is `v2`'s operator, called and not copied — same B3
/// kernel, same holes, same soft shrinkage, same residual left untouched so
/// the tonality cannot move.
pub(crate) fn wavelet_denoise_adaptive(
    plane: &mut [f32],
    width: usize,
    height: usize,
    gain: f32,
    sigma: &[f32],
    levels: usize,
) {
    if gain <= 0.0 || levels == 0 || width == 0 || height == 0 || sigma.len() != plane.len() {
        return;
    }
    let mut current = plane.to_vec();
    for value in plane.iter_mut() {
        *value = 0.0;
    }
    for level in 0..levels {
        let spacing = 1usize << level;
        let next = convolve_b3(&current, width, height, spacing);
        let level_gain = gain * LEVEL_SIGMA[level.min(LEVELS - 1)];
        plane
            .par_iter_mut()
            .zip(current.par_iter())
            .zip(next.par_iter())
            .zip(sigma.par_iter())
            .for_each(|(((out, &coarse_in), &coarse_out), &deviation)| {
                *out += soft_threshold(coarse_in - coarse_out, level_gain * deviation);
            });
        current = next;
    }
    plane
        .par_iter_mut()
        .zip(current.par_iter())
        .for_each(|(out, &residual)| *out += residual);
}

/// The parsed table, read once per process on first use — never at startup:
/// a library that only imports files never pays for it.
fn table() -> &'static Table {
    static TABLE_ONCE: OnceLock<Table> = OnceLock::new();
    TABLE_ONCE
        .get_or_init(|| serde_json::from_str(TABLE).expect("the frozen noise profile table parses"))
}

#[derive(Debug, Deserialize)]
struct Table {
    cameras: Vec<Camera>,
}

#[derive(Debug, Deserialize)]
struct Camera {
    maker: String,
    model: String,
    profiles: Vec<Measured>,
}

#[derive(Debug, Deserialize)]
struct Measured {
    iso: f32,
    a: [f32; 3],
    b: [f32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENSOR: fn(f32) -> SensorShot = |iso| SensorShot {
        camera_make: "Canon".into(),
        camera_model: "EOS 60D".into(),
        iso,
    };

    const RAW: SourceColor = SourceColor::Camera {
        to_xyz: None,
        multipliers: None,
    };

    /// The frozen table parses, and holds what ADR 0072 §1 says it holds.
    /// A table that silently lost half its bodies would denoise every one of
    /// them with the fallback and nothing would say so.
    #[test]
    fn the_frozen_table_holds_every_body() {
        let table = table();
        assert_eq!(table.cameras.len(), 434);
        let profiles: usize = table.cameras.iter().map(|c| c.profiles.len()).sum();
        assert_eq!(profiles, 7842);
        // Sorted by sensitivity, which the interpolation below relies on.
        for camera in &table.cameras {
            assert!(
                camera.profiles.windows(2).all(|w| w[0].iso < w[1].iso),
                "{} {} is not sorted by ISO",
                camera.maker,
                camera.model
            );
        }
    }

    /// An exact sensitivity comes back as measured, not interpolated with a
    /// neighbour.
    #[test]
    fn an_exact_sensitivity_is_the_measured_one() {
        let model = measured_model("Canon", "EOS 60D", 3200.0).unwrap();
        assert!((model.a[1] - 9.68587e-05).abs() < 1e-10, "{:?}", model.a);
    }

    /// Between two measured sensitivities the coefficients are interpolated,
    /// and the result sits strictly between its neighbours (ADR 0072 §6).
    #[test]
    fn a_sensitivity_between_two_lands_between_them() {
        let low = measured_model("Canon", "EOS 60D", 640.0).unwrap();
        let high = measured_model("Canon", "EOS 60D", 800.0).unwrap();
        let middle = measured_model("Canon", "EOS 60D", 720.0).unwrap();
        assert!(middle.a[1] > low.a[1] && middle.a[1] < high.a[1]);
        // Halfway in ISO is halfway in the coefficient.
        let want = (low.a[1] + high.a[1]) / 2.0;
        assert!((middle.a[1] - want).abs() < want * 1e-3);
    }

    /// Outside the measured range the ends hold rather than extrapolate: a
    /// body pushed to an unmeasured extension keeps the closest real
    /// measurement instead of a number nobody took.
    #[test]
    fn the_ends_of_the_ladder_hold() {
        let lowest = measured_model("Canon", "EOS 60D", 100.0).unwrap();
        assert_eq!(measured_model("Canon", "EOS 60D", 25.0), Some(lowest));
        let highest = measured_model("Canon", "EOS 60D", 12800.0).unwrap();
        assert_eq!(measured_model("Canon", "EOS 60D", 51200.0), Some(highest));
    }

    /// Case and spacing do not decide a match; a different body does.
    #[test]
    fn a_body_is_matched_by_name_not_by_resemblance() {
        assert!(measured_model("canon", "eos  60d", 400.0).is_some());
        assert!(measured_model("Canon", "EOS 60", 400.0).is_none());
        assert!(measured_model("Nikon", "EOS 60D", 400.0).is_none());
    }

    /// Noise grows with sensitivity — the whole point of the table. Two
    /// stops up must show it, not merely differ.
    #[test]
    fn noise_grows_with_sensitivity() {
        let (low, high) = (
            measured_model("Canon", "EOS 60D", 400.0).unwrap(),
            measured_model("Canon", "EOS 60D", 1600.0).unwrap(),
        );
        assert!(high.a[1] > low.a[1] * 3.0, "{} vs {}", high.a[1], low.a[1]);
    }

    /// Everything missing lands on the fallback rather than on nothing: the
    /// stage always denoises (ADR 0072 §7).
    #[test]
    fn a_missing_profile_falls_back_instead_of_giving_up() {
        assert_eq!(model_for(None, RAW, false), NoiseModel::fallback());
        assert_eq!(
            model_for(Some(&SENSOR(f32::NAN)), RAW, false),
            NoiseModel::fallback()
        );
        assert_eq!(
            model_for(Some(&SENSOR(400.0)), SourceColor::Srgb, false),
            NoiseModel::fallback()
        );
        let unknown = SensorShot {
            camera_make: "Acme".into(),
            camera_model: "Box Brownie".into(),
            iso: 400.0,
        };
        assert_eq!(
            model_for(Some(&unknown), RAW, false),
            NoiseModel::fallback()
        );
    }

    /// The decoder's white balance multiplies the variance, and the model
    /// has to follow it channel by channel (ADR 0072 §5).
    #[test]
    fn white_balance_carries_the_model() {
        let source = SourceColor::Camera {
            to_xyz: None,
            multipliers: Some([2.0, 1.0, 1.5, 0.0]),
        };
        let measured = measured_model("Canon", "EOS 60D", 800.0).unwrap();
        let carried = transport(measured, source, true);
        assert!((carried.a[0] - measured.a[0] * 2.0).abs() < 1e-12);
        assert!((carried.b[0] - measured.b[0] * 4.0).abs() < 1e-14);
        assert!((carried.a[1] - measured.a[1]).abs() < 1e-12);
    }

    /// A camera profile means the buffer is still camera-native at this
    /// rank, so no matrix is applied to the model either.
    #[test]
    fn a_camera_profile_leaves_the_model_in_camera_space() {
        let source = SourceColor::Camera {
            to_xyz: Some([[0.7, 0.2, 0.1], [0.2, 0.6, 0.2], [0.1, 0.2, 0.7]]),
            multipliers: None,
        };
        let measured = measured_model("Canon", "EOS 60D", 800.0).unwrap();
        assert_eq!(transport(measured, source, true), measured);
        assert_ne!(transport(measured, source, false), measured);
    }

    /// σ follows the proxy factor: a preview reduced four times sees noise
    /// already divided by four (ADR 0072 §3).
    #[test]
    fn sigma_follows_the_proxy_scale() {
        let signal = vec![0.18f32; 4];
        let full = sigma_plane(&signal, 1e-4, 1e-8, 1.0);
        let proxy = sigma_plane(&signal, 1e-4, 1e-8, 0.25);
        assert!((proxy[0] - full[0] * 0.25).abs() < 1e-9);
    }

    /// A negative read term is a fit artefact, not a negative variance: the
    /// floor is on the variance, so the σ stays real.
    #[test]
    fn a_negative_read_term_floors_at_zero() {
        let sigma = sigma_plane(&[0.0, 0.5], 1e-4, -1e-6, 1.0);
        assert_eq!(sigma[0], 0.0);
        assert!(sigma[1] > 0.0);
    }

    /// The adaptive threshold is the flat one where σ is flat — the two
    /// operators are the same transform, and this is what says so.
    #[test]
    fn a_flat_sigma_reproduces_the_flat_threshold() {
        let (w, h) = (24, 16);
        let source: Vec<f32> = (0..w * h).map(|i| ((i * 53) % 97) as f32 / 97.0).collect();
        let mut flat = source.clone();
        super::super::v2::wavelet_denoise(&mut flat, w, h, 0.03, LEVELS);
        let mut adaptive = source.clone();
        wavelet_denoise_adaptive(&mut adaptive, w, h, 0.03, &vec![1.0; w * h], LEVELS);
        for (got, want) in adaptive.iter().zip(&flat) {
            assert!((got - want).abs() < 1e-6, "{got} != {want}");
        }
    }

    /// A σ plane of the wrong length denoises nothing rather than
    /// thresholding part of the image: silently zipping to the shorter of
    /// the two would leave a visible seam.
    #[test]
    fn a_mismatched_sigma_plane_is_refused() {
        let (w, h) = (8, 8);
        let source: Vec<f32> = (0..w * h).map(|i| i as f32 / 64.0).collect();
        let mut plane = source.clone();
        wavelet_denoise_adaptive(&mut plane, w, h, 1.0, &[0.1; 3], LEVELS);
        assert_eq!(plane, source);
    }
}
