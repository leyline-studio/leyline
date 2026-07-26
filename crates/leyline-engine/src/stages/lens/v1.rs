//! Lens correction v1 (ADR 0016) — rank 20. Distortion only.
//!
//! Introduced by process 3, the first version to render lens correction at
//! all: processes 1 and 2 declare the setting and ignore it, which is why
//! they carry no lens stage in their expansion rather than an inert one.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::render::LensShot;
use crate::stages::kernel::v1::lens_bilinear;

/// Undistorts the image geometrically using the shot's Lensfun profile, when
/// one matches. No match — unknown camera or lens, or no calibration at this
/// focal length — leaves `px` unchanged: correction is only ever applied
/// from real calibration data, never approximated.
pub(crate) fn correct_lens(px: &Pixels, shot: &LensShot) -> Pixels {
    let Some(profile) = leyline_lens::find_profile(
        &shot.camera_make,
        &shot.camera_model,
        shot.lens_make.as_deref(),
        shot.lens_model.as_deref().unwrap_or(""),
    ) else {
        return px.clone();
    };
    let correction = leyline_lens::Correction::new(&profile, shot.focal_mm, px.width, px.height);
    if !correction.distortion_matched() {
        // No distortion calibration at this focal length: `source_row`
        // would return the identity map, and resampling at identity
        // coordinates is bit-identical to the original pixel (integer
        // coordinates make every bilinear weight exactly 0.0 or 1.0) — so
        // skip the whole per-pixel pass rather than pay for a no-op.
        return px.clone();
    }

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
