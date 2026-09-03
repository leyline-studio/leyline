//! Perspective v1 (ADR 0052) — rank 205, after rotation and before crop.
//!
//! Straightens converging edges: the two sliders move the frame's corners in
//! opposite directions, and the resulting **projective** transform is what
//! makes a building's verticals parallel again. An affine shear would tilt
//! those edges without changing their convergence, which is why the division
//! by the third coordinate below is the whole point (ADR 0052 §2).
//!
//! The canvas grows to the transformed quadrilateral's bounding box and
//! whatever has no source stays black, exactly like `rotate::v1` — recovering a
//! rectangle from that is `crop`'s job, and the user's decision (§4).
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::Perspective;
use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::bilinear;

/// How far a slider at ±100 pushes a corner, as a fraction of the frame.
///
/// A third of the width is a strong but still usable correction: it turns a
/// building shot from well below into parallel verticals, and beyond it the
/// stretch at the far edge becomes visible as softness rather than as
/// perspective.
pub(crate) const MAX_SHIFT: f64 = 1.0 / 3.0;

/// Applies the perspective correction, returning a new buffer.
pub(crate) fn correct(px: &Pixels, perspective: &Perspective) -> Pixels {
    let (w, h) = (f64::from(px.width), f64::from(px.height));
    let vertical = f64::from(perspective.vertical) / 100.0 * MAX_SHIFT;
    let horizontal = f64::from(perspective.horizontal) / 100.0 * MAX_SHIFT;

    // Where the four source corners end up, in pixels. A positive vertical
    // slider squeezes the top and spreads the bottom; a positive horizontal
    // one squeezes the left and spreads the right. Each is expressed as a
    // fraction of the frame, so nothing here depends on the render size
    // (ADR 0052 §5).
    let (dx, dy) = (w * vertical, h * horizontal);
    let corners = [
        (dx, dy),         // top-left
        (w - dx, -dy),    // top-right
        (w + dx, h + dy), // bottom-right
        (-dx, h - dy),    // bottom-left
    ];

    let Some(forward) = homography(w, h, corners) else {
        // A degenerate quadrilateral (three corners collinear) has no
        // homography. Leaving the buffer alone is the honest outcome: the
        // sliders are bounded so this cannot happen from the UI, and an
        // inventive fallback would be a rendering nobody asked for.
        return px.clone();
    };
    let Some(inverse) = invert(forward) else {
        return px.clone();
    };

    // The output canvas is the bounding box of the transformed corners, like
    // `rotate::v1`'s (ADR 0052 §4).
    let mapped: Vec<(f64, f64)> = corners.iter().map(|&(x, y)| (x, y)).collect();
    let min_x = mapped.iter().map(|p| p.0).fold(f64::MAX, f64::min);
    let max_x = mapped.iter().map(|p| p.0).fold(f64::MIN, f64::max);
    let min_y = mapped.iter().map(|p| p.1).fold(f64::MAX, f64::min);
    let max_y = mapped.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let out_w = (max_x - min_x).round().max(1.0) as u32;
    let out_h = (max_y - min_y).round().max(1.0) as u32;

    let mut data = vec![0.0f32; out_w as usize * out_h as usize * 3];
    data.par_chunks_mut(out_w as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, rgb_out) in row.chunks_exact_mut(3).enumerate() {
                let ox = x as f64 + 0.5 + min_x;
                let oy = y as f64 + 0.5 + min_y;
                let denominator = inverse[6] * ox + inverse[7] * oy + inverse[8];
                if denominator.abs() < f64::EPSILON {
                    continue;
                }
                let sx = (inverse[0] * ox + inverse[1] * oy + inverse[2]) / denominator;
                let sy = (inverse[3] * ox + inverse[4] * oy + inverse[5]) / denominator;
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

/// The homography mapping the rectangle `(0,0)-(w,h)` onto `corners`
/// (top-left, top-right, bottom-right, bottom-left), row-major 3×3.
///
/// Solved the classical way: the unit square maps to the quadrilateral in
/// closed form, and the rectangle maps to the unit square by a scaling, so the
/// answer is the product of the two.
pub(crate) fn homography(w: f64, h: f64, corners: [(f64, f64); 4]) -> Option<[f64; 9]> {
    let [(x0, y0), (x1, y1), (x2, y2), (x3, y3)] = corners;
    // Unit square → quadrilateral (Heckbert's derivation).
    let sum_x = x0 - x1 + x2 - x3;
    let sum_y = y0 - y1 + y2 - y3;
    let unit = if sum_x.abs() < 1e-12 && sum_y.abs() < 1e-12 {
        // An affine case: the quadrilateral is a parallelogram.
        [x1 - x0, x2 - x1, x0, y1 - y0, y2 - y1, y0, 0.0, 0.0, 1.0]
    } else {
        let dx1 = x1 - x2;
        let dx2 = x3 - x2;
        let dy1 = y1 - y2;
        let dy2 = y3 - y2;
        let determinant = dx1 * dy2 - dx2 * dy1;
        if determinant.abs() < 1e-12 {
            return None;
        }
        let g = (sum_x * dy2 - dx2 * sum_y) / determinant;
        let hh = (dx1 * sum_y - sum_x * dy1) / determinant;
        [
            x1 - x0 + g * x1,
            x3 - x0 + hh * x3,
            x0,
            y1 - y0 + g * y1,
            y3 - y0 + hh * y3,
            y0,
            g,
            hh,
            1.0,
        ]
    };
    // Rectangle → unit square: divide the input coordinates by the frame.
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let scale = [1.0 / w, 0.0, 0.0, 0.0, 1.0 / h, 0.0, 0.0, 0.0, 1.0];
    Some(multiply(unit, scale))
}

/// Row-major 3×3 product.
fn multiply(a: [f64; 9], b: [f64; 9]) -> [f64; 9] {
    let mut out = [0.0f64; 9];
    for row in 0..3 {
        for column in 0..3 {
            out[row * 3 + column] = (0..3).map(|k| a[row * 3 + k] * b[k * 3 + column]).sum();
        }
    }
    out
}

/// Row-major 3×3 inverse, `None` when singular.
fn invert(m: [f64; 9]) -> Option<[f64; 9]> {
    let cofactor = [
        m[4] * m[8] - m[5] * m[7],
        m[2] * m[7] - m[1] * m[8],
        m[1] * m[5] - m[2] * m[4],
        m[5] * m[6] - m[3] * m[8],
        m[0] * m[8] - m[2] * m[6],
        m[2] * m[3] - m[0] * m[5],
        m[3] * m[7] - m[4] * m[6],
        m[1] * m[6] - m[0] * m[7],
        m[0] * m[4] - m[1] * m[3],
    ];
    let determinant = m[0] * cofactor[0] + m[1] * cofactor[3] + m[2] * cofactor[6];
    if determinant.abs() < 1e-12 {
        return None;
    }
    let mut inverse = [0.0f64; 9];
    for (out, value) in inverse.iter_mut().zip(cofactor) {
        *out = value / determinant;
    }
    Some(inverse)
}
