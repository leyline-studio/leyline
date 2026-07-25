//! Process version 10 — the tenth rendering contract of the develop
//! pipeline (ADR 0033).
//!
//! This module *is* the definition of `process: 10`: the exact formulas
//! below, applied in the fixed order of `docs/pipeline.md` §3.1, are
//! frozen. Any change that alters the pixels they produce must land as a
//! new process version in a new module — this one is kept as-is forever
//! (§3.3).
//!
//! Process 10 differs from process 9 in exactly one place: clarity
//! ([`local_contrast`] at a large radius), texture ([`local_contrast`] at a
//! small radius) and dehaze ([`dehaze`]) run right after the tone curve and
//! before Vibrance/Saturation — ADR 0033's placement, in the tonal/presence
//! block, ahead of the color block ADR 0031 (process 9) added. Clarity and
//! texture are **the same algorithm** — the unsharp-mask local-contrast
//! boost `sharpen` above already uses (`L' = L + amount·(L − blur(L))`) —
//! called twice with different radius constants; ADR 0033 is explicit that
//! sharing code *within* one frozen module is fine, only sharing *between*
//! frozen modules is what ADR 0028 forbids. Clarity's radius is far too
//! large for a literal full-resolution Gaussian kernel to stay affordable,
//! so its blur goes through [`approx_blur`] (downsample → small blur →
//! bilinear upsample) instead of `sharpen`'s literal [`gaussian_blur`].
//! Dehaze is a dark-channel-prior haze removal: every step — the dark
//! channel's local-minimum filter, the atmospheric-light percentile
//! selection, the transmission estimate — is closed-form and deterministic,
//! no iterative solver, so it stays reproducible once frozen with this
//! process version (`docs/pipeline.md` §5). Every other operator is copied
//! from process 9 unchanged, so this module stays self-contained and
//! frozen.
//!
//! Design rules shared by every operator:
//!
//! * a parameter at its neutral value skips its operator entirely, so the
//!   neutral rendering is bit-for-bit the decoded image;
//! * operators are pure and deterministic; their loops may run rows in
//!   parallel (ADR 0012), but every sample is computed by the same scalar
//!   formula in the same order regardless of thread count, so the output
//!   is bit-for-bit identical to a single-threaded run;
//! * the working buffer stays gamma-encoded sRGB in [0, 1] between steps
//!   (see [`crate::pixels`]); white balance and exposure convert to linear
//!   light internally.

use std::cell::RefCell;
use std::sync::OnceLock;

use leyline_core::Result;
use leyline_core::{
    ColorGrading, ColorGradingZone, Crop, CurvePoint, HslBand, LocalAdjustment, Point, Settings,
    SpotRemoval, WhiteBalance,
};
use leyline_raw::RawImage;
use rayon::prelude::*;

use crate::mask;
use crate::pixels::{Pixels, luma};
use crate::render::{LensShot, Rendered};

// ---------------------------------------------------------------------------
// Transfer functions by lookup table (unchanged from process 2)
// ---------------------------------------------------------------------------

/// Number of intervals in each transfer lookup table. Frozen: changing it
/// changes the pixels, i.e. requires a new process version.
const LUT_SIZE: usize = 4096;

/// The exact sRGB EOTF, used only to build the table.
fn exact_srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The exact sRGB OETF, used only to build the table.
fn exact_linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// `LUT_SIZE + 1` samples of `f` over [0, 1], entry `i` at `i / LUT_SIZE`.
fn build_table(f: fn(f32) -> f32) -> [f32; LUT_SIZE + 1] {
    let mut table = [0.0f32; LUT_SIZE + 1];
    for (i, entry) in table.iter_mut().enumerate() {
        *entry = f(i as f32 / LUT_SIZE as f32);
    }
    table
}

/// Interpolated lookup: `v` clamped to [0, 1], linear blend between the
/// two surrounding entries.
fn lookup(table: &[f32; LUT_SIZE + 1], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * LUT_SIZE as f32;
    let i = (x as usize).min(LUT_SIZE - 1);
    let t = x - i as f32;
    table[i] + (table[i + 1] - table[i]) * t
}

/// The two tables, built once per process.
fn tables() -> &'static ([f32; LUT_SIZE + 1], [f32; LUT_SIZE + 1]) {
    static TABLES: OnceLock<([f32; LUT_SIZE + 1], [f32; LUT_SIZE + 1])> = OnceLock::new();
    TABLES.get_or_init(|| {
        (
            build_table(exact_srgb_to_linear),
            build_table(exact_linear_to_srgb),
        )
    })
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Runs `op` on every pixel row of the interleaved buffer, in parallel.
fn par_rows(px: &mut Pixels, op: impl Fn(&mut [f32]) + Send + Sync) {
    let row = px.width as usize * 3;
    px.data.par_chunks_mut(row).for_each(op);
}

/// Renders a decoded image according to `settings`, which the caller has
/// already validated and confirmed to declare `process: 10`. `shot` is the
/// EXIF identification needed to look up a Lensfun profile; `None` when the
/// caller has none (e.g. no camera/lens metadata on the asset).
pub(crate) fn develop(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
) -> Result<Rendered> {
    let mut px = Pixels::from_raw(image)?;

    if settings.lens_correction.enabled {
        if let Some(shot) = shot {
            if let Some(profile) = leyline_lens::find_profile(
                &shot.camera_make,
                &shot.camera_model,
                shot.lens_make.as_deref(),
                shot.lens_model.as_deref().unwrap_or(""),
            ) {
                let correction =
                    leyline_lens::Correction::new(&profile, shot.focal_mm, px.width, px.height);
                px = undistort(&px, &correction);
                px = correct_tca(&px, &correction);
                if let Some(aperture_f) = shot.aperture_f {
                    devignette(&mut px, &profile, shot.focal_mm, aperture_f);
                }
            }
        }
    }

    if !settings.spot_removal.is_empty() {
        spot_removal(&mut px, &settings.spot_removal, settings.rotation);
    }

    if settings.white_balance.is_some() || settings.exposure != 0.0 {
        linear_gains(&mut px, settings.white_balance.as_ref(), settings.exposure);
    }
    if settings.contrast != 0 {
        contrast(&mut px, settings.contrast);
    }
    if settings.highlights != 0 || settings.shadows != 0 {
        highlights_shadows(&mut px, settings.highlights, settings.shadows);
    }
    if settings.whites != 0 || settings.blacks != 0 {
        whites_blacks(&mut px, settings.whites, settings.blacks);
    }
    if !settings.tone_curve.points.is_empty() {
        tone_curve(&mut px, &settings.tone_curve.points);
    }
    if settings.clarity != 0 {
        local_contrast(&mut px, settings.clarity, CLARITY_RADIUS);
    }
    if settings.texture != 0 {
        local_contrast(&mut px, settings.texture, TEXTURE_RADIUS);
    }
    if settings.dehaze != 0 {
        dehaze(&mut px, settings.dehaze);
    }
    if settings.vibrance != 0 {
        saturate(&mut px, settings.vibrance, true);
    }
    if settings.saturation != 0 {
        saturate(&mut px, settings.saturation, false);
    }
    if settings.hsl.iter().any(|band| *band != HslBand::default()) {
        hsl_mixer(&mut px, &settings.hsl);
    }
    if settings.color_grading != ColorGrading::default() {
        color_grading(&mut px, &settings.color_grading);
    }
    if !settings.local_adjustments.is_empty() {
        local_adjustments(&mut px, &settings.local_adjustments, settings.rotation);
    }
    if settings.noise_reduction.luminance != 0 {
        luminance_noise_reduction(&mut px, settings.noise_reduction.luminance);
    }
    if settings.noise_reduction.color != 0 {
        color_noise_reduction(&mut px, settings.noise_reduction.color);
    }
    if settings.sharpening.amount != 0 {
        sharpen(
            &mut px,
            settings.sharpening.amount,
            settings.sharpening.radius,
        );
    }
    if settings.rotation.rem_euclid(360.0) != 0.0 {
        px = rotate(&px, settings.rotation);
    }
    if let Some(rect) = &settings.crop {
        px = crop(&px, rect);
    }

    Ok(Rendered {
        width: px.width,
        height: px.height,
        data: px.to_rgb8(),
    })
}

// ---------------------------------------------------------------------------
// Lens correction (process 4's distortion and vignetting, carried
// unchanged, plus process 5's TCA)
// ---------------------------------------------------------------------------

/// Undistorts the image geometrically using an already-built [`leyline_lens::Correction`].
/// No distortion calibration at this focal length leaves `px` unchanged:
/// correction is only ever applied from real calibration data, never
/// approximated. `correction` is shared with [`correct_tca`] — one profile
/// match, one `Correction` built, both geometric passes reuse it.
fn undistort(px: &Pixels, correction: &leyline_lens::Correction) -> Pixels {
    if !correction.distortion_matched() {
        // No distortion calibration at this focal length: `source_row`
        // would return the identity map, and resampling at identity
        // coordinates is bit-identical to the original pixel (integer
        // coordinates make every bilinear weight exactly 0.0 or 1.0) — so
        // skip the whole per-pixel pass rather than pay for a no-op.
        return px.clone();
    }
    // Reused per render thread rather than allocated fresh per row: `source_row`
    // computes the same coordinates either way, this only spares the two Vec
    // allocations `source_row` would otherwise make on every one of the
    // image's rows.
    type SourceRowScratch = RefCell<(Vec<f32>, Vec<(f32, f32)>)>;
    thread_local! {
        static SCRATCH: SourceRowScratch = const { RefCell::new((Vec::new(), Vec::new())) };
    }
    let mut data = vec![0.0f32; px.data.len()];
    data.par_chunks_mut(px.width as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            SCRATCH.with(|cell| {
                let (scratch, sources) = &mut *cell.borrow_mut();
                correction.source_row_into(y as u32, px.width, scratch, sources);
                for (x, rgb_out) in row.chunks_exact_mut(3).enumerate() {
                    let (sx, sy) = sources[x];
                    if let Some(rgb) = lens_bilinear(px, sx, sy) {
                        rgb_out.copy_from_slice(&rgb);
                    }
                }
            });
        });
    Pixels {
        width: px.width,
        height: px.height,
        data,
    }
}

