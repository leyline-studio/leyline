//! Process version 3 — the third rendering contract of the develop pipeline
//! (ADR 0016).
//!
//! This module *is* the definition of `process: 3`: the exact formulas
//! below, applied in the fixed order of `docs/pipeline.md` §3.1, are
//! frozen. Any change that alters the pixels they produce must land as a
//! new process version in a new module — this one is kept as-is forever
//! (§3.3).
//!
//! Process 3 differs from process 2 in exactly one place: `lens_correction`
//! is now rendered instead of being a declared-but-inert setting. When
//! enabled, the shot's camera and lens EXIF strings are matched against
//! Lensfun's bundled profile database (`leyline_lens::find_profile`); a
//! match yields a per-pixel backward coordinate map that undistorts the
//! image before any tonal operator runs — the first step of §3.1's fixed
//! order. No match (unknown gear, or no [`LensShot`] at all) leaves the
//! image untouched: EXIF is best-effort, correction is never guessed.
//! Vignetting and transverse chromatic aberration are not corrected by this
//! process version — only geometric distortion — a deliberate scope cut,
//! not a limitation of Lensfun itself. Every other operator is copied from
//! process 2 unchanged, so this module stays self-contained and frozen.
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

use std::sync::OnceLock;

use leyline_core::Result;
use leyline_core::{Crop, Settings, WhiteBalance};
use leyline_raw::RawImage;
use rayon::prelude::*;

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
/// already validated and confirmed to declare `process: 3`. `shot` is the
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
            px = correct_lens(&px, shot);
        }
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
// Lens correction (the process 3 difference)
// ---------------------------------------------------------------------------

/// Undistorts the image geometrically using the shot's Lensfun profile, when
/// one matches. No match — unknown camera or lens, or no calibration at this
/// focal length — leaves `px` unchanged: correction is only ever applied
/// from real calibration data, never approximated.
fn correct_lens(px: &Pixels, shot: &LensShot) -> Pixels {
    let Some(profile) = leyline_lens::find_profile(
        &shot.camera_make,
        &shot.camera_model,
        shot.lens_make.as_deref(),
        shot.lens_model.as_deref().unwrap_or(""),
    ) else {
        return px.clone();
    };
    let correction = leyline_lens::Correction::new(&profile, shot.focal_mm, px.width, px.height);

    let mut data = vec![0.0f32; px.data.len()];
    data.par_chunks_mut(px.width as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            let sources = correction.source_row(y as u32, px.width);
            for (x, rgb_out) in row.chunks_exact_mut(3).enumerate() {
                let (sx, sy) = sources[x];
                if let Some(rgb) = lens_bilinear(px, sx, sy) {
                    rgb_out.copy_from_slice(&rgb);
                }
            }
        });
    Pixels {
        width: px.width,
        height: px.height,
        data,
    }
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
/// unchanged into process 3. Channels are floored at 0.01 so gain ratios
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
        }
    }

    #[test]
    fn disabled_lens_correction_matches_process_2_bit_for_bit() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 3,
            exposure: 0.3,
            contrast: 20,
            ..Settings::default()
        };
        let with_process_3 = develop(&image, &settings, Some(&canon_shot(20.0))).unwrap();
        let with_process_2 = crate::process2::develop(&image, &settings).unwrap();
        assert_eq!(with_process_3, with_process_2);
    }

    #[test]
    fn no_shot_leaves_the_image_unchanged_even_when_enabled() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 3,
            lens_correction: LensCorrection {
                enabled: true,
                profile: "auto".to_owned(),
            },
            ..Settings::default()
        };
        let out = develop(&image, &settings, None).unwrap();
        assert_eq!(out.data, image.data);
    }

    #[test]
    fn unmatched_gear_leaves_the_image_unchanged() {
        let image = test_image(64, 48);
        let settings = Settings {
            process: 3,
            lens_correction: LensCorrection {
                enabled: true,
                profile: "auto".to_owned(),
            },
            ..Settings::default()
        };
        let shot = LensShot {
            camera_make: "Nobody".to_owned(),
            camera_model: "Nothing".to_owned(),
            lens_make: Some("Nobody".to_owned()),
            lens_model: Some("Nothing".to_owned()),
            focal_mm: 20.0,
        };
        let out = develop(&image, &settings, Some(&shot)).unwrap();
        assert_eq!(out.data, image.data);
    }

    #[test]
    fn a_matched_profile_undistorts_the_image() {
        let image = test_image(640, 480);
        let settings = Settings {
            process: 3,
            lens_correction: LensCorrection {
                enabled: true,
                profile: "auto".to_owned(),
            },
            ..Settings::default()
        };
        let out = develop(&image, &settings, Some(&canon_shot(20.0))).unwrap();
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
            process: 3,
            ..Settings::default()
        };
        let out = develop(&image, &settings, Some(&canon_shot(20.0))).unwrap();
        assert_eq!(out.data, image.data);
    }
}
