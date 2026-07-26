//! Kernel v1 — the primitives shared by the first generation of stages.
//!
//! A body used by more than one stage version cannot live inside any one of
//! them, so it lives here. Versioning applies unchanged: `kernel::v1` is
//! frozen exactly like a stage version, and a primitive whose result must
//! change becomes `kernel::v2` that only new stage versions call. Nothing
//! here is a "shared mutable operator library" — the thing ADR 0028
//! rejected and ADR 0042 still rejects.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).
//!
//! Every body below was moved verbatim from `process11.rs`, itself an
//! unedited copy of `process1.rs`'s originals (plus process 2's tables and
//! process 10's approximate blur). The golden renders are what proves it
//! (`tests/golden_renders.rs`).

use std::sync::OnceLock;

use leyline_core::Point;
use rayon::prelude::*;

use crate::pixels::{Pixels, luma};

/// Runs `op` on every pixel row of the interleaved buffer, in parallel.
pub(crate) fn par_rows(px: &mut Pixels, op: impl Fn(&mut [f32]) + Send + Sync) {
    let row = px.width as usize * 3;
    px.data.par_chunks_mut(row).for_each(op);
}

/// Number of intervals in each transfer lookup table. Frozen: changing it
/// changes the pixels, i.e. requires a new process version.
pub(crate) const LUT_SIZE: usize = 4096;

/// The exact sRGB EOTF, used only to build the table.
pub(crate) fn exact_srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The exact sRGB OETF, used only to build the table.
pub(crate) fn exact_linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// `LUT_SIZE + 1` samples of `f` over [0, 1], entry `i` at `i / LUT_SIZE`.
pub(crate) fn build_table(f: fn(f32) -> f32) -> [f32; LUT_SIZE + 1] {
    let mut table = [0.0f32; LUT_SIZE + 1];
    for (i, entry) in table.iter_mut().enumerate() {
        *entry = f(i as f32 / LUT_SIZE as f32);
    }
    table
}

/// Interpolated lookup: `v` clamped to [0, 1], linear blend between the
/// two surrounding entries.
pub(crate) fn lookup(table: &[f32; LUT_SIZE + 1], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * LUT_SIZE as f32;
    let i = (x as usize).min(LUT_SIZE - 1);
    let t = x - i as f32;
    table[i] + (table[i + 1] - table[i]) * t
}

/// The two tables, built once per process.
pub(crate) fn tables() -> &'static ([f32; LUT_SIZE + 1], [f32; LUT_SIZE + 1]) {
    static TABLES: OnceLock<([f32; LUT_SIZE + 1], [f32; LUT_SIZE + 1])> = OnceLock::new();
    TABLES.get_or_init(|| {
        (
            build_table(exact_srgb_to_linear),
            build_table(exact_linear_to_srgb),
        )
    })
}