/// Corrects transverse chromatic aberration: each channel of each output
/// pixel is resampled independently from its own Lensfun-reported source
/// coordinate, on top of the buffer [`undistort`] already produced (see the
/// module docs for why this runs as a second independent pass rather than a
/// combined distortion+TCA remap). No TCA calibration at this focal length
/// leaves every channel's coordinate at the identity, so `px` comes back
/// unchanged.
fn correct_tca(px: &Pixels, correction: &leyline_lens::Correction) -> Pixels {
    if !correction.tca_matched() {
        // No TCA calibration at this focal length: `tca_row` would map
        // every channel to the same identity coordinate, and resampling at
        // an identity coordinate is bit-identical to the original pixel
        // (integer coordinates make every bilinear weight exactly 0.0 or
        // 1.0) — so skip the whole per-channel pass rather than pay for a
        // no-op that discards nothing new but still costs three independent
        // resamples per pixel.
        return px.clone();
    }
    // Same reuse-per-thread rationale as `undistort`'s scratch buffers above.
    type TcaRowScratch = RefCell<(Vec<f32>, Vec<[(f32, f32); 3]>)>;
    thread_local! {
        static SCRATCH: TcaRowScratch = const { RefCell::new((Vec::new(), Vec::new())) };
    }
    let mut data = vec![0.0f32; px.data.len()];
    data.par_chunks_mut(px.width as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            SCRATCH.with(|cell| {
                let (scratch, channels) = &mut *cell.borrow_mut();
                correction.tca_row_into(y as u32, px.width, scratch, channels);
                for (x, rgb_out) in row.chunks_exact_mut(3).enumerate() {
                    for (c, &(sx, sy)) in channels[x].iter().enumerate() {
                        if let Some(value) = lens_bilinear_channel(px, sx, sy, c) {
                            rgb_out[c] = value;
                        }
                    }
                }
            });
        });
    Pixels {
        width: px.width,
        height: px.height,
        data,
    }
}

/// Corrects corner darkening (vignetting) using an already-matched Lensfun
/// profile, `aperture_f` and [`leyline_lens::Vignetting`]'s assumed subject
/// distance. No vignetting calibration for this focal/aperture pair leaves
/// `px` unchanged. The gain is a radial multiplier defined in linear light
/// (a physical falloff of incoming light), so each sample round-trips
/// through the same transfer tables as [`linear_gains`] rather than being
/// multiplied directly in gamma space.
fn devignette(px: &mut Pixels, profile: &leyline_lens::Profile, focal_mm: f32, aperture_f: f32) {
    let vignetting =
        leyline_lens::Vignetting::new(profile, focal_mm, aperture_f, px.width, px.height);
    if !vignetting.matched() {
        return;
    }
    let (to_linear, to_srgb) = tables();
    let width = px.width;
    px.data
        .par_chunks_mut(width as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            let gains = vignetting.gain_row(y as u32, width);
            for (x, rgb) in row.chunks_exact_mut(3).enumerate() {
                let gain = gains[x];
                for sample in rgb {
                    *sample = lookup(to_srgb, (lookup(to_linear, *sample) * gain).clamp(0.0, 1.0));
                }
            }
        });
}

/// Bilinear sample in Lensfun's own pixel convention: centers at integer
/// coordinates `0..width-1` / `0..height-1`, unlike [`bilinear`] below whose
/// `n + 0.5` convention is this module's own choice for `rotate`. `None`
/// outside the source frame — the caller leaves those samples black, same
/// as an out-of-frame rotation.
fn lens_bilinear(px: &Pixels, sx: f32, sy: f32) -> Option<[f32; 3]> {
    let (w, h) = (px.width as f32, px.height as f32);
    if sx < 0.0 || sy < 0.0 || sx > w - 1.0 || sy > h - 1.0 {
        return None;
    }
    let x0 = sx.floor() as usize;
    let y0 = sy.floor() as usize;
    let x1 = (x0 + 1).min(px.width as usize - 1);
    let y1 = (y0 + 1).min(px.height as usize - 1);
    let (tx, ty) = (sx - x0 as f32, sy - y0 as f32);

    let at = |x: usize, y: usize, c: usize| px.data[(y * px.width as usize + x) * 3 + c];
    let mut rgb = [0.0f32; 3];
    for (c, value) in rgb.iter_mut().enumerate() {
        let top = at(x0, y0, c) * (1.0 - tx) + at(x1, y0, c) * tx;
        let bottom = at(x0, y1, c) * (1.0 - tx) + at(x1, y1, c) * tx;
        *value = top * (1.0 - ty) + bottom * ty;
    }
    Some(rgb)
}

/// Same convention and bounds check as [`lens_bilinear`], but interpolates a
/// single channel `c` instead of all three. [`correct_tca`] samples each
/// channel at its own TCA-shifted coordinate and only ever keeps that one
/// channel's result, so computing (and discarding) the other two via
/// [`lens_bilinear`] was pure waste. The arithmetic below is copied
/// byte-for-byte from `lens_bilinear`'s per-channel computation — same
/// coefficients, same multiply/add order — so the surviving channel is
/// bit-identical to what `lens_bilinear(px, sx, sy).unwrap()[c]` would have
/// produced.
fn lens_bilinear_channel(px: &Pixels, sx: f32, sy: f32, c: usize) -> Option<f32> {
    let (w, h) = (px.width as f32, px.height as f32);
    if sx < 0.0 || sy < 0.0 || sx > w - 1.0 || sy > h - 1.0 {
        return None;
    }
    let x0 = sx.floor() as usize;
    let y0 = sy.floor() as usize;
    let x1 = (x0 + 1).min(px.width as usize - 1);
    let y1 = (y0 + 1).min(px.height as usize - 1);
    let (tx, ty) = (sx - x0 as f32, sy - y0 as f32);

    let at = |x: usize, y: usize| px.data[(y * px.width as usize + x) * 3 + c];
    let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
    let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
    Some(top * (1.0 - ty) + bottom * ty)
}

// ---------------------------------------------------------------------------
// White balance + exposure (linear light, through the tables)
// ---------------------------------------------------------------------------

/// Applies white balance and exposure as per-channel gains in linear light.
fn linear_gains(px: &mut Pixels, wb: Option<&WhiteBalance>, exposure_ev: f64) {
    let mut gains = [1.0f64; 3];
    if let Some(wb) = wb {
        let reference = blackbody_rgb(6500.0);
        let target = blackbody_rgb(f64::from(wb.temperature));
        for c in 0..3 {
            gains[c] = (reference[c] / target[c]).clamp(0.1, 10.0);
        }
        // Normalize on green so white balance alone does not change exposure.
        let green = gains[1];
        for gain in &mut gains {
            *gain /= green;
        }
        gains[1] *= 2.0f64.powf(-f64::from(wb.tint) / 200.0);
    }
    let gain = 2.0f64.powf(exposure_ev);
    let gains = gains.map(|g| (g * gain) as f32);

    let (to_linear, to_srgb) = tables();
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            for (sample, gain) in rgb.iter_mut().zip(gains) {
                *sample = lookup(to_srgb, (lookup(to_linear, *sample) * gain).clamp(0.0, 1.0));
            }
        }
    });
}

/// Approximate color of a blackbody radiator, gamma-encoded RGB in (0, 1].
///
/// Tanner Helland's polynomial fit, frozen as part of process 2 and carried
/// unchanged into process 3, process 4 and process 5. Channels are floored
/// at 0.01 so gain ratios
/// stay finite at extreme temperatures.
fn blackbody_rgb(kelvin: f64) -> [f64; 3] {
    let t = kelvin.clamp(1000.0, 40000.0) / 100.0;
    let r = if t <= 66.0 {
        255.0
    } else {
        329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
    };
    let g = if t <= 66.0 {
        99.470_802_586_1 * t.ln() - 161.119_568_166_1
    } else {
        288.122_169_528_3 * (t - 60.0).powf(-0.075_514_849_2)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7
    };
    [r, g, b].map(|v| (v.clamp(0.0, 255.0) / 255.0).max(0.01))
}

// ---------------------------------------------------------------------------
// Tone (gamma domain)
// ---------------------------------------------------------------------------

/// S-curve around middle gray, per channel.
fn contrast(px: &mut Pixels, amount: i32) {
    let k = f32::from(amount as i16) / 100.0;
    par_rows(px, |row| {
        for sample in row {
            let x = *sample;
            *sample = if k >= 0.0 {
                let s = x * x * (3.0 - 2.0 * x);
                ((1.0 - k) * x + k * s).clamp(0.0, 1.0)
            } else {
                let flat = 0.25 + 0.5 * x;
                ((1.0 + k) * x - k * flat).clamp(0.0, 1.0)
            };
        }
    });
}

/// Luma-masked tone adjustments: `shadows` acts on dark pixels with weight
/// `(1 − L)²`, `highlights` on bright pixels with weight `L²`.
fn highlights_shadows(px: &mut Pixels, highlights: i32, shadows: i32) {
    let h = f32::from(highlights as i16) / 100.0;
    let s = f32::from(shadows as i16) / 100.0;
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let l = luma(rgb);
            let delta = 0.5 * (s * (1.0 - l) * (1.0 - l) + h * l * l);
            for sample in rgb {
                let x = *sample;
                let moved = if delta >= 0.0 {
                    x + delta * (1.0 - x)
                } else {
                    x + delta * x
                };
                *sample = moved.clamp(0.0, 1.0);
            }
        }
    });
}

/// Endpoint remapping: positive `whites` brightens by lowering the white
/// point, positive `blacks` lifts the black point (negative values crush).
fn whites_blacks(px: &mut Pixels, whites: i32, blacks: i32) {
    let white = 1.0 - f32::from(whites as i16) / 100.0 * 0.25;
    let black = -f32::from(blacks as i16) / 100.0 * 0.25;
    let scale = 1.0 / (white - black);
    par_rows(px, |row| {
        for sample in row {
            *sample = ((*sample - black) * scale).clamp(0.0, 1.0);
        }
    });
}

// ---------------------------------------------------------------------------
// Tone curve (gamma domain, luminance — ADR 0030)
// ---------------------------------------------------------------------------

/// Number of intervals in the tone curve lookup table. Frozen alongside
/// [`LUT_SIZE`]: changing it changes the pixels, i.e. requires a new process
/// version.
const CURVE_LUT_SIZE: usize = 4096;

/// Applies the tone curve identically to every channel, via an interpolated
/// lookup into a table precomputed once from `points` (ADR 0030: LUT, not a
/// per-pixel spline evaluation).
fn tone_curve(px: &mut Pixels, points: &[CurvePoint]) {
    let table = build_curve_lut(points);
    par_rows(px, |row| {
        for sample in row {
            *sample = curve_lookup(&table, *sample);
        }
    });
}

/// Interpolated lookup into a `CURVE_LUT_SIZE + 1`-entry table spanning
/// [0, 1], same convention as [`lookup`] above but a fixed-size array of a
/// different length, hence its own function.
fn curve_lookup(table: &[f32; CURVE_LUT_SIZE + 1], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * CURVE_LUT_SIZE as f32;
    let i = (x as usize).min(CURVE_LUT_SIZE - 1);
    let t = x - i as f32;
    table[i] + (table[i + 1] - table[i]) * t
}

