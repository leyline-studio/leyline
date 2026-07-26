//! The render pipeline's working space: linear Rec. 2020, D65 (ADR 0044).
//!
//! Everything here is colorimetry, not rendering: fixed matrices, and the
//! one derivation that depends on the camera (its native RGB into the
//! working space). The pipeline's frozen stage modules call into it; the
//! matrices themselves are constants and cannot drift.
//!
//! Rec. 2020 is what ADR 0044 §1 chose, and its white point is why: D65,
//! like sRGB, Display P3 and Adobe RGB, so no chromatic adaptation has to
//! appear anywhere between the sensor and the screen. Its primaries are
//! real spectral colors, unlike ProPhoto's, and it covers photographic
//! sensors comfortably.

use crate::dcp::Matrix3;

/// Linear Rec. 2020 (D65) → linear sRGB (D65).
///
/// The output-side matrix: rows sum to 1, so the working space's white is
/// sRGB's white exactly. Colors outside sRGB come out negative or above 1 —
/// that is the gamut clipping ADR 0044 defers to the very end of the
/// pipeline instead of paying at the beginning.
pub const REC2020_TO_LINEAR_SRGB: Matrix3 = [
    [1.660_491, -0.587_641, -0.072_850],
    [-0.124_551, 1.132_900, -0.008_349],
    [-0.018_151, -0.100_579, 1.118_730],
];

/// Linear sRGB (D65) → linear Rec. 2020 (D65), the inverse of
/// [`REC2020_TO_LINEAR_SRGB`]. Used on the way *in* for sources that are
/// already sRGB — JPEG, PNG and TIFF imports.
pub const LINEAR_SRGB_TO_REC2020: Matrix3 = [
    [0.627_404, 0.329_283, 0.043_313],
    [0.069_097, 0.919_540, 0.011_362],
    [0.016_391, 0.088_013, 0.895_595],
];

/// Linear Rec. 2020 (D65) → XYZ (D65).
pub const REC2020_TO_XYZ_D65: Matrix3 = [
    [0.636_958, 0.144_617, 0.168_881],
    [0.262_700, 0.677_998, 0.059_302],
    [0.000_000, 0.028_073, 1.060_985],
];

/// XYZ (D65) → linear Rec. 2020 (D65).
pub const XYZ_D65_TO_REC2020: Matrix3 = [
    [1.716_651, -0.355_671, -0.253_366],
    [-0.666_684, 1.616_481, 0.015_769],
    [0.017_640, -0.042_771, 0.942_103],
];

/// Applies a 3×3 matrix to an RGB triple.
pub fn apply_matrix(m: Matrix3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Derives a camera's native-RGB → linear Rec. 2020 matrix from the
/// XYZ→camera matrix LibRaw reports for that body
/// ([`leyline_raw::RawMetadata::camera_to_xyz`]).
///
/// The normalization step is dcraw's, kept deliberately: the rows of the
/// Rec. 2020→camera matrix are scaled to sum to one, so a neutral camera
/// triple `(1, 1, 1)` — which is what the decoder's white balance already
/// produced — lands on a neutral working-space triple. Without it the white
/// balance the sensor recorded would be undone by the color matrix.
///
/// `None` when the matrix cannot be inverted, which in practice means
/// LibRaw handed over something degenerate; the caller then has no camera
/// colorimetry and must say so rather than guess.
pub fn camera_to_rec2020(camera_to_xyz: Matrix3) -> Option<Matrix3> {
    // XYZ→camera composed with Rec.2020→XYZ gives Rec.2020→camera.
    let mut rec2020_to_camera = [[0.0f64; 3]; 3];
    for (row, out) in rec2020_to_camera.iter_mut().enumerate() {
        for (column, cell) in out.iter_mut().enumerate() {
            *cell = (0..3)
                .map(|k| camera_to_xyz[row][k] * REC2020_TO_XYZ_D65[k][column])
                .sum();
        }
        let sum: f64 = out.iter().sum();
        if sum.abs() < 1e-12 {
            return None;
        }
        for cell in out.iter_mut() {
            *cell /= sum;
        }
    }
    crate::dcp::invert(rec2020_to_camera)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: [f64; 3], expected: [f64; 3], tolerance: f64, what: &str) {
        for (a, e) in actual.iter().zip(expected.iter()) {
            assert!(
                (a - e).abs() < tolerance,
                "{what}: {actual:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn white_stays_white_in_both_directions() {
        assert_close(
            apply_matrix(REC2020_TO_LINEAR_SRGB, [1.0, 1.0, 1.0]),
            [1.0, 1.0, 1.0],
            1e-5,
            "Rec.2020 white → sRGB white",
        );
        assert_close(
            apply_matrix(LINEAR_SRGB_TO_REC2020, [1.0, 1.0, 1.0]),
            [1.0, 1.0, 1.0],
            1e-5,
            "sRGB white → Rec.2020 white",
        );
        assert_close(
            apply_matrix(XYZ_D65_TO_REC2020, [0.950_47, 1.0, 1.088_83]),
            [1.0, 1.0, 1.0],
            1e-3,
            "D65 white point → Rec.2020 white",
        );
    }

    #[test]
    fn the_two_srgb_matrices_are_inverses() {
        for color in [[0.2, 0.5, 0.9], [1.0, 0.0, 0.0], [0.3, 0.3, 0.3]] {
            let round_trip = apply_matrix(
                REC2020_TO_LINEAR_SRGB,
                apply_matrix(LINEAR_SRGB_TO_REC2020, color),
            );
            assert_close(round_trip, color, 1e-4, "sRGB → Rec.2020 → sRGB");
        }
    }

    #[test]
    fn a_saturated_srgb_primary_fits_inside_rec2020() {
        // The point of the wider space: sRGB's most saturated green is an
        // ordinary, unremarkable color in Rec. 2020 — room is left on every
        // side for the sensor's own greens.
        let green = apply_matrix(LINEAR_SRGB_TO_REC2020, [0.0, 1.0, 0.0]);
        assert!(
            green.iter().all(|c| *c >= 0.0 && *c <= 1.0),
            "sRGB green should sit inside Rec. 2020: {green:?}"
        );
        assert!(green[1] < 0.93, "and strictly inside it: {green:?}");
    }

    #[test]
    fn a_camera_matrix_maps_a_neutral_sensor_triple_to_neutral() {
        // The Canon EOS 60D's matrix, as LibRaw reports it. What must hold
        // is the normalization: the decoder has already applied the camera's
        // white balance, so (1, 1, 1) is neutral by the time the matrix
        // runs, and it has to stay neutral.
        let cam_xyz = [
            [0.671_9, -0.099_4, -0.092_5],
            [-0.440_8, 1.242_6, 0.221_1],
            [-0.088_7, 0.212_9, 0.605_1],
        ];
        let matrix = camera_to_rec2020(cam_xyz).expect("a real camera matrix inverts");
        assert_close(
            apply_matrix(matrix, [1.0, 1.0, 1.0]),
            [1.0, 1.0, 1.0],
            1e-6,
            "neutral camera triple → neutral working space",
        );
    }

    #[test]
    fn a_degenerate_camera_matrix_is_refused() {
        assert_eq!(camera_to_rec2020([[0.0; 3]; 3]), None);
    }
}
