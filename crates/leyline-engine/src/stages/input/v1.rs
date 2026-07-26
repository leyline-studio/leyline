//! Input v1 (ADR 0044 §3) — rank 0, ahead of every other stage.
//!
//! The pipeline's entry point, and it carries two things a revision has to
//! pin together: what the decoder is asked to produce, and what turns that
//! into the working space every operator downstream assumes — linear
//! Rec. 2020, unbounded above (ADR 0044 §1–2).
//!
//! **The decoder is asked for camera-native linear data and nothing else.**
//! LibRaw can convert to sRGB itself, and did until ADR 0044, but it clips
//! the result to that gamut on the way out: every color the sensor saw
//! outside sRGB would be gone before the first slider ran. Asking for the
//! sensor's own numbers and applying the matrix here is what makes a
//! wide-gamut working space real rather than nominal.
//!
//! Which matrix depends on what the revision has:
//!
//! * a camera profile (DCP) — then this stage leaves the buffer alone and
//!   `camera_profile::v1` does the conversion from the profile's own
//!   matrices (ADR 0035). Applying both would apply colorimetry twice;
//! * no profile, but a body LibRaw knows — its table matrix, normalized so
//!   the decoder's white balance survives it ([`leyline_color::camera_to_rec2020`]);
//! * neither — the sensor's numbers are all we have, and the honest thing
//!   is to leave them alone rather than invent a colorimetry for them;
//! * a JPEG/PNG/TIFF import, which is already sRGB: decode its transfer
//!   function and rotate the primaries into the working space.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_color::{LINEAR_SRGB_TO_REC2020, apply_matrix, camera_to_rec2020};
use leyline_raw::DecodeParams;

use crate::pixels::Pixels;
use crate::stages::SourceColor;
use crate::stages::kernel::v1::{lookup, par_rows, tables};

/// The decoder configuration this version calls for.
///
/// `camera_native` is unconditional for RAW sources — see the module docs.
/// Sixteen bits likewise: the buffer is linear from here on, and eight-bit
/// linear samples would put barely a dozen distinct values in the shadows,
/// where the eye takes the most.
///
/// `half_size` is the caller's size class (ADR 0041), not a property of the
/// revision: the same revision renders through the same version whether it
/// is being previewed or exported.
pub(crate) fn decode_params(half_size: bool) -> DecodeParams {
    DecodeParams {
        half_size,
        sixteen_bit: true,
        camera_native: true,
        auto_brighten: false,
    }
}

/// Brings the decoded buffer into the working space.
pub(crate) fn to_working_space(px: &mut Pixels, source: SourceColor, has_camera_profile: bool) {
    match source {
        // The camera profile stage owns this conversion (ADR 0035).
        SourceColor::Camera { .. } if has_camera_profile => {}
        SourceColor::Camera {
            to_xyz: Some(matrix),
        } => {
            if let Some(to_working) = camera_to_rec2020(matrix) {
                matrix_in_place(px, to_working);
            }
        }
        SourceColor::Camera { to_xyz: None } => {}
        SourceColor::Srgb => {
            let (to_linear, _) = tables();
            par_rows(px, |row| {
                for sample in row.iter_mut() {
                    *sample = lookup(to_linear, *sample);
                }
                for rgb in row.chunks_exact_mut(3) {
                    let working = apply_matrix(
                        LINEAR_SRGB_TO_REC2020,
                        [f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])],
                    );
                    for (sample, value) in rgb.iter_mut().zip(working) {
                        *sample = (value as f32).max(0.0);
                    }
                }
            });
        }
    }
}

/// Applies a color matrix to every pixel, clipping only what the working
/// space cannot express at all (negatives), never the highlights.
fn matrix_in_place(px: &mut Pixels, matrix: leyline_color::Matrix3) {
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let working = apply_matrix(
                matrix,
                [f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])],
            );
            for (sample, value) in rgb.iter_mut().zip(working) {
                *sample = (value as f32).max(0.0);
            }
        }
    });
}
