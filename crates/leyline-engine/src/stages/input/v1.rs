//! Input v1 (ADR 0044 §3) — rank 0, ahead of every other stage.
//!
//! The pipeline's entry point: what the decoder is asked to produce, and
//! what has to happen to the buffer before the first operator sees it. At
//! this version the second half is empty — LibRaw already hands over
//! gamma-encoded sRGB, and `camera_profile::v1` does the matrix when a DCP
//! is in play — so all this version carries is the decoder configuration.
//!
//! That configuration is not a detail: `camera_native` switches LibRaw
//! between its own sRGB conversion and a linear camera-space output, which
//! is a different image. Until ADR 0044 it was decided by four copies of
//! `camera_native: camera_profile.is_some()` scattered across the preview,
//! export and print paths, and recorded in no `stages` map — the one input
//! to a rendering that a revision did not describe.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_raw::DecodeParams;

use crate::stages::InputRequest;

/// The decoder configuration this version calls for.
///
/// `camera_native` follows the camera profile, and only it: a DCP matrix
/// converts *camera-native* linear RGB, so LibRaw must be told not to apply
/// its own conversion first (ADR 0035). Without a profile the reference
/// rendering is LibRaw's ordinary gamma-encoded sRGB.
///
/// `half_size` is the caller's size class (ADR 0041), not a property of the
/// revision: the same revision renders through the same version whether it
/// is being previewed or exported.
pub(crate) fn decode_params(request: InputRequest) -> DecodeParams {
    DecodeParams {
        half_size: request.half_size,
        camera_native: request.has_camera_profile,
        ..DecodeParams::default()
    }
}