/// Precomputes the tone curve into a lookup table: a monotone cubic Hermite
/// spline (Fritsch–Carlson) through `points`, evaluated once per table entry
/// at construction time (ADR 0030). `points` must have at least 2 entries
/// with strictly increasing `x` in [0, 1] — the caller (`Settings::validate`)
/// guarantees this. Inputs below the first or above the last control point
/// clamp to that point's `y`.
fn build_curve_lut(points: &[CurvePoint]) -> [f32; CURVE_LUT_SIZE + 1] {
    let n = points.len();
    let xs: Vec<f64> = points.iter().map(|p| p.x).collect();
    let ys: Vec<f64> = points.iter().map(|p| p.y).collect();

    // Secant slopes between consecutive points.
    let secants: Vec<f64> = (0..n - 1)
        .map(|i| (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]))
        .collect();

    // Initial tangents: the secant at the endpoints, the average of the two
    // adjacent secants elsewhere.
    let mut tangents = vec![0.0f64; n];
    tangents[0] = secants[0];
    tangents[n - 1] = secants[n - 2];
    for i in 1..n - 1 {
        tangents[i] = (secants[i - 1] + secants[i]) / 2.0;
    }

    // Fritsch–Carlson monotonicity adjustment: clamp each tangent pair
    // against its shared secant so the interpolant never overshoots.
    for i in 0..n - 1 {
        let d = secants[i];
        if d == 0.0 {
            tangents[i] = 0.0;
            tangents[i + 1] = 0.0;
            continue;
        }
        let alpha = tangents[i] / d;
        let beta = tangents[i + 1] / d;
        let magnitude = alpha.hypot(beta);
        if magnitude > 3.0 {
            let tau = 3.0 / magnitude;
            tangents[i] = tau * alpha * d;
            tangents[i + 1] = tau * beta * d;
        }
    }

    let mut table = [0.0f32; CURVE_LUT_SIZE + 1];
    for (i, entry) in table.iter_mut().enumerate() {
        let x = i as f64 / CURVE_LUT_SIZE as f64;
        *entry = eval_hermite(&xs, &ys, &tangents, x) as f32;
    }
    table
}

/// Evaluates the monotone cubic Hermite spline built by [`build_curve_lut`]
/// at `x`, clamping to the first/last control point's `y` outside `[x0,
/// x_last]`.
fn eval_hermite(xs: &[f64], ys: &[f64], tangents: &[f64], x: f64) -> f64 {
    if x <= xs[0] {
        return ys[0];
    }
    let last = xs.len() - 1;
    if x >= xs[last] {
        return ys[last];
    }
    let i = match xs.binary_search_by(|probe| probe.partial_cmp(&x).unwrap()) {
        Ok(i) => return ys[i],
        Err(i) => i - 1,
    };
    let h = xs[i + 1] - xs[i];
    let t = (x - xs[i]) / h;
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    h00 * ys[i] + h10 * h * tangents[i] + h01 * ys[i + 1] + h11 * h * tangents[i + 1]
}

// ---------------------------------------------------------------------------
// Color (gamma domain)
// ---------------------------------------------------------------------------

/// Scales chroma around the pixel's luma. Plain `saturation` applies the
/// factor uniformly; `vibrance` weights it by `1 − chroma`.
fn saturate(px: &mut Pixels, amount: i32, vibrance: bool) {
    let k = f32::from(amount as i16) / 100.0;
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let l = luma(rgb);
            let factor = if vibrance {
                let chroma = rgb.iter().fold(0.0f32, |m, &v| m.max(v))
                    - rgb.iter().fold(1.0f32, |m, &v| m.min(v));
                1.0 + k * (1.0 - chroma)
            } else {
                1.0 + k
            };
            for sample in rgb {
                *sample = (l + (*sample - l) * factor).clamp(0.0, 1.0);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Clarity, texture and dehaze (ADR 0033)
//
// ADR 0033 freezes the *algorithm family* for all three — local contrast at
// two radii for clarity/texture, dark-channel-prior for dehaze — the exact
// numeric constants below (radii, patch size, percentile, guard factors)
// are the implementation detail the ADR explicitly defers, the same
// latitude ADR 0030/0031 left the tone curve's spline shape and the HSL
// mixer's band centers.
// ---------------------------------------------------------------------------

/// Blur radius (same "sigma" units [`gaussian_blur`] uses) for the clarity
/// stage: a large-radius local contrast boost, the classic "clarity" look.
const CLARITY_RADIUS: f32 = 40.0;
/// Blur radius for the texture stage: fine local contrast, wider than
/// sharpening's own radius range but much narrower than clarity's.
const TEXTURE_RADIUS: f32 = 6.0;

/// Local contrast: the same unsharp-mask math `sharpen` uses
/// (`L' = L + amount·(L − blur(L))`) at a caller-chosen radius — clarity
/// and texture are this one function called twice (ADR 0033), not two
/// independently invented algorithms. Uses [`approx_blur`], not
/// `sharpen`'s literal [`gaussian_blur`]: clarity's radius is large enough
/// that a literal full-resolution kernel would be prohibitively expensive
/// on big previews/exports (`docs/engine-api.md` §11).
fn local_contrast(px: &mut Pixels, amount: i32, radius_px: f32) {
    let k = f32::from(amount as i16) / 100.0;
    let plane = luma_plane(px);
    let blurred = approx_blur(&plane, px.width as usize, px.height as usize, radius_px);
    add_luma_delta(px, |i| k * (plane[i] - blurred[i]));
}

/// How much smaller a side of the working plane gets before blurring, at
/// `sigma` per [`CLARITY_RADIUS`]'s scale — bounds the cost of a large-σ
/// blur regardless of how large the source image is.
const APPROX_BLUR_DOWNSAMPLE_DIVISOR: f32 = 4.0;

/// Approximates a large-radius Gaussian blur via downsample → small blur →
/// bilinear upsample (a single-level Gaussian-pyramid approximation)
/// instead of a literal full-resolution kernel (ADR 0033). At `sigma <= 1`
/// downsampling would lose more than it saves, so this falls back to the
/// literal [`gaussian_blur`] directly.
fn approx_blur(plane: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    if sigma <= 1.0 || width < 2 || height < 2 {
        return gaussian_blur(plane, width, height, sigma);
    }
    let factor = (sigma / APPROX_BLUR_DOWNSAMPLE_DIVISOR).round().max(1.0) as usize;
    if factor <= 1 {
        return gaussian_blur(plane, width, height, sigma);
    }
    let small_w = width.div_ceil(factor).max(1);
    let small_h = height.div_ceil(factor).max(1);

    // Downsample by box-averaging factor×factor blocks.
    let mut small = vec![0.0f32; small_w * small_h];
    small
        .par_chunks_mut(small_w)
        .enumerate()
        .for_each(|(sy, row)| {
            for (sx, out) in row.iter_mut().enumerate() {
                let mut sum = 0.0f32;
                let mut count = 0u32;
                for dy in 0..factor {
                    let y = sy * factor + dy;
                    if y >= height {
                        continue;
                    }
                    for dx in 0..factor {
                        let x = sx * factor + dx;
                        if x >= width {
                            continue;
                        }
                        sum += plane[y * width + x];
                        count += 1;
                    }
                }
                *out = if count > 0 { sum / count as f32 } else { 0.0 };
            }
        });

    // A light extra blur at the reduced scale smooths block-averaging
    // edges; sigma scales down with the same factor the plane did.
    let small_sigma = (sigma / factor as f32).max(0.5);
    let small_blurred = gaussian_blur(&small, small_w, small_h, small_sigma);

    // Bilinear upsample back to full resolution.
    let mut out = vec![0.0f32; width * height];
    out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        let sy = (y as f32 / factor as f32).min(small_h as f32 - 1.0);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(small_h - 1);
        let ty = sy - y0 as f32;
        for (x, out) in row.iter_mut().enumerate() {
            let sx = (x as f32 / factor as f32).min(small_w as f32 - 1.0);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(small_w - 1);
            let tx = sx - x0 as f32;
            let v00 = small_blurred[y0 * small_w + x0];
            let v10 = small_blurred[y0 * small_w + x1];
            let v01 = small_blurred[y1 * small_w + x0];
            let v11 = small_blurred[y1 * small_w + x1];
            let v0 = v00 + (v10 - v00) * tx;
            let v1 = v01 + (v11 - v01) * tx;
            *out = v0 + (v1 - v0) * ty;
        }
    });
    out
}

/// Neighborhood radius (pixels) of the dark channel's local-minimum filter
/// — He et al.'s "patch size", conventionally an odd window around 15px.
const DEHAZE_PATCH_RADIUS: usize = 7;
/// Fraction of the brightest dark-channel pixels considered when picking
/// the atmospheric-light candidate — a closed-form top-percentile select,
/// never an iterative search.
const DEHAZE_TOP_FRACTION: f32 = 0.001;
/// How aggressively the transmission estimate assumes haze is present;
/// `1.0` would fully trust the dark-channel prior, slightly under that
/// (the conventional choice) keeps a touch of natural haze at infinity.
const DEHAZE_OMEGA: f32 = 0.95;
/// Floor on the estimated transmission: guards the recovered-radiance
/// division from blowing up (and amplifying noise) in dense-haze regions.
const DEHAZE_MIN_TRANSMISSION: f32 = 0.1;

