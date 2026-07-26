//! Camera profile v1 (ADR 0035) — rank 10, ahead of every other stage.
//!
//! Introduced by process 11. Converts the camera-native RGB the decoder
//! produced into the gamma-encoded sRGB working buffer every downstream
//! stage assumes, so it necessarily runs first.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_color::DcpProfile;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{lookup, par_rows, tables};

/// Converts `px` from camera-native linear RGB (what the decoder produced
/// when `DecodeParams::camera_native` was set) to gamma-encoded linear-sRGB,
/// via `profile`'s camera→XYZ→linear-sRGB matrix
/// ([`leyline_color::DcpProfile::camera_to_linear_srgb`]). Runs before every
/// other operator (ADR 0035): everything downstream assumes a gamma-encoded
/// sRGB working buffer, and this is the stage that produces it when a
/// profile is in play.
pub(crate) fn apply_camera_profile(px: &mut Pixels, profile: &DcpProfile) {
    let (_, to_srgb) = tables();
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let linear_srgb = profile.camera_to_linear_srgb([
                f64::from(rgb[0]),
                f64::from(rgb[1]),
                f64::from(rgb[2]),
            ]);
            for (sample, value) in rgb.iter_mut().zip(linear_srgb) {
                *sample = lookup(to_srgb, value as f32);
            }
        }
    });
}
