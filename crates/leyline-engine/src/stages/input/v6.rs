//! Input v6 (ADR 0115) — rank 0, ahead of every other stage.
//!
//! `v5` with one addition: a non-RAW file that **says** what colour it is is
//! no longer read as sRGB. A phone writes Display P3 in a JPEG as readily as
//! in a HEIC, a camera's in-body JPEG is often Adobe RGB, and until this
//! version every one of them was flattened onto sRGB primaries — a
//! photograph rendered slightly dull, with nothing to say so.
//!
//! The rotation goes **straight into the working space**. Converting to sRGB
//! first would clip every colour outside its gamut before the pipeline that
//! was built to hold them ever saw it, which is exactly what ADR 0044's
//! unbounded Rec. 2020 space exists to prevent.
//!
//! For a RAW, for an untagged file, and for a profile this engine cannot
//! reduce to primaries and a curve, this version renders **bit for bit**
//! what `v5` renders — it calls it. Only a tagged non-RAW source takes the
//! new path.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract: a
//! revision citing this stage version renders through exactly this code,
//! forever.

use leyline_color::{TaggedSource, apply_matrix};
use leyline_core::{Settings, SourceEncoding};

use crate::pixels::Pixels;
use crate::stages::SourceColor;
use crate::stages::kernel::v1::par_rows;

pub(crate) use super::v4::decode_params;

/// Brings the decoded buffer into the working space.
///
/// `profile` is what the file declared, which only this version reads.
pub(crate) fn to_working_space(
    px: &mut Pixels,
    source: SourceColor,
    profile: Option<&TaggedSource>,
    has_camera_profile: bool,
    settings: &Settings,
) {
    // A file that carries its own colour space, and is not camera-native:
    // the only case that is not `v5`'s.
    if let (SourceColor::Srgb, Some(profile)) = (source, profile) {
        if settings.source_encoding == SourceEncoding::LinearWorkspace {
            return;
        }
        let matrix = profile.to_rec2020;
        let transfer = &profile.transfer;
        par_rows(px, |row| {
            for rgb in row.chunks_exact_mut(3) {
                let linear = [
                    transfer.to_linear(rgb[0]),
                    transfer.to_linear(rgb[1]),
                    transfer.to_linear(rgb[2]),
                ];
                let working = apply_matrix(
                    matrix,
                    [
                        f64::from(linear[0]),
                        f64::from(linear[1]),
                        f64::from(linear[2]),
                    ],
                );
                // Floored at zero and unbounded above, like every other
                // sample in this pipeline (ADR 0044 §2): a primary rotation
                // can send a saturated colour slightly negative, and that is
                // a colour outside the working space, not a colour to keep.
                for (sample, value) in rgb.iter_mut().zip(working) {
                    *sample = (value as f32).max(0.0);
                }
            }
        });
        return;
    }
    super::v5::to_working_space(px, source, has_camera_profile, settings);
}
