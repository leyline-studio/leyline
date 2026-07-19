//! Process version 2 — the second rendering contract of the develop
//! pipeline (ADR 0013).
//!
//! This module *is* the definition of `process: 2`: the exact formulas
//! below, applied in the fixed order of `docs/pipeline.md` §3.1, are
//! frozen. Any change that alters the pixels they produce must land as a
//! new process version in a new module — this one is kept as-is forever
//! (§3.3).
//!
//! Process 2 differs from process 1 in exactly one place: the sRGB
//! transfer functions of the linear-light operators (white balance and
//! exposure) are computed through lookup tables of [`LUT_SIZE`] intervals
//! with linear interpolation, instead of calling `powf` per sample. The
//! tables are part of the contract: entry `i` is the exact process-1
//! formula evaluated at `i / LUT_SIZE`, and lookups interpolate linearly
//! between adjacent entries in `f32`. The approximation error (< 2·10⁻⁵ in
//! gamma domain) is invisible in 8-bit output but not bit-identical to
//! process 1 — hence the new version. Every other operator is copied from
//! process 1 unchanged, so this module stays self-contained and frozen.
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
//!
//! Lens correction is declared in schema 1 but **not rendered** by
//! process 2: profile-based corrections arrive with a later process
//! version. This is part of the contract — `enabled: true` renders
//! identically to `false`.

use std::sync::OnceLock;

use leyline_core::Result;
use leyline_core::{Crop, Settings, WhiteBalance};
use leyline_raw::RawImage;
use rayon::prelude::*;

use crate::pixels::{Pixels, luma};
use crate::render::Rendered;

// ---------------------------------------------------------------------------
// Transfer functions by lookup table (the process 2 difference)
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
/// already validated and confirmed to declare `process: 2`.
pub(crate) fn develop(image: &RawImage, settings: &Settings) -> Result<Rendered> {
    let mut px = Pixels::from_raw(image)?;

    // Lens correction: declared, not rendered (see module docs).

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
    if settings.vibrance != 0 {
        saturate(&mut px, settings.vibrance, true);
    }
    if settings.saturation != 0 {
        saturate(&mut px, settings.saturation, false);
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
// White balance + exposure (linear light, through the tables)
// ---------------------------------------------------------------------------

/// Applies white balance and exposure as per-channel gains in linear light.
///
/// Identical to process 1 except that the conversions to and from linear
/// light go through the lookup tables.
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
/// Tanner Helland's polynomial fit, frozen as part of process 2. Channels
/// are floored at 0.01 so gain ratios stay finite at extreme temperatures.
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
///
/// Positive values blend toward the smoothstep curve `x²(3 − 2x)`; negative
/// values blend toward the half-slope line through 0.5. Both blends are
/// monotone and fix middle gray.
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
/// `(1 − L)²`, `highlights` on bright pixels with weight `L²`. A positive
/// delta pushes toward white proportionally to the remaining headroom, a
/// negative one toward black proportionally to the current value — so the
/// endpoints are fixed and the response is monotone.
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
/// Full slider deflection moves an endpoint by a quarter of the range.
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
// Color (gamma domain)
// ---------------------------------------------------------------------------

/// Scales chroma around the pixel's luma. Plain `saturation` applies the
/// factor uniformly; `vibrance` weights it by `1 − chroma`, so muted colors
/// move more than already-saturated ones.
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
// Noise reduction and sharpening (gamma domain, luma/chroma split)
// ---------------------------------------------------------------------------

/// Blends the luma plane toward its Gaussian blur; chroma is untouched.
/// Strength 0–100 maps linearly to both the blur radius (σ up to 2 px) and
/// the blend factor.
fn luminance_noise_reduction(px: &mut Pixels, strength: i32) {
    let k = f32::from(strength as i16) / 100.0;
    let plane = luma_plane(px);
    let blurred = gaussian_blur(&plane, px.width as usize, px.height as usize, k * 2.0);
    add_luma_delta(px, |i| k * (blurred[i] - plane[i]));
}

/// Blends the chroma planes (per-channel deviation from luma) toward their
/// Gaussian blur. Strength 0–100 maps to σ up to 3 px and to the blend.
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

/// Unsharp mask on the luma plane only, so sharpening never fringes colors:
/// `L' = L + amount/100 · (L − blur(L, σ = radius))`.
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
// Geometry
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
/// `None` when the point lies outside the source frame.
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

    #[test]
    fn lookup_matches_the_exact_functions_at_the_domain_endpoints() {
        // `exact_linear_to_srgb(1.0)` is 0.99999994 in f32 — same as in
        // process 1, and rounded back to 255 in 8-bit output; the lookup
        // must reproduce the exact functions, not idealized endpoints.
        let (to_linear, to_srgb) = tables();
        assert_eq!(lookup(to_linear, 0.0), exact_srgb_to_linear(0.0));
        assert_eq!(lookup(to_linear, 1.0), exact_srgb_to_linear(1.0));
        assert_eq!(lookup(to_srgb, 0.0), exact_linear_to_srgb(0.0));
        assert_eq!(lookup(to_srgb, 1.0), exact_linear_to_srgb(1.0));
        // Out-of-range inputs clamp instead of reading out of bounds.
        assert_eq!(lookup(to_srgb, -0.5), lookup(to_srgb, 0.0));
        assert_eq!(lookup(to_srgb, 1.5), lookup(to_srgb, 1.0));
    }
}