/// Dark-channel-prior haze removal (ADR 0033): `amount > 0` removes
/// atmospheric haze, `amount < 0` re-adds it. Every step — the dark
/// channel's local-minimum filter, the atmospheric-light selection, the
/// transmission estimate — is closed-form and deterministic: no iterative
/// solver, nothing whose result depends on an initial guess or a
/// convergence tolerance (`docs/pipeline.md` §5).
fn dehaze(px: &mut Pixels, amount: i32) {
    let width = px.width as usize;
    let height = px.height as usize;
    let k = f32::from(amount as i16) / 100.0;

    let per_pixel_min: Vec<f32> = px
        .data
        .chunks_exact(3)
        .map(|rgb| rgb[0].min(rgb[1]).min(rgb[2]))
        .collect();
    let dark = min_filter(&per_pixel_min, width, height, DEHAZE_PATCH_RADIUS);
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
    let normalized_dark = min_filter(&normalized_min, width, height, DEHAZE_PATCH_RADIUS);

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
/// factoring [`gaussian_blur`] uses, valid for min/max exactly as it is for
/// weighted sums.
fn min_filter(plane: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
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
fn atmospheric_light(px: &Pixels, dark: &[f32]) -> [f32; 3] {
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

// ---------------------------------------------------------------------------
// HSL mixer and color grading (ADR 0031)
//
// ADR 0031 freezes the *model* for both operators; the exact numeric
// constants below (band centers, shift magnitudes, zone transition shape)
// are the implementation detail the ADR explicitly defers to this module,
// the same level of freedom ADR 0030 left the tone curve's exact spline
// shape.
// ---------------------------------------------------------------------------

/// Fixed hue-band centers in degrees, in the module's declared band order
/// (red, orange, yellow, green, aqua, blue, purple, magenta) — chosen to
/// match each band's everyday position on the hue wheel, not evenly spaced.
const HSL_BAND_CENTERS_DEG: [f32; 8] = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 275.0, 315.0];

/// Maximum hue shift, in degrees, a slider at its ±100 extreme applies.
const MAX_HUE_SHIFT_DEG: f32 = 30.0;
/// Maximum luminance shift a slider at its ±100 extreme applies, as a
/// fraction of the full [0, 1] lightness range.
const MAX_LUM_SHIFT: f32 = 0.25;
/// How strongly a color grading zone's fully-saturated color tints a pixel
/// fully weighted into that zone.
const GRADING_TINT_STRENGTH: f32 = 0.6;

/// Applies the 8-band HSL mixer: every pixel's hue picks up a weighted
/// blend of its two nearest bands' hue/saturation/luminance offsets. The
/// two weights always sum to 1 ([`hue_band_neighbors`]/[`smoothstep01`]), so
/// a pixel exactly on a band's center is fully that band and a pixel
/// between two centers blends smoothly, never double-counting or losing
/// coverage. A neutral `bands` (checked by the caller) is skipped entirely,
/// so this is only ever called when at least one slider is non-zero.
fn hsl_mixer(px: &mut Pixels, bands: &[HslBand; 8]) {
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let (h, s, l) = rgb_to_hsl(rgb);
            let (i0, i1, t) = hue_band_neighbors(h);
            let w1 = smoothstep01(t);
            let w0 = 1.0 - w1;
            let hue_shift = (f32::from(bands[i0].hue as i16) * w0
                + f32::from(bands[i1].hue as i16) * w1)
                / 100.0
                * MAX_HUE_SHIFT_DEG;
            let sat_shift = (f32::from(bands[i0].saturation as i16) * w0
                + f32::from(bands[i1].saturation as i16) * w1)
                / 100.0;
            let lum_shift = (f32::from(bands[i0].luminance as i16) * w0
                + f32::from(bands[i1].luminance as i16) * w1)
                / 100.0;
            let new_h = (h + hue_shift).rem_euclid(360.0);
            let new_s = (s * (1.0 + sat_shift)).clamp(0.0, 1.0);
            let new_l = (l + lum_shift * MAX_LUM_SHIFT).clamp(0.0, 1.0);
            rgb.copy_from_slice(&hsl_to_rgb(new_h, new_s, new_l));
        }
    });
}

/// Finds the two hue bands adjacent to `hue_deg` and how far between them it
/// falls: `(i0, i1, t)`, indices into [`HSL_BAND_CENTERS_DEG`] and `t` in
/// `[0, 1]` from `i0`'s center (`t = 0`) to `i1`'s (`t = 1`), the short way
/// around the hue circle. The bands' spans wrap exactly once around the
/// full circle, so exactly one pair always matches.
fn hue_band_neighbors(hue_deg: f32) -> (usize, usize, f32) {
    let h = hue_deg.rem_euclid(360.0);
    let bands = HSL_BAND_CENTERS_DEG.len();
    for (i, &start) in HSL_BAND_CENTERS_DEG.iter().enumerate() {
        let j = (i + 1) % bands;
        let end = if j == 0 {
            HSL_BAND_CENTERS_DEG[j] + 360.0
        } else {
            HSL_BAND_CENTERS_DEG[j]
        };
        let probe = if h < start { h + 360.0 } else { h };
        if probe >= start && probe <= end {
            return (i, j, (probe - start) / (end - start));
        }
    }
    unreachable!("HSL_BAND_CENTERS_DEG spans exactly one full turn of the hue circle")
}

/// Smoothstep ease; `x` must already be in [0, 1].
fn smoothstep01(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

/// Converts a working-buffer RGB sample (gamma-encoded sRGB, [0, 1]) to
/// standard HSL: hue in degrees `[0, 360)`, saturation and lightness in
/// `[0, 1]`. Standard HSL, not the Rec. 709 luma [`luma`] uses elsewhere —
/// ADR 0031 is explicit that the mixer works in RGB-derived HSL.
fn rgb_to_hsl(rgb: &[f32]) -> (f32, f32, f32) {
    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let delta = max - min;
    if delta < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = if l <= 0.5 {
        delta / (max + min)
    } else {
        delta / (2.0 - max - min)
    };
    let h = if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    (h.rem_euclid(360.0), s, l)
}

/// Inverse of [`rgb_to_hsl`].
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s <= 1e-6 {
        return [l, l, l];
    }
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    [
        (r1 + m).clamp(0.0, 1.0),
        (g1 + m).clamp(0.0, 1.0),
        (b1 + m).clamp(0.0, 1.0),
    ]
}

/// Applies shadows/midtones/highlights color grading: every pixel's own
/// Rec. 709 luma decides its membership weight in each of the three zones
/// ([`zone_weights`], a partition of unity), each zone's color tints the
/// pixel proportional to that weight and the zone's own saturation
/// ([`zone_tint`]), and each zone's luminance offset blends the same way.
/// A pixel's hue/lightness relationship to the mixer above is coincidental
/// — this operates on luma, the mixer above on HSL lightness, per ADR 0031.
fn color_grading(px: &mut Pixels, grading: &ColorGrading) {
    let tints = [
        zone_tint(&grading.shadows),
        zone_tint(&grading.midtones),
        zone_tint(&grading.highlights),
    ];
    let lum_shifts = [
        f32::from(grading.shadows.luminance as i16) / 100.0,
        f32::from(grading.midtones.luminance as i16) / 100.0,
        f32::from(grading.highlights.luminance as i16) / 100.0,
    ];
    let balance = f32::from(grading.balance as i16) / 100.0;
    let blending = f32::from(grading.blending as i16) / 100.0;

    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let weights = zone_weights(luma(rgb), balance, blending);
            let mut tint = [0.0f32; 3];
            let mut lum_shift = 0.0f32;
            for zone in 0..3 {
                for c in 0..3 {
                    tint[c] += tints[zone][c] * weights[zone];
                }
                lum_shift += lum_shifts[zone] * weights[zone];
            }
            let delta_l = lum_shift * MAX_LUM_SHIFT;
            for c in 0..3 {
                rgb[c] = (rgb[c] + delta_l + tint[c] * GRADING_TINT_STRENGTH).clamp(0.0, 1.0);
            }
        }
    });
}

/// The color a zone tints toward: a delta from neutral gray, scaled by the
/// zone's saturation — `[0, 0, 0]` at `saturation = 0`, so a zone with no
/// saturation set contributes nothing regardless of how much weight a pixel
/// gives it (required for the neutral-settings-skip-the-operator rule).
fn zone_tint(zone: &ColorGradingZone) -> [f32; 3] {
    let sat = f32::from(zone.saturation as i16) / 100.0;
    if sat <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    let pure = hsl_to_rgb(f32::from(zone.hue as i16), 1.0, 0.5);
    [
        (pure[0] - 0.5) * sat,
        (pure[1] - 0.5) * sat,
        (pure[2] - 0.5) * sat,
    ]
}

/// Shadow/midtone/highlight membership weights for a pixel of luma `l`,
/// always summing to 1. Two smoothstep transitions (shadow→mid, mid→
/// highlight) separated by a fixed gap so they never overlap; `balance`
/// (already scaled to [-1, 1]) shifts where the gap sits, `blending`
/// (already scaled to [0, 1]) widens both transitions.
fn zone_weights(l: f32, balance: f32, blending: f32) -> [f32; 3] {
    const GAP: f32 = 0.3;
    let center = (0.5 - balance * 0.25).clamp(0.05, 0.95);
    let half_width = 0.02 + blending * 0.13;
    let b1 = center - GAP / 2.0;
    let b2 = center + GAP / 2.0;
    let shadow = 1.0 - smoothstep01(((l - (b1 - half_width)) / (2.0 * half_width)).clamp(0.0, 1.0));
    let highlight = smoothstep01(((l - (b2 - half_width)) / (2.0 * half_width)).clamp(0.0, 1.0));
    let midtone = (1.0 - shadow - highlight).max(0.0);
    let sum = shadow + midtone + highlight;
    if sum <= 1e-6 {
        [0.0, 1.0, 0.0]
    } else {
        [shadow / sum, midtone / sum, highlight / sum]
    }
}

// ---------------------------------------------------------------------------
// Local (masked) adjustments (gamma domain — ADR 0029)
// ---------------------------------------------------------------------------

/// Applies every local adjustment, in list order: later entries composite
/// on top of the buffer earlier ones already wrote, exactly like spot
/// removal's list-order rule above.
fn local_adjustments(px: &mut Pixels, adjustments: &[LocalAdjustment], rotation_degrees: f64) {
    for adjustment in adjustments {
        apply_local_adjustment(px, adjustment, rotation_degrees);
    }
}

/// Re-adjusts a full copy of `px` with this module's own operator functions
/// — re-parameterized by `adjustment.adjustments`, falling back to `px`'s
/// *global* setting for whichever of the restricted fields the entry
/// doesn't set (so a mask that only sets `contrast` still sees the photo's
/// existing global exposure/white-balance as its starting point, not the
/// as-shot neutral) — then blends that copy back into `px` by the mask's
/// rasterized coverage times `opacity` (`mask::blend_by_coverage`).
///
/// Takes the *global* values it falls back to from `px`'s own state at
/// call time in the pipeline: since this stage runs once, after every
/// global tonal/color operator above has already applied, `px` already
/// reflects the global settings — this function only needs `adjustment`'s
/// overrides, not a second copy of the global `Settings`.
fn apply_local_adjustment(px: &mut Pixels, adjustment: &LocalAdjustment, rotation_degrees: f64) {
    let coverage =
        mask::rasterize_coverage(&adjustment.mask, px.width, px.height, rotation_degrees);
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
    mask::blend_by_coverage(px, &adjusted, &coverage, adjustment.opacity);
}

// ---------------------------------------------------------------------------
// Noise reduction and sharpening (gamma domain, luma/chroma split)
// ---------------------------------------------------------------------------

/// Blends the luma plane toward its Gaussian blur; chroma is untouched.
fn luminance_noise_reduction(px: &mut Pixels, strength: i32) {
    let k = f32::from(strength as i16) / 100.0;
    let plane = luma_plane(px);
    let blurred = gaussian_blur(&plane, px.width as usize, px.height as usize, k * 2.0);
    add_luma_delta(px, |i| k * (blurred[i] - plane[i]));
}

/// Blends the chroma planes (per-channel deviation from luma) toward their
/// Gaussian blur.
fn color_noise_reduction(px: &mut Pixels, strength: i32) {
    let k = f32::from(strength as i16) / 100.0;
    let (w, h) = (px.width as usize, px.height as usize);
    let plane = luma_plane(px);
    for channel in 0..3 {
        let chroma: Vec<f32> = (0..w * h)
            .map(|i| px.data[i * 3 + channel] - plane[i])
            .collect();
        let blurred = gaussian_blur(&chroma, w, h, k * 3.0);
        px.data
            .par_chunks_mut(w * 3)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..w {
                    let i = y * w + x;
                    let smoothed = chroma[i] + k * (blurred[i] - chroma[i]);
                    row[x * 3 + channel] = (plane[i] + smoothed).clamp(0.0, 1.0);
                }
            });
    }
}

