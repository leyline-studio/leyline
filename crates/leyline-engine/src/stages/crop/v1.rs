//! Crop v1 — rank 210, the last stage of the pipeline.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::Crop;

use crate::pixels::Pixels;

/// Extracts the crop rectangle, normalized coordinates rounded to whole
/// pixels, clamped to the frame, at least one pixel each way.
pub(crate) fn crop(px: &Pixels, rect: &Crop) -> Pixels {
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
