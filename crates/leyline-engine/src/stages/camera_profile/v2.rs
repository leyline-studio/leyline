//! Camera profile v2 (ADR 0062) — rank 10, ahead of every other stage.
//!
//! A copy of `v1`'s work with one change: the profile's two calibrations are
//! **interpolated for the scene's light** instead of averaged. `v1` blended
//! tungsten and daylight in equal parts whatever the photograph was, which
//! matches no real illuminant; measured against the correct calibration it
//! cost up to 0.044 on a saturated red — eleven levels out of 255 — while
//! leaving a neutral grey untouched, which is how it went unnoticed.
//!
//! The scene temperature comes from the revision when it names one, and
//! from the camera's as-shot neutral otherwise (ADR 0062 §2). The matrix is
//! resolved **once per render**, not per pixel: the light does not change
//! within one image.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_color::DcpProfile;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::par_rows;

/// Converts `px` from camera-native linear RGB into linear Rec. 2020 through
/// the calibration `temperature_k` calls for.
pub(crate) fn apply_camera_profile(px: &mut Pixels, profile: &DcpProfile, temperature_k: f64) {
    let matrix = profile.camera_to_xyz_at(temperature_k);
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let working = leyline_color::xyz_d50_to_linear_rec2020(apply(
                matrix,
                [f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])],
            ));
            for (sample, value) in rgb.iter_mut().zip(working) {
                *sample = (value as f32).max(0.0);
            }
        }
    });
}

/// `m · v`.
fn apply(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}