/// Unsharp mask on the luma plane only, so sharpening never fringes colors.
fn sharpen(px: &mut Pixels, amount: i32, radius: f64) {
    let k = f32::from(amount as i16) / 100.0;
    let plane = luma_plane(px);
    let blurred = gaussian_blur(&plane, px.width as usize, px.height as usize, radius as f32);
    add_luma_delta(px, |i| k * (plane[i] - blurred[i]));
}

/// Extracts the luma plane.
fn luma_plane(px: &Pixels) -> Vec<f32> {
    let width = px.width as usize;
    let mut out = vec![0.0f32; width * px.height as usize];
    out.par_chunks_mut(width)
        .zip(px.data.par_chunks(width * 3))
        .for_each(|(dst, src)| {
            for (value, rgb) in dst.iter_mut().zip(src.chunks_exact(3)) {
                *value = luma(rgb);
            }
        });
    out
}

/// Adds a per-pixel delta to all three channels (a pure luma shift).
fn add_luma_delta(px: &mut Pixels, delta: impl Fn(usize) -> f32 + Sync) {
    let width = px.width as usize;
    px.data
        .par_chunks_mut(width * 3)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, rgb) in row.chunks_exact_mut(3).enumerate() {
                let d = delta(y * width + x);
                for sample in rgb {
                    *sample = (*sample + d).clamp(0.0, 1.0);
                }
            }
        });
}

/// Separable Gaussian blur of a single plane. Kernel radius is `⌈3σ⌉`,
/// edges are clamped (replicated). σ ≤ 0 returns the plane unchanged.
fn gaussian_blur(plane: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    if sigma <= 0.0 {
        return plane.to_vec();
    }
    let radius = (3.0 * sigma).ceil() as usize;
    let mut kernel: Vec<f32> = (0..=radius)
        .map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp())
        .collect();
    let sum: f32 = kernel[0] + 2.0 * kernel[1..].iter().sum::<f32>();
    for weight in &mut kernel {
        *weight /= sum;
    }

    let convolve = |src: &[f32], i: usize, length: usize, stride: usize, base: usize| -> f32 {
        let mut acc = kernel[0] * src[base + i * stride];
        for (k, &weight) in kernel.iter().enumerate().skip(1) {
            let lo = i.saturating_sub(k);
            let hi = (i + k).min(length - 1);
            acc += weight * (src[base + lo * stride] + src[base + hi * stride]);
        }
        acc
    };

    // Horizontal pass, then vertical; both write full output rows, so the
    // rows parallelize without overlapping.
    let mut horizontal = vec![0.0f32; plane.len()];
    horizontal
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, value) in row.iter_mut().enumerate() {
                *value = convolve(plane, x, width, 1, y * width);
            }
        });
    let mut out = vec![0.0f32; plane.len()];
    out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        for (x, value) in row.iter_mut().enumerate() {
            *value = convolve(&horizontal, y, height, width, x);
        }
    });
    out
}

// ---------------------------------------------------------------------------
// Spot removal (gamma domain, clone only — ADR 0032)
// ---------------------------------------------------------------------------

/// Applies every clone patch, in list order: later entries see the pixels
/// earlier entries already wrote, since each reads from a fresh snapshot of
/// `px` taken at the start of its own turn.
fn spot_removal(px: &mut Pixels, spots: &[SpotRemoval], rotation_degrees: f64) {
    for spot in spots {
        apply_spot(px, spot, rotation_degrees);
    }
}

/// Clones the disk around `spot.source` onto the disk around `spot.target`.
/// Both points are stored normalized in the post-rotation, pre-crop
/// referential of ADR 0026; `rotation_degrees` maps them back to this
/// still-unrotated buffer via [`post_rotation_point_to_buffer`]. The radius
/// is normalized against the buffer's larger dimension so the disk stays
/// circular in physical pixels regardless of aspect ratio (an implementation
/// constant, not fixed by the ADR).
fn apply_spot(px: &mut Pixels, spot: &SpotRemoval, rotation_degrees: f64) {
    let (width, height) = (px.width, px.height);
    let (tx, ty) = post_rotation_point_to_buffer(width, height, rotation_degrees, spot.target);
    let (sx, sy) = post_rotation_point_to_buffer(width, height, rotation_degrees, spot.source);
    let radius_px = spot.radius * f64::from(width.max(height));
    if radius_px <= 0.0 {
        return;
    }
    let (dx, dy) = (sx - tx, sy - ty);
    let feather = spot.feather.clamp(0.0, 1.0);
    let opacity = spot.opacity.clamp(0.0, 1.0) as f32;

    let x0 = (tx - radius_px).floor().max(0.0) as usize;
    let y0 = (ty - radius_px).floor().max(0.0) as usize;
    let x1 = ((tx + radius_px).ceil() as i64).clamp(0, i64::from(width) - 1) as usize;
    let y1 = ((ty + radius_px).ceil() as i64).clamp(0, i64::from(height) - 1) as usize;
    if x0 >= width as usize || y0 >= height as usize || x1 < x0 || y1 < y0 {
        return;
    }

    // A snapshot taken once per spot: every pixel this turn writes reads
    // from the state before this spot, so the pass is well-defined even when
    // source and target disks overlap.
    let before = px.clone();
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
            let dist = ((cx - tx).powi(2) + (cy - ty).powi(2)).sqrt();
            if dist > radius_px {
                continue;
            }
            let coverage = radial_coverage(dist / radius_px, feather) * opacity;
            if coverage <= 0.0 {
                continue;
            }
            let Some(sample) = bilinear(&before, cx + dx, cy + dy) else {
                continue;
            };
            let index = (y * width as usize + x) * 3;
            for (c, source) in sample.into_iter().enumerate() {
                let background = px.data[index + c];
                px.data[index + c] = background + (source - background) * coverage;
            }
        }
    }
}

/// Radial falloff at the disk's edge: full coverage out to `1 - feather` of
/// the normalized radius `t` (0 at the center, 1 at the rim), smoothstep-
/// eased to 0 from there to the rim. `feather = 0` is a hard edge;
/// `feather = 1` eases across the whole disk.
fn radial_coverage(t: f64, feather: f64) -> f32 {
    if t >= 1.0 {
        return 0.0;
    }
    let inner = 1.0 - feather;
    if t <= inner {
        return 1.0;
    }
    let span = (1.0 - inner).max(1e-9);
    let f = ((t - inner) / span).clamp(0.0, 1.0) as f32;
    1.0 - f * f * (3.0 - 2.0 * f)
}

/// Maps a point normalized in the post-rotation, pre-crop referential (ADR
/// 0026) back to this still-unrotated buffer's pixel coordinates: the exact
/// inverse of the per-pixel source lookup [`rotate`] performs later in the
/// pipeline, evaluated once for a single point instead of every output pixel.
pub(crate) fn post_rotation_point_to_buffer(
    width: u32,
    height: u32,
    degrees: f64,
    point: Point,
) -> (f64, f64) {
    let radians = degrees.rem_euclid(360.0).to_radians();
    let (sin, cos) = radians.sin_cos();
    let (w, h) = (f64::from(width), f64::from(height));
    let out_w = (w * cos.abs() + h * sin.abs()).round().max(1.0);
    let out_h = (w * sin.abs() + h * cos.abs()).round().max(1.0);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (ocx, ocy) = (out_w / 2.0, out_h / 2.0);

    let dx = point.x * out_w - ocx;
    let dy = point.y * out_h - ocy;
    let sx = cos * dx + sin * dy + cx;
    let sy = -sin * dx + cos * dy + cy;
    (sx, sy)
}

// ---------------------------------------------------------------------------
// Geometry (rotation, crop — unchanged from process 2)
// ---------------------------------------------------------------------------

/// Rotates clockwise by an arbitrary angle. The output canvas is the axis-
/// aligned bounding box of the rotated frame; samples falling outside the
/// source are black. Sampling is bilinear, geometry is computed in `f64`.
fn rotate(px: &Pixels, degrees: f64) -> Pixels {
    let radians = degrees.rem_euclid(360.0).to_radians();
    let (sin, cos) = radians.sin_cos();
    let (w, h) = (f64::from(px.width), f64::from(px.height));
    let out_w = (w * cos.abs() + h * sin.abs()).round().max(1.0) as u32;
    let out_h = (w * sin.abs() + h * cos.abs()).round().max(1.0) as u32;

    let mut data = vec![0.0f32; out_w as usize * out_h as usize * 3];
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (ocx, ocy) = (f64::from(out_w) / 2.0, f64::from(out_h) / 2.0);

    data.par_chunks_mut(out_w as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, rgb_out) in row.chunks_exact_mut(3).enumerate() {
                // Screen coordinates grow downward, so the clockwise
                // rotation matrix is [cos −sin; sin cos]; this is its
                // inverse.
                let dx = (x as f64 + 0.5) - ocx;
                let dy = (y as f64 + 0.5) - ocy;
                let sx = cos * dx + sin * dy + cx;
                let sy = -sin * dx + cos * dy + cy;
                if let Some(rgb) = bilinear(px, sx, sy) {
                    rgb_out.copy_from_slice(&rgb);
                }
            }
        });
    Pixels {
        width: out_w,
        height: out_h,
        data,
    }
}

/// Bilinear sample at continuous coordinates (pixel centers at n + 0.5);
/// `None` when the point lies outside the source frame. This module's own
/// convention for `rotate`/`crop` — distinct from [`lens_bilinear`], which
/// follows Lensfun's integer-centered convention instead.
fn bilinear(px: &Pixels, sx: f64, sy: f64) -> Option<[f32; 3]> {
    let (w, h) = (f64::from(px.width), f64::from(px.height));
    if sx < 0.0 || sy < 0.0 || sx >= w || sy >= h {
        return None;
    }
    let fx = (sx - 0.5).clamp(0.0, w - 1.0);
    let fy = (sy - 0.5).clamp(0.0, h - 1.0);
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let x1 = (x0 + 1).min(px.width as usize - 1);
    let y1 = (y0 + 1).min(px.height as usize - 1);
    let (tx, ty) = ((fx - fx.floor()) as f32, (fy - fy.floor()) as f32);

    let at = |x: usize, y: usize, c: usize| px.data[(y * px.width as usize + x) * 3 + c];
    let mut rgb = [0.0f32; 3];
    for (c, value) in rgb.iter_mut().enumerate() {
        let top = at(x0, y0, c) * (1.0 - tx) + at(x1, y0, c) * tx;
        let bottom = at(x0, y1, c) * (1.0 - tx) + at(x1, y1, c) * tx;
        *value = top * (1.0 - ty) + bottom * ty;
    }
    Some(rgb)
}

