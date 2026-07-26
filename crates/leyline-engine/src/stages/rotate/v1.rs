//! Rotation v1 — rank 200. Process 1's arbitrary-angle rotation.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::bilinear;

/// Rotates clockwise by an arbitrary angle. The output canvas is the axis-
/// aligned bounding box of the rotated frame; samples falling outside the
/// source are black. Sampling is bilinear, geometry is computed in `f64`.
pub(crate) fn rotate(px: &Pixels, degrees: f64) -> Pixels {
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
