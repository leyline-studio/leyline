//! Tone curve v1 (ADR 0024) — rank 80, after the basic tonal sliders.
//!
//! Introduced by process 6. A monotone cubic Hermite interpolant through the
//! control points, sampled once into a table and looked up per sample.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::CurvePoint;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{display_curve, in_display};

/// Number of intervals in the tone curve lookup table. Frozen alongside
/// [`crate::stages::kernel::v1::LUT_SIZE`]: changing it changes the pixels, i.e. requires a new process
/// version.
pub(crate) const CURVE_LUT_SIZE: usize = 4096;

/// Applies the tone curve identically to every channel, via an interpolated
/// lookup into a table precomputed once from `points` (ADR 0030: LUT, not a
/// per-pixel spline evaluation).
pub(crate) fn tone_curve(px: &mut Pixels, points: &[CurvePoint]) {
    let table = build_curve_lut(points);
    in_display(px, |px| {
        display_curve(px, |x| curve_lookup(&table, x));
    });
}

/// Interpolated lookup into a `CURVE_LUT_SIZE + 1`-entry table spanning
/// [0, 1], same convention as [`crate::stages::kernel::v1::lookup`] above but a fixed-size array of a
/// different length, hence its own function.
pub(crate) fn curve_lookup(table: &[f32; CURVE_LUT_SIZE + 1], v: f32) -> f32 {
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
pub(crate) fn build_curve_lut(points: &[CurvePoint]) -> [f32; CURVE_LUT_SIZE + 1] {
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
pub(crate) fn eval_hermite(xs: &[f64], ys: &[f64], tangents: &[f64], x: f64) -> f64 {
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