/// Extracts the crop rectangle, normalized coordinates rounded to whole
/// pixels, clamped to the frame, at least one pixel each way.
fn crop(px: &Pixels, rect: &Crop) -> Pixels {
    let (w, h) = (f64::from(px.width), f64::from(px.height));
    let x0 = ((rect.x * w).round() as u32).min(px.width - 1);
    let y0 = ((rect.y * h).round() as u32).min(px.height - 1);
    let out_w = ((rect.width * w).round() as u32).clamp(1, px.width - x0);
    let out_h = ((rect.height * h).round() as u32).clamp(1, px.height - y0);

    let mut data = Vec::with_capacity(out_w as usize * out_h as usize * 3);
    for y in y0..y0 + out_h {
        let start = ((y * px.width + x0) * 3) as usize;
        data.extend_from_slice(&px.data[start..start + out_w as usize * 3]);
    }
    Pixels {
        width: out_w,
        height: out_h,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leyline_core::LensCorrection;

    #[test]
    fn lookup_tables_track_the_exact_transfer_functions() {
        let (to_linear, to_srgb) = tables();
        for i in 0..=100_000 {
            let v = i as f32 / 100_000.0;
            assert!(
                (lookup(to_linear, v) - exact_srgb_to_linear(v)).abs() < 2e-5,
                "srgb_to_linear at {v}"
            );
            assert!(
                (lookup(to_srgb, v) - exact_linear_to_srgb(v)).abs() < 2e-5,
                "linear_to_srgb at {v}"
            );
        }
    }

    /// A deterministic gradient-plus-block test card, large enough for the
    /// bundled Canon profile's distortion to move samples by more than a
    /// rounding error.
    fn test_image(width: u32, height: u32) -> RawImage {
        let mut data = Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            for x in 0..width {
                data.push((x * 255 / width) as u8);
                data.push((y * 255 / height) as u8);
                data.push((((x + y) * 255) / (width + height)) as u8);
            }
        }
        RawImage {
            width,
            height,
            bits: 8,
            data,
        }
    }

    fn canon_shot(focal_mm: f32) -> LensShot {
        LensShot {
            camera_make: "Canon".to_owned(),
            camera_model: "Canon EOS 5D Mark III".to_owned(),
            lens_make: Some("Canon".to_owned()),
            lens_model: Some("Canon EF 16-35mm f/2.8L II USM".to_owned()),
            focal_mm,
            aperture_f: Some(2.8),
        }
    }

    fn enabled_settings() -> Settings {
        Settings {
            process: 10,
            lens_correction: LensCorrection {
                enabled: true,
                profile: "auto".to_owned(),
            },
            ..Settings::default()
        }
    }

    #[test]
    fn disabled_lens_correction_matches_process_7_bit_for_bit() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 10,
            exposure: 0.3,
            contrast: 20,
            ..Settings::default()
        };
        let with_process_8 = develop(&image, &settings, Some(&canon_shot(20.0))).unwrap();
        let with_process_7 =
            crate::process7::develop(&image, &settings, Some(&canon_shot(20.0))).unwrap();
        assert_eq!(with_process_8, with_process_7);
    }

    #[test]
    fn no_shot_leaves_the_image_unchanged_even_when_enabled() {
        let image = test_image(64, 48);
        let out = develop(&image, &enabled_settings(), None).unwrap();
        assert_eq!(out.data, image.data);
    }

    #[test]
    fn unmatched_gear_leaves_the_image_unchanged() {
        let image = test_image(64, 48);
        let shot = LensShot {
            camera_make: "Nobody".to_owned(),
            camera_model: "Nothing".to_owned(),
            lens_make: Some("Nobody".to_owned()),
            lens_model: Some("Nothing".to_owned()),
            focal_mm: 20.0,
            aperture_f: Some(2.8),
        };
        let out = develop(&image, &enabled_settings(), Some(&shot)).unwrap();
        assert_eq!(out.data, image.data);
    }

    #[test]
    fn a_matched_profile_undistorts_the_image() {
        let image = test_image(640, 480);
        let out = develop(&image, &enabled_settings(), Some(&canon_shot(20.0))).unwrap();
        assert_eq!((out.width, out.height), (image.width, image.height));
        assert_ne!(
            out.data, image.data,
            "distortion correction should move pixels"
        );
    }

    #[test]
    fn disabled_setting_ignores_a_matched_profile() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 10,
            ..Settings::default()
        };
        let out = develop(&image, &settings, Some(&canon_shot(20.0))).unwrap();
        assert_eq!(out.data, image.data);
    }

    // -------------------------------------------------------------------
    // Tone curve (ADR 0030) — inherited from process 6, still exercised here
    // -------------------------------------------------------------------

    #[test]
    fn no_points_matches_process_7_bit_for_bit() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 10,
            exposure: 0.2,
            contrast: 15,
            ..Settings::default()
        };
        let out8 = develop(&image, &settings, None).unwrap();
        let out7 = crate::process7::develop(&image, &settings, None).unwrap();
        assert_eq!(out8, out7);
    }

    #[test]
    fn identity_curve_matches_no_points_bit_for_bit() {
        let image = test_image(64, 48);
        let with_points = Settings {
            process: 10,
            tone_curve: leyline_core::ToneCurve {
                points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }],
            },
            ..Settings::default()
        };
        let without_points = Settings {
            process: 10,
            ..Settings::default()
        };
        let out_with = develop(&image, &with_points, None).unwrap();
        let out_without = develop(&image, &without_points, None).unwrap();
        assert_eq!(out_with, out_without);
    }

    #[test]
    fn s_curve_raises_shadows_and_lowers_highlights() {
        // The ADR 0030 example: a soft S-curve.
        let settings = Settings {
            process: 10,
            tone_curve: leyline_core::ToneCurve {
                points: vec![
                    CurvePoint { x: 0.0, y: 0.0 },
                    CurvePoint { x: 0.25, y: 0.30 },
                    CurvePoint { x: 0.75, y: 0.70 },
                    CurvePoint { x: 1.0, y: 1.0 },
                ],
            },
            ..Settings::default()
        };
        settings.validate().unwrap();
        let table = build_curve_lut(&settings.tone_curve.points);
        assert!(curve_lookup(&table, 0.1) > 0.1, "shadows should lift");
        assert!(curve_lookup(&table, 0.9) < 0.9, "highlights should drop");
        assert!((curve_lookup(&table, 0.0) - 0.0).abs() < 1e-4);
        assert!((curve_lookup(&table, 1.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn curve_lut_is_monotone_even_with_unevenly_spaced_points() {
        let points = vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.05, y: 0.4 },
            CurvePoint { x: 0.5, y: 0.5 },
            CurvePoint { x: 1.0, y: 1.0 },
        ];
        let table = build_curve_lut(&points);
        for pair in table.windows(2) {
            assert!(
                pair[1] >= pair[0] - 1e-6,
                "curve LUT must never decrease: {pair:?}"
            );
        }
    }

    #[test]
    fn curve_passes_through_its_control_points() {
        let points = vec![
            CurvePoint { x: 0.0, y: 0.1 },
            CurvePoint { x: 0.4, y: 0.6 },
            CurvePoint { x: 1.0, y: 0.9 },
        ];
        let table = build_curve_lut(&points);
        for point in &points {
            let looked_up = curve_lookup(&table, point.x as f32);
            assert!(
                (looked_up - point.y as f32).abs() < 1e-3,
                "expected {} at x={}, got {looked_up}",
                point.y,
                point.x
            );
        }
    }

    // -------------------------------------------------------------------
    // Spot removal (ADR 0032)
    // -------------------------------------------------------------------

    /// A test card with a distinct solid-color marker block near one corner
    /// (the clone source) and a gradient everywhere else (the background a
    /// clone should overwrite at the target).
    fn spot_test_image(width: u32, height: u32) -> RawImage {
        let mut data = Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            for x in 0..width {
                if (2..6).contains(&x) && (2..6).contains(&y) {
                    data.extend_from_slice(&[200, 40, 40]);
                } else {
                    let g = ((x * 7 + y * 11) % 255) as u8;
                    data.extend_from_slice(&[g, g, g]);
                }
            }
        }
        RawImage {
            width,
            height,
            bits: 8,
            data,
        }
    }

    #[test]
    fn no_spots_matches_process_7_bit_for_bit() {
        let image = spot_test_image(32, 24);
        let settings = Settings {
            process: 10,
            exposure: 0.1,
            ..Settings::default()
        };
        let out8 = develop(&image, &settings, None).unwrap();
        let out7 = crate::process7::develop(&image, &settings, None).unwrap();
        assert_eq!(out8, out7);
    }

    #[test]
    fn a_full_opacity_hard_edged_clone_copies_the_source_disk_onto_the_target() {
        let width = 32u32;
        let height = 24u32;
        let image = spot_test_image(width, height);
        // Source disk centered on the marker block at (4, 4); target far away.
        let settings = Settings {
            process: 10,
            spot_removal: vec![SpotRemoval {
                target: Point {
                    x: 20.0 / width as f64,
                    y: 16.0 / height as f64,
                },
                source: Point {
                    x: 4.0 / width as f64,
                    y: 4.0 / height as f64,
                },
                radius: 1.0 / width as f64, // ~1px: stays well inside the marker/background
                feather: 0.0,
                opacity: 1.0,
            }],
            ..Settings::default()
        };
        let out = develop(&image, &settings, None).unwrap();
        let idx = (16 * width as usize + 20) * 3;
        assert_eq!(
            &out.data[idx..idx + 3],
            &[200, 40, 40],
            "the target pixel should now match the marker it cloned"
        );
        // Far outside the target disk, the background is untouched.
        let untouched_idx = (2 * width as usize + 2) * 3;
        assert_eq!(
            &out.data[untouched_idx..untouched_idx + 3],
            &image.data[untouched_idx..untouched_idx + 3]
        );
    }

    #[test]
    fn zero_opacity_leaves_the_image_unchanged() {
        let width = 32u32;
        let height = 24u32;
        let image = spot_test_image(width, height);
        let settings = Settings {
            process: 10,
            spot_removal: vec![SpotRemoval {
                target: Point { x: 0.6, y: 0.6 },
                source: Point { x: 0.1, y: 0.1 },
                radius: 0.1,
                feather: 0.5,
                opacity: 0.0,
            }],
            ..Settings::default()
        };
        let out = develop(&image, &settings, None).unwrap();
        assert_eq!(out.data, image.data);
    }

    #[test]
    fn post_rotation_point_to_buffer_is_identity_at_zero_rotation() {
        let point = Point { x: 0.3, y: 0.7 };
        let (sx, sy) = post_rotation_point_to_buffer(100, 50, 0.0, point);
        assert!((sx - 30.0).abs() < 1e-9);
        assert!((sy - 35.0).abs() < 1e-9);
    }

    #[test]
    fn post_rotation_point_to_buffer_flips_both_axes_at_180_degrees() {
        let point = Point { x: 0.2, y: 0.9 };
        let (sx, sy) = post_rotation_point_to_buffer(100, 50, 180.0, point);
        assert!((sx - 80.0).abs() < 1e-6, "sx = {sx}");
        assert!((sy - 5.0).abs() < 1e-6, "sy = {sy}");
    }

    #[test]
    fn post_rotation_point_to_buffer_swaps_axes_at_90_degrees() {
        // Rotating 90 degrees clockwise swaps the canvas dimensions: the
        // post-rotation canvas is height x width relative to the buffer.
        let point = Point { x: 0.25, y: 0.5 };
        let (sx, sy) = post_rotation_point_to_buffer(100, 50, 90.0, point);
        // out_w = 50, out_h = 100 for a 100x50 buffer rotated 90 degrees.
        assert!((0.0..=100.0).contains(&sx));
        assert!((0.0..=50.0).contains(&sy));
    }

    #[test]
    fn radial_coverage_is_full_inside_the_hard_core_and_zero_past_the_rim() {
        assert_eq!(radial_coverage(0.0, 0.5), 1.0);
        assert_eq!(radial_coverage(1.0, 0.5), 0.0);
        assert_eq!(radial_coverage(1.5, 0.5), 0.0);
        assert_eq!(radial_coverage(0.0, 0.0), 1.0);
        // Just past the rim with no feather: a hard edge.
        assert_eq!(radial_coverage(0.999, 0.0), 1.0);
    }

    #[test]
    fn radial_coverage_eases_monotonically_across_the_feather_band() {
        let mut previous = radial_coverage(0.5, 1.0);
        for i in 1..=10 {
            let t = 0.5 + 0.05 * i as f64;
            let current = radial_coverage(t, 1.0);
            assert!(
                current <= previous + 1e-6,
                "coverage must not increase toward the rim"
            );
            previous = current;
        }
    }

    /// `lens_bilinear_channel` must return, for every channel, exactly the
    /// value `lens_bilinear` would have computed for that channel — it's an
    /// in-place optimization ([`correct_tca`] discarded two of the three
    /// channels `lens_bilinear` computed), not a formula change. Covers
    /// interior samples, edge/corner clamping, and the out-of-frame `None`
    /// case.
    #[test]
    fn lens_bilinear_channel_matches_the_full_rgb_sampler_bit_for_bit() {
        let width = 12u32;
        let height = 9u32;
        let mut data = Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            for x in 0..width {
                data.push((x * 37 % 255) as f32 / 255.0);
                data.push((y * 53 % 255) as f32 / 255.0);
                data.push(((x + y) * 29 % 255) as f32 / 255.0);
            }
        }
        let px = Pixels {
            width,
            height,
            data,
        };

        let samples = [
            (0.0, 0.0),                                // corner
            (width as f32 - 1.0, 0.0),                 // corner, x clamp
            (0.0, height as f32 - 1.0),                // corner, y clamp
            (5.3, 4.7),                                // interior, fractional
            (2.999_9, 6.000_1),                        // near-integer fractional
            (width as f32 - 1.0, height as f32 - 1.0), // far corner
            (-0.001, 3.0),                             // just out of frame: x
            (3.0, height as f32),                      // just out of frame: y
        ];

        for (sx, sy) in samples {
            let full = lens_bilinear(&px, sx, sy);
            for c in 0..3 {
                let single = lens_bilinear_channel(&px, sx, sy, c);
                match full {
                    Some(rgb) => assert_eq!(
                        single,
                        Some(rgb[c]),
                        "channel {c} at ({sx}, {sy}) diverged from the full-RGB sampler"
                    ),
                    None => assert_eq!(
                        single, None,
                        "channel {c} at ({sx}, {sy}) should also be out of frame"
                    ),
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // Local adjustments (ADR 0029)
    // -------------------------------------------------------------------

    use leyline_core::{LocalAdjustment, LocalAdjustmentValues, Mask};

    #[test]
    fn no_local_adjustments_matches_process_7_bit_for_bit() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 10,
            exposure: 0.2,
            contrast: 15,
            ..Settings::default()
        };
        let out8 = develop(&image, &settings, None).unwrap();
        let out7 = crate::process7::develop(&image, &settings, None).unwrap();
        assert_eq!(out8, out7);
    }

    #[test]
    fn a_radial_mask_darkens_only_its_covered_area() {
        let image = test_image(200, 150);
        let settings = Settings {
            process: 10,
            local_adjustments: vec![LocalAdjustment {
                mask: Mask::Radial {
                    cx: 0.5,
                    cy: 0.5,
                    rx: 0.15,
                    ry: 0.15,
                    angle: 0.0,
                    feather: 0.0,
                    inverted: false,
                },
                opacity: 1.0,
                adjustments: LocalAdjustmentValues {
                    exposure: Some(-2.0),
                    ..LocalAdjustmentValues::default()
                },
            }],
            ..Settings::default()
        };
        let out = develop(&image, &settings, None).unwrap();
        let plain = develop(
            &image,
            &Settings {
                process: 10,
                ..Settings::default()
            },
            None,
        )
        .unwrap();

        let center_idx = ((150 / 2 * 200 + 100) * 3) as usize;
        assert!(
            out.data[center_idx] < plain.data[center_idx],
            "the masked area should be darkened by the local exposure drop"
        );
        // Far corner, well outside the mask's radius: untouched.
        let corner_idx = 0usize;
        assert_eq!(out.data[corner_idx], plain.data[corner_idx]);
    }

    #[test]
    fn zero_opacity_local_adjustment_leaves_the_image_unchanged() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 10,
            local_adjustments: vec![LocalAdjustment {
                mask: Mask::Radial {
                    cx: 0.5,
                    cy: 0.5,
                    rx: 0.3,
                    ry: 0.3,
                    angle: 0.0,
                    feather: 0.2,
                    inverted: false,
                },
                opacity: 0.0,
                adjustments: LocalAdjustmentValues {
                    exposure: Some(2.0),
                    ..LocalAdjustmentValues::default()
                },
            }],
            ..Settings::default()
        };
        let out = develop(&image, &settings, None).unwrap();
        let plain = develop(
            &image,
            &Settings {
                process: 10,
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(out.data, plain.data);
    }

    #[test]
    fn an_empty_local_adjustments_list_is_neutral() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 10,
            local_adjustments: vec![],
            exposure: 0.1,
            ..Settings::default()
        };
        let with_empty = develop(&image, &settings, None).unwrap();
        let without_field = develop(
            &image,
            &Settings {
                process: 10,
                exposure: 0.1,
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(with_empty.data, without_field.data);
    }

    #[test]
    fn local_adjustments_apply_in_list_order_on_top_of_each_other() {
        // Two full-coverage exposure-raising entries in sequence: the second
        // entry's blend reads the buffer the first entry already brightened
        // (list-order composition, the same rule spot removal follows
        // above), so stacking both must brighten a non-clipped pixel
        // strictly more than either alone.
        let image = test_image(200, 150);
        let full_frame_radial = |exposure: f64| LocalAdjustment {
            mask: Mask::Radial {
                cx: 0.5,
                cy: 0.5,
                rx: 1.0,
                ry: 1.0,
                angle: 0.0,
                feather: 0.0,
                inverted: false,
            },
            opacity: 1.0,
            adjustments: LocalAdjustmentValues {
                exposure: Some(exposure),
                ..LocalAdjustmentValues::default()
            },
        };
        let stacked = Settings {
            process: 10,
            local_adjustments: vec![full_frame_radial(0.5), full_frame_radial(0.5)],
            ..Settings::default()
        };
        let single = Settings {
            process: 10,
            local_adjustments: vec![full_frame_radial(0.5)],
            ..Settings::default()
        };
        let out_stacked = develop(&image, &stacked, None).unwrap();
        let out_single = develop(&image, &single, None).unwrap();
        // A middling, non-clipped pixel near the frame's center.
        let idx = ((150 / 2 * 200 + 100) * 3) as usize;
        assert!(
            out_stacked.data[idx] > out_single.data[idx],
            "stacking two exposure-raising entries should brighten more than one alone"
        );
    }

    // -----------------------------------------------------------------
    // HSL mixer and color grading (ADR 0031)
    // -----------------------------------------------------------------

    #[test]
    fn neutral_hsl_and_color_grading_match_process_8_bit_for_bit() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 10,
            exposure: 0.3,
            vibrance: 20,
            ..Settings::default()
        };
        let out9 = develop(&image, &settings, None).unwrap();
        let out8 = crate::process8::develop(&image, &settings, None).unwrap();
        assert_eq!(out9, out8);
    }

    #[test]
    fn rgb_hsl_round_trips_within_float_error() {
        let samples: [[f32; 3]; 6] = [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.3, 0.6, 0.2],
            [0.8, 0.8, 0.8],
            [0.0, 0.0, 0.0],
        ];
        for rgb in samples {
            let (h, s, l) = rgb_to_hsl(&rgb);
            let back = hsl_to_rgb(h, s, l);
            for c in 0..3 {
                assert!(
                    (rgb[c] - back[c]).abs() < 1e-4,
                    "channel {c}: {rgb:?} -> hsl({h},{s},{l}) -> {back:?}"
                );
            }
        }
    }

    #[test]
    fn hue_band_neighbors_always_form_a_partition_of_unity() {
        let mut h = 0.0f32;
        while h < 360.0 {
            let (i0, i1, t) = hue_band_neighbors(h);
            assert!(i0 < 8 && i1 < 8);
            assert!((0.0..=1.0).contains(&t), "t out of range at {h}: {t}");
            h += 1.0;
        }
    }

    #[test]
    fn hue_band_neighbors_is_exact_at_band_centers() {
        // A center point sits exactly on the shared boundary of two
        // segments (`[i-1, i]` and `[i, i+1]`); which one the scan matches
        // first is unspecified, but either way the *effective* weight must
        // land 100% on band `i` — never split.
        for (i, &center) in HSL_BAND_CENTERS_DEG.iter().enumerate() {
            let (i0, i1, t) = hue_band_neighbors(center);
            let w1 = smoothstep01(t);
            let w0 = 1.0 - w1;
            let band_i_weight = if i0 == i {
                w0
            } else if i1 == i {
                w1
            } else {
                0.0
            };
            assert!(
                band_i_weight > 0.999,
                "center {center} (band {i}) should resolve to full weight, got \
                 i0={i0} i1={i1} t={t} (band_i_weight={band_i_weight})"
            );
        }
    }

    #[test]
    fn zone_weights_always_sum_to_one() {
        for balance in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
            for blending in [0.0f32, 0.3, 0.6, 1.0] {
                let mut l = 0.0f32;
                while l <= 1.0 {
                    let w = zone_weights(l, balance, blending);
                    let sum = w[0] + w[1] + w[2];
                    assert!(
                        (sum - 1.0).abs() < 1e-4,
                        "weights {w:?} at l={l} balance={balance} blending={blending} sum to {sum}"
                    );
                    assert!(w.iter().all(|&x| (0.0..=1.0).contains(&x)));
                    l += 0.05;
                }
            }
        }
    }

    #[test]
    fn zone_weights_favor_shadows_at_low_luma_and_highlights_at_high_luma() {
        let low = zone_weights(0.0, 0.0, 0.5);
        let high = zone_weights(1.0, 0.0, 0.5);
        assert!(low[0] > 0.9, "shadow weight at l=0: {low:?}");
        assert!(high[2] > 0.9, "highlight weight at l=1: {high:?}");
    }

    #[test]
    fn zone_tint_is_zero_at_zero_saturation() {
        let zone = ColorGradingZone {
            hue: 220,
            saturation: 0,
            luminance: 0,
        };
        assert_eq!(zone_tint(&zone), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn zone_tint_is_non_zero_once_saturation_is_set() {
        let zone = ColorGradingZone {
            hue: 220,
            saturation: 50,
            luminance: 0,
        };
        assert_ne!(zone_tint(&zone), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn hsl_mixer_boosts_saturation_of_a_matched_band_only() {
        // A muted red (band 0, center 0°) and a muted green (band 3, center
        // 120°, far enough from red's neighbors — orange 30°, magenta
        // 315° — to be unaffected by red's slider).
        let mut image = test_image(4, 4);
        for px in image.data.chunks_exact_mut(3) {
            px[0] = 160;
            px[1] = 90;
            px[2] = 90;
        }
        let mut bands = [HslBand::default(); 8];
        bands[0].saturation = 100;
        let settings = Settings {
            process: 10,
            hsl: bands,
            ..Settings::default()
        };
        let before = rgb_to_hsl(&[160.0 / 255.0, 90.0 / 255.0, 90.0 / 255.0]);
        let out = develop(&image, &settings, None).unwrap();
        let after = rgb_to_hsl(&[
            f32::from(out.data[0]) / 255.0,
            f32::from(out.data[1]) / 255.0,
            f32::from(out.data[2]) / 255.0,
        ]);
        assert!(
            after.1 > before.1,
            "saturation should increase: before {before:?}, after {after:?}"
        );
    }

    #[test]
    fn color_grading_tints_shadows_without_touching_highlights() {
        let mut image = test_image(4, 4);
        for (i, px) in image.data.chunks_exact_mut(3).enumerate() {
            let v = if i % 2 == 0 { 10 } else { 245 };
            px[0] = v;
            px[1] = v;
            px[2] = v;
        }
        let settings = Settings {
            process: 10,
            color_grading: ColorGrading {
                shadows: ColorGradingZone {
                    hue: 220,
                    saturation: 80,
                    luminance: 0,
                },
                ..ColorGrading::default()
            },
            ..Settings::default()
        };
        let out = develop(&image, &settings, None).unwrap();
        // The dark (shadow) pixel picks up a blue tint: B channel rises
        // above R/G. The bright (highlight) pixel, far from the shadow
        // zone's weight, stays gray (all channels equal).
        let dark = &out.data[0..3];
        let bright = &out.data[3..6];
        assert!(
            dark[2] > dark[0],
            "shadow pixel should gain a blue tint: {dark:?}"
        );
        assert_eq!(
            bright[0], bright[1],
            "highlight pixel should stay neutral: {bright:?}"
        );
        assert_eq!(
            bright[1], bright[2],
            "highlight pixel should stay neutral: {bright:?}"
        );
    }

    // -----------------------------------------------------------------
    // Clarity, texture and dehaze (ADR 0033)
    // -----------------------------------------------------------------

    #[test]
    fn neutral_clarity_texture_dehaze_match_process_9_bit_for_bit() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 10,
            exposure: 0.2,
            vibrance: 10,
            ..Settings::default()
        };
        let out10 = develop(&image, &settings, None).unwrap();
        let out9 = crate::process9::develop(&image, &settings, None).unwrap();
        assert_eq!(out10, out9);
    }

    #[test]
    fn min_filter_matches_a_naive_2d_minimum() {
        let width = 6;
        let height = 5;
        let plane: Vec<f32> = (0..width * height)
            .map(|i| ((i * 37) % 100) as f32)
            .collect();
        let radius = 2;
        let filtered = min_filter(&plane, width, height, radius);
        for y in 0..height {
            for x in 0..width {
                let mut expected = f32::INFINITY;
                for yy in y.saturating_sub(radius)..=(y + radius).min(height - 1) {
                    for xx in x.saturating_sub(radius)..=(x + radius).min(width - 1) {
                        expected = expected.min(plane[yy * width + xx]);
                    }
                }
                assert_eq!(filtered[y * width + x], expected, "at ({x},{y})");
            }
        }
    }

    #[test]
    fn min_filter_on_a_uniform_plane_is_unchanged() {
        let plane = vec![0.42f32; 20];
        let filtered = min_filter(&plane, 5, 4, 1);
        assert!(filtered.iter().all(|&v| (v - 0.42).abs() < 1e-6));
    }

    #[test]
    fn approx_blur_smooths_a_checkerboard_toward_its_mean() {
        let width = 64;
        let height = 64;
        let plane: Vec<f32> = (0..width * height)
            .map(|i| {
                let x = i % width;
                let y = i / width;
                if (x / 4 + y / 4) % 2 == 0 { 1.0 } else { 0.0 }
            })
            .collect();
        let blurred = approx_blur(&plane, width, height, 20.0);
        let idx = (height / 2) * width + width / 2;
        assert!(
            (blurred[idx] - 0.5).abs() < 0.35,
            "expected the blur to land near the checkerboard's 0.5 mean, got {}",
            blurred[idx]
        );
    }

    #[test]
    fn approx_blur_falls_back_to_the_literal_kernel_at_a_small_sigma() {
        let plane = vec![0.2, 0.8, 0.3, 0.9, 0.1, 0.7, 0.4, 0.6, 0.5];
        let (width, height) = (3, 3);
        assert_eq!(
            approx_blur(&plane, width, height, 0.5),
            gaussian_blur(&plane, width, height, 0.5)
        );
    }

    #[test]
    fn local_contrast_with_zero_amount_is_a_no_op() {
        let image = test_image(16, 12);
        let mut px = Pixels::from_raw(&image).unwrap();
        let before = px.to_rgb8();
        local_contrast(&mut px, 0, CLARITY_RADIUS);
        assert_eq!(px.to_rgb8(), before);
    }

    #[test]
    fn local_contrast_pushes_a_bright_region_brighter() {
        // Left half dark, right half bright: a positive-amount local
        // contrast boost should push pixels away from the low-pass
        // (blurred) version, brightening the already-bright side further.
        let width = 40usize;
        let height = 20usize;
        let mut data = Vec::with_capacity(width * height * 3);
        for _y in 0..height {
            for x in 0..width {
                let v = if x < width / 2 { 60u8 } else { 180u8 };
                data.push(v);
                data.push(v);
                data.push(v);
            }
        }
        let image = RawImage {
            width: width as u32,
            height: height as u32,
            bits: 8,
            data,
        };
        let mut px = Pixels::from_raw(&image).unwrap();
        local_contrast(&mut px, 80, TEXTURE_RADIUS);
        let out = px.to_rgb8();
        let idx = (10 * width + (width / 2 + 2)) * 3;
        assert!(
            out[idx] as i32 > 180,
            "expected the bright side to gain further contrast, got {}",
            out[idx]
        );
    }

    /// A gradient scene veiled toward a bright gray "atmosphere" — exactly
    /// the kind of image dehaze is meant to undo (`amount > 0`) or add to
    /// (`amount < 0`).
    fn hazy_test_image(width: u32, height: u32) -> RawImage {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for _y in 0..height {
            for x in 0..width {
                let scene = (x * 100 / width) as u16;
                let haze = 180u16;
                let v = ((scene + haze) / 2) as u8;
                data.push(v);
                data.push(v);
                data.push(v);
            }
        }
        RawImage {
            width,
            height,
            bits: 8,
            data,
        }
    }

    /// Widest span between the darkest and brightest R sample (R=G=B in
    /// these synthetic gray images, so one channel is representative).
    fn tonal_range(data: &[u8]) -> u8 {
        let min = data.iter().step_by(3).min().copied().unwrap();
        let max = data.iter().step_by(3).max().copied().unwrap();
        max - min
    }

    #[test]
    fn atmospheric_light_matches_the_brightest_dark_channel_patch() {
        let width = 10;
        let height = 10;
        let mut data = vec![50u8; width * height * 3];
        for y in 0..2 {
            for x in 0..2 {
                let i = (y * width + x) * 3;
                data[i] = 240;
                data[i + 1] = 230;
                data[i + 2] = 220;
            }
        }
        let image = RawImage {
            width: width as u32,
            height: height as u32,
            bits: 8,
            data,
        };
        let px = Pixels::from_raw(&image).unwrap();
        let per_pixel_min: Vec<f32> = px
            .data
            .chunks_exact(3)
            .map(|rgb| rgb[0].min(rgb[1]).min(rgb[2]))
            .collect();
        let dark = min_filter(&per_pixel_min, width, height, DEHAZE_PATCH_RADIUS);
        let atmosphere = atmospheric_light(&px, &dark);
        assert!(
            atmosphere[0] > 0.8,
            "expected the bright patch's color, got {atmosphere:?}"
        );
    }

    #[test]
    fn dehaze_positive_amount_widens_the_tonal_range() {
        let image = hazy_test_image(40, 30);
        let neutral = develop(
            &image,
            &Settings {
                process: 10,
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        let dehazed = develop(
            &image,
            &Settings {
                dehaze: 80,
                ..Settings {
                    process: 10,
                    ..Settings::default()
                }
            },
            None,
        )
        .unwrap();
        assert!(
            tonal_range(&dehazed.data) > tonal_range(&neutral.data),
            "dehaze should widen the tonal range: neutral {}, dehazed {}",
            tonal_range(&neutral.data),
            tonal_range(&dehazed.data)
        );
    }

    #[test]
    fn dehaze_negative_amount_narrows_the_tonal_range() {
        let image = hazy_test_image(40, 30);
        let neutral = develop(
            &image,
            &Settings {
                process: 10,
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        let hazier = develop(
            &image,
            &Settings {
                dehaze: -80,
                ..Settings {
                    process: 10,
                    ..Settings::default()
                }
            },
            None,
        )
        .unwrap();
        assert!(
            tonal_range(&hazier.data) < tonal_range(&neutral.data),
            "negative dehaze should narrow the tonal range: neutral {}, hazier {}",
            tonal_range(&neutral.data),
            tonal_range(&hazier.data)
        );
    }
}
