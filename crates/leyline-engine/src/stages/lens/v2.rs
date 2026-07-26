//! Lens correction v2 (ADR 0017) — rank 20. Distortion, then vignetting.
//!
//! Introduced by process 4. The distortion pass is process 3's, unchanged;
//! vignetting is the addition.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{lens_bilinear, lookup, tables};

/// Undistorts the image geometrically using an already-matched Lensfun
/// profile. No distortion calibration at this focal length leaves `px`
/// unchanged: correction is only ever applied from real calibration data,
/// never approximated. Identical to process 3's `correct_lens`, split from
/// the profile lookup so [`devignette`] can reuse the same match.
pub(crate) fn undistort(px: &Pixels, profile: &leyline_lens::Profile, focal_mm: f32) -> Pixels {
    let correction = leyline_lens::Correction::new(profile, focal_mm, px.width, px.height);
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

/// Corrects corner darkening (vignetting) using an already-matched Lensfun
/// profile, `aperture_f` and [`leyline_lens::Vignetting`]'s assumed subject
/// distance. No vignetting calibration for this focal/aperture pair leaves
/// `px` unchanged. The gain is a radial multiplier defined in linear light
/// (a physical falloff of incoming light), so each sample round-trips
/// through the same transfer tables as [`crate::stages::gains::v2::linear_gains`] rather than being
/// multiplied directly in gamma space.
pub(crate) fn devignette(
    px: &mut Pixels,
    profile: &leyline_lens::Profile,
    focal_mm: f32,
    aperture_f: f32,
) {
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
