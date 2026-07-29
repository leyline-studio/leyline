//! Input v2 (ADR 0050) — rank 0, ahead of every other stage.
//!
//! The pipeline's entry point, and it carries two things a revision has to
//! pin together: what the decoder is asked to produce, and what turns that
//! into the working space every operator downstream assumes — linear
//! Rec. 2020, unbounded above (ADR 0044 §1–2).
//!
//! A copy of `v1` with one addition: the decoder is told what to do with
//! channels that saturated at the sensor ([`decode_params`]). `v1` asked for
//! LibRaw's default — clip at white — without ever naming it, which threw
//! away what the unsaturated channels still knew about a blown highlight
//! (ADR 0050 §1). Everything else, including the whole conversion into the
//! working space below, is `v1` verbatim: the duplication is the price of the
//! freeze (ADR 0042 §1).
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
use leyline_core::{HighlightReconstruction, Settings};
use leyline_raw::{DecodeParams, HighlightMode};

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
///
/// The highlight mode, unlike those, *is* a property of the revision — it
/// changes pixels — which is why this version exists (ADR 0050 §4).
pub(crate) fn decode_params(settings: &Settings, half_size: bool) -> DecodeParams {
    DecodeParams {
        half_size,
        sixteen_bit: true,
        camera_native: true,
        auto_brighten: false,
        highlight: match settings.highlight_reconstruction {
            HighlightReconstruction::Clip => HighlightMode::Clip,
            HighlightReconstruction::Blend => HighlightMode::Blend,
            HighlightReconstruction::Rebuild => HighlightMode::Rebuild,
        },
    }
}

/// Brings the decoded buffer into the working space.
///
/// One step ahead of `v1`'s: when the decoder was asked to reconstruct
/// highlights rather than clip them, the gain it normalized the whole image by
/// is undone first (ADR 0050 §3, [`reconstruction_gain`]).
pub(crate) fn to_working_space(
    px: &mut Pixels,
    source: SourceColor,
    has_camera_profile: bool,
    reconstruction: HighlightReconstruction,
) {
    if !reconstruction.is_clip() {
        if let SourceColor::Camera {
            multipliers: Some(multipliers),
            ..
        } = source
        {
            let gain = reconstruction_gain(multipliers);
            if gain > 1.0 {
                let gain = gain as f32;
                par_rows(px, |row| {
                    for sample in row.iter_mut() {
                        *sample *= gain;
                    }
                });
            }
        }
    }
    match source {
        // The camera profile stage owns this conversion (ADR 0035).
        SourceColor::Camera { .. } if has_camera_profile => {}
        SourceColor::Camera {
            to_xyz: Some(matrix),
            ..
        } => {
            if let Some(to_working) = camera_to_rec2020(matrix) {
                matrix_in_place(px, to_working);
            }
        }
        SourceColor::Camera { to_xyz: None, .. } => {}
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

/// The gain that undoes the decoder's own renormalization when it is asked
/// for highlight reconstruction (ADR 0050 §3).
///
/// dcraw — and LibRaw after it — normalizes the image by the white balance
/// multipliers, and picks which one to divide by depending on the highlight
/// mode: the *smallest* when clipping, so every multiplier ends up at or above
/// one and the brightest channel saturates; the *largest* when reconstructing,
/// so no channel can exceed white and nothing has to be thrown away. The
/// second choice darkens the whole image by the ratio between the two, and
/// that ratio is what this returns.
///
/// Undoing it is what makes reconstruction a *highlight* operation instead of
/// a global exposure change: mid-tones come back exactly where clipping put
/// them, and the recovered highlights land above white, where the unbounded
/// working buffer of ADR 0044 keeps them until `output_rendering` decides
/// what becomes of them.
///
/// Non-positive entries are skipped: a three-color sensor leaves the fourth
/// multiplier at zero. A degenerate set (nothing positive, or a ratio that is
/// not finite) yields `1.0` — no compensation rather than a wrong one.
pub(crate) fn reconstruction_gain(multipliers: [f64; 4]) -> f64 {
    let positive = multipliers.iter().copied().filter(|m| *m > 0.0);
    let (smallest, largest) =
        positive.fold((f64::MAX, 0.0f64), |(min, max), m| (min.min(m), max.max(m)));
    if smallest <= 0.0 || !smallest.is_finite() || !largest.is_finite() {
        return 1.0;
    }
    let gain = largest / smallest;
    if gain.is_finite() { gain.max(1.0) } else { 1.0 }
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
