//! Camera profile v3 (ADR 0063) — rank 10, ahead of every other stage.
//!
//! `v2` applies a profile's matrices. This applies its **tables** too — the
//! hue/saturation map, the look table and the tone curve — which is what
//! carries a profile's *look* where the matrix only carries its correctness.
//! A profile whose look lives in its tables rendered, until here, as a
//! colorimetrically sound image that did not resemble what its author
//! intended.
//!
//! The order is the specification's, and not the one the names suggest: the
//! look table runs **before** the tone curve, and everything after the
//! matrix happens in ProPhoto RGB rather than in the working space
//! (ADR 0063 §1). The profile is resolved once per render, never per pixel.
//!
//! A profile carrying no table renders here exactly as it does in `v2`,
//! which is what makes reprocessing into this version safe to offer.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_color::DcpProfile;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::par_rows;

/// Converts `px` from camera-native linear RGB into linear Rec. 2020,
/// through the calibration and the tables `temperature_k` calls for.
pub(crate) fn apply_camera_profile(px: &mut Pixels, profile: &DcpProfile, temperature_k: f64) {
    let prepared = profile.prepare(temperature_k);
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let working = prepared.camera_to_working([rgb[0], rgb[1], rgb[2]]);
            for (sample, value) in rgb.iter_mut().zip(working) {
                *sample = value.max(0.0);
            }
        }
    });
}