/// Approximate color of a blackbody radiator, gamma-encoded RGB in (0, 1].
///
/// Tanner Helland's polynomial fit, frozen as part of process 2 and carried
/// unchanged into process 3, process 4 and process 5. Channels are floored
/// at 0.01 so gain ratios
/// stay finite at extreme temperatures.
pub(crate) fn blackbody_rgb(kelvin: f64) -> [f64; 3] {
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

/// Extracts the luma plane.
pub(crate) fn luma_plane(px: &Pixels) -> Vec<f32> {
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
pub(crate) fn add_luma_delta(px: &mut Pixels, delta: impl Fn(usize) -> f32 + Sync) {
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
pub(crate) fn gaussian_blur(plane: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
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

/// How much smaller a side of the working plane gets before blurring, at
/// `sigma` per [`crate::stages::clarity::v1::CLARITY_RADIUS`]'s scale — bounds the cost of a large-σ
/// blur regardless of how large the source image is.
pub(crate) const APPROX_BLUR_DOWNSAMPLE_DIVISOR: f32 = 4.0;

/// Approximates a large-radius Gaussian blur via downsample → small blur →
/// bilinear upsample (a single-level Gaussian-pyramid approximation)
/// instead of a literal full-resolution kernel (ADR 0033). At `sigma <= 1`
/// downsampling would lose more than it saves, so this falls back to the
/// literal [`gaussian_blur`] directly.
pub(crate) fn approx_blur(plane: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
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

/// Local contrast: the same unsharp-mask math `sharpen` uses
/// (`L' = L + amount·(L − blur(L))`) at a caller-chosen radius — clarity
/// and texture are this one function called twice (ADR 0033), not two
/// independently invented algorithms. Uses [`approx_blur`], not
/// `sharpen`'s literal [`gaussian_blur`]: clarity's radius is large enough
/// that a literal full-resolution kernel would be prohibitively expensive
/// on big previews/exports (`docs/engine-api.md` §11).
pub(crate) fn local_contrast(px: &mut Pixels, amount: i32, radius_px: f32) {
    let k = f32::from(amount as i16) / 100.0;
    let plane = luma_plane(px);
    let blurred = approx_blur(&plane, px.width as usize, px.height as usize, radius_px);
    add_luma_delta(px, |i| k * (plane[i] - blurred[i]));
}

/// Scales chroma around the pixel's luma. Plain `saturation` applies the
/// factor uniformly; `vibrance` weights it by `1 − chroma`.
pub(crate) fn saturate(px: &mut Pixels, amount: i32, vibrance: bool) {
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

/// Bilinear sample at continuous coordinates (pixel centers at n + 0.5);
/// `None` when the point lies outside the source frame. This module's own
/// convention for `rotate`/`crop` — distinct from [`lens_bilinear`], which
/// follows Lensfun's integer-centered convention instead.
pub(crate) fn bilinear(px: &Pixels, sx: f64, sy: f64) -> Option<[f32; 3]> {
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

/// Bilinear sample in Lensfun's own pixel convention: centers at integer
/// coordinates `0..width-1` / `0..height-1`, unlike [`bilinear`] below whose
/// `n + 0.5` convention is this module's own choice for `rotate`. `None`
/// outside the source frame — the caller leaves those samples black, same
/// as an out-of-frame rotation.
pub(crate) fn lens_bilinear(px: &Pixels, sx: f32, sy: f32) -> Option<[f32; 3]> {
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
/// single channel `c` instead of all three. [`crate::stages::lens::v1::correct_tca`] samples each
/// channel at its own TCA-shifted coordinate and only ever keeps that one
/// channel's result, so computing (and discarding) the other two via
/// [`lens_bilinear`] was pure waste. The arithmetic below is copied
/// byte-for-byte from `lens_bilinear`'s per-channel computation — same
/// coefficients, same multiply/add order — so the surviving channel is
/// bit-identical to what `lens_bilinear(px, sx, sy).unwrap()[c]` would have
/// produced.
pub(crate) fn lens_bilinear_channel(px: &Pixels, sx: f32, sy: f32, c: usize) -> Option<f32> {
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

/// Smoothstep ease; `x` must already be in [0, 1].
pub(crate) fn smoothstep01(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

/// Converts a working-buffer RGB sample (gamma-encoded sRGB, [0, 1]) to
/// standard HSL: hue in degrees `[0, 360)`, saturation and lightness in
/// `[0, 1]`. Standard HSL, not the Rec. 709 luma [`luma`] uses elsewhere —
/// ADR 0031 is explicit that the mixer works in RGB-derived HSL.
pub(crate) fn rgb_to_hsl(rgb: &[f32]) -> (f32, f32, f32) {
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
pub(crate) fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
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

/// Maps a point normalized in the post-rotation, pre-crop referential (ADR
/// 0026) back to this still-unrotated buffer's pixel coordinates: the exact
/// inverse of the per-pixel source lookup [`crate::stages::rotate::v1::rotate`] performs later in the
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
