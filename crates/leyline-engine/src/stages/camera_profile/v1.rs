//! Camera profile v1 (ADR 0035) — rank 10, ahead of every other stage.
//!
//! Converts the camera-native RGB the decoder produced into the working
//! space every downstream stage assumes — linear Rec. 2020 since ADR 0044 —
//! so it necessarily runs first.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_color::DcpProfile;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::par_rows;

/// Converts `px` from camera-native linear RGB (what the decoder produced
/// when `DecodeParams::camera_native` was set) into linear Rec. 2020, via
/// `profile`'s camera→XYZ matrix
/// ([`leyline_color::DcpProfile::camera_to_linear_rec2020`]).
///
/// Runs before every other operator (ADR 0035), and replaces the `input`
/// stage's own conversion when a profile is in play: the profile *is* the
/// camera's colorimetry, so having both apply a matrix would be applying it
/// twice.
///
/// Only the negative side is clipped — a color the working space cannot
/// express at all. Everything above white is kept: this is where a RAW's
/// highlight headroom enters the pipeline, and clipping it here is exactly
/// what ADR 0044 set out to stop doing.
pub(crate) fn apply_camera_profile(px: &mut Pixels, profile: &DcpProfile) {
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let working = profile.camera_to_linear_rec2020([
                f64::from(rgb[0]),
                f64::from(rgb[1]),
                f64::from(rgb[2]),
            ]);
            for (sample, value) in rgb.iter_mut().zip(working) {
                *sample = (value as f32).max(0.0);
            }
        }
    });
}
