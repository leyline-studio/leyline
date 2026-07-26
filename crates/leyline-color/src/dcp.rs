//! Camera profile (DCP) parsing and application (ADR 0035, ADR 0037).
//!
//! DCP is Adobe's per-camera color calibration format: a handful of
//! private TIFF/EP tags (documented in Adobe's public DNG Specification)
//! carrying color matrices, calibration illuminants, and (optionally) a
//! tone curve and hue/saturation/look tables. It is **not** ICC — `lcms2`
//! cannot read it — which is why this module exists instead of routing
//! through the ICC path this crate already has (`OutputTransform`).
//!
//! Per ADR 0037: the container is read with a home-grown tag reader built
//! on the `tiff` crate's generic IFD decoder (already a workspace
//! dependency, used elsewhere for TIFF *export*) rather than a new
//! DCP-specific dependency — DCP's tag set is small and fully documented,
//! not a reverse-engineered format. This module is read-only: Leyline
//! never authors `.dcp` files.
//!
//! **Scope of the color math applied here, stated plainly:** this reads
//! and applies [`ColorMatrix1`]/[`ForwardMatrix1`] (falling back to
//! `ColorMatrix1`'s inverse when no forward matrix is present) to convert
//! linear camera-native RGB to linear sRGB via the CIE XYZ (D50) connection
//! space, using the standard published D50→sRGB matrix. When **both**
//! calibration illuminants are present, this averages their two matrices
//! rather than interpolating by the estimated scene color temperature the
//! full DNG spec describes — a deliberate simplification, not a
//! misreading of the spec. `ProfileHueSatMapData`/`ProfileLookTableData`
//! (the 3D hue/saturation/value correction tables) and `ProfileToneCurve`
//! are **neither parsed nor applied**: ADR 0037 scopes the eventual parser
//! to include them, but [`DcpProfile`] currently carries only the profile
//! name and the resolved camera→XYZ matrix, and [`tag_id`] lists only the
//! tags that matrix is built from. A profile whose look is largely carried
//! by those tables will therefore render differently here than in Adobe's
//! own converter — a known gap, not a silently-dropped requirement.
//! **Colorimetric correctness of the matrix path has not been validated
//! against real Adobe-generated `.dcp` files and their reference renders**
//! — the validation bar ADR 0035/0037 themselves set before this can be
//! considered release-ready. Container parsing is well-tested for the tags
//! it does read (round-trips against self-constructed fixtures); the color
//! science is implemented to the documented spec in good faith, not
//! verified against Adobe's own output.
//!
//! [`ColorMatrix1`]: https://helpx.adobe.com/camera-raw/digital-negative.html
//! [`ForwardMatrix1`]: https://helpx.adobe.com/camera-raw/digital-negative.html

use std::io::Cursor;

use tiff::decoder::{Decoder, ifd};
use tiff::tags::Tag;

/// Adobe DNG Specification private tag IDs this module reads. Not a
/// complete DNG/DCP tag set — only what a matrix-based render needs.
mod tag_id {
    pub const CALIBRATION_ILLUMINANT_1: u16 = 50778;
    pub const CALIBRATION_ILLUMINANT_2: u16 = 50779;
    pub const COLOR_MATRIX_1: u16 = 50721;
    pub const COLOR_MATRIX_2: u16 = 50722;
    pub const FORWARD_MATRIX_1: u16 = 50964;
    pub const FORWARD_MATRIX_2: u16 = 50965;
    pub const PROFILE_NAME: u16 = 50936;
}

/// What can go wrong reading a `.dcp` file.
#[derive(Debug, thiserror::Error)]
pub enum DcpError {
    /// The file isn't a valid TIFF/EP container, or a required tag is
    /// missing/malformed.
    #[error("invalid DCP profile: {0}")]
    Invalid(String),
}

type Result<T> = std::result::Result<T, DcpError>;

/// A 3×3 matrix, row-major, as DCP color matrices are stored.
pub type Matrix3 = [[f64; 3]; 3];

/// A parsed camera profile (ADR 0035), ready to convert linear
/// camera-native RGB samples to linear sRGB.
#[derive(Debug, Clone, PartialEq)]
pub struct DcpProfile {
    /// Human-readable profile name, when the file declares one.
    pub name: Option<String>,
    /// The matrix this profile's camera→XYZ(D50) conversion is built from
    /// — already resolved from whichever combination of `ColorMatrix`/
    /// `ForwardMatrix` tags the file declared (see the module doc for the
    /// exact resolution rule).
    camera_to_xyz_d50: Matrix3,
}

impl DcpProfile {
    /// Parses a `.dcp` file's bytes.
    pub fn parse(bytes: &[u8]) -> Result<DcpProfile> {
        let mut decoder = Decoder::new(Cursor::new(bytes))
            .map_err(|e| DcpError::Invalid(format!("not a valid TIFF/EP container: {e}")))?;

        let color_matrix_1 = read_matrix(&mut decoder, tag_id::COLOR_MATRIX_1)?
            .ok_or_else(|| DcpError::Invalid("missing required ColorMatrix1 tag".to_owned()))?;
        let color_matrix_2 = read_matrix(&mut decoder, tag_id::COLOR_MATRIX_2)?;
        let forward_matrix_1 = read_matrix(&mut decoder, tag_id::FORWARD_MATRIX_1)?;
        let forward_matrix_2 = read_matrix(&mut decoder, tag_id::FORWARD_MATRIX_2)?;
        // Read but not yet consumed by `camera_to_xyz_d50`'s resolution —
        // see the module doc's stated simplification (averaged, not
        // CCT-interpolated).
        let _calibration_illuminant_1 = read_u32(&mut decoder, tag_id::CALIBRATION_ILLUMINANT_1);
        let _calibration_illuminant_2 = read_u32(&mut decoder, tag_id::CALIBRATION_ILLUMINANT_2);
        let name = read_ascii(&mut decoder, tag_id::PROFILE_NAME);

        // Prefer the forward matrix/matrices (the DNG spec's documented
        // preference when present); average both illuminants' matrices
        // when both are declared, matrix-invert ColorMatrix1 as the last
        // resort. All three branches are legitimate per-file shapes, not
        // error cases.
        let camera_to_xyz_d50 = match (forward_matrix_1, forward_matrix_2) {
            (Some(a), Some(b)) => average(a, b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => {
                let color_matrix = match color_matrix_2 {
                    Some(cm2) => average(color_matrix_1, cm2),
                    None => color_matrix_1,
                };
                invert(color_matrix)
                    .ok_or_else(|| DcpError::Invalid("ColorMatrix1 is not invertible".to_owned()))?
            }
        };

        Ok(DcpProfile {
            name,
            camera_to_xyz_d50,
        })
    }

    /// Converts one linear camera-native RGB sample (in `[0, 1]`, as
    /// [`leyline_raw::DecodeParams::camera_native`] decodes) to linear
    /// sRGB via this profile's camera→XYZ(D50) matrix and the standard
    /// XYZ(D50)→sRGB matrix. The caller is responsible for the sRGB gamma
    /// encode this doesn't do — matrix work stays in linear light.
    pub fn camera_to_linear_srgb(&self, camera_rgb: [f64; 3]) -> [f64; 3] {
        let xyz = apply(self.camera_to_xyz_d50, camera_rgb);
        apply(XYZ_D50_TO_LINEAR_SRGB, xyz)
    }

    /// Converts one linear camera-native RGB sample into the pipeline's
    /// working space, linear Rec. 2020 (ADR 0044 §1), through this profile's
    /// camera→XYZ(D50) matrix, the Bradford adaptation to D65 and the
    /// standard XYZ→Rec. 2020 matrix.
    ///
    /// Nothing is clipped on the way: a color the working space cannot hold
    /// would come out negative, and it is the *output* stage's business to
    /// decide what happens to it, not this one's.
    pub fn camera_to_linear_rec2020(&self, camera_rgb: [f64; 3]) -> [f64; 3] {
        let xyz_d50 = apply(self.camera_to_xyz_d50, camera_rgb);
        let xyz_d65 = apply(BRADFORD_D50_TO_D65, xyz_d50);
        crate::working_space::apply_matrix(crate::working_space::XYZ_D65_TO_REC2020, xyz_d65)
    }
}

/// The standard XYZ (D50) to linear sRGB matrix (Bruce Lindbloom's
/// widely-published Bradford-adapted conversion) — the same connection
/// space DNG color matrices are defined against.
const XYZ_D50_TO_LINEAR_SRGB: Matrix3 = [
    [3.1338561, -1.6168667, -0.4906146],
    [-0.9787684, 1.9161415, 0.0334540],
    [0.0719453, -0.2289914, 1.4052427],
];

/// Bradford chromatic adaptation from D50 — the illuminant DNG color
/// matrices are defined against — to D65, the working space's white point.
const BRADFORD_D50_TO_D65: Matrix3 = [
    [0.955_576_6, -0.023_039_3, 0.063_163_6],
    [-0.028_289_5, 1.009_941_6, 0.021_007_7],
    [0.012_298_2, -0.020_483_0, 1.329_909_8],
];

/// Applies a 3×3 matrix to a 3-vector.
fn apply(m: Matrix3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Element-wise average of two matrices — the simplified stand-in for
/// CCT-weighted dual-illuminant interpolation (module doc).
fn average(a: Matrix3, b: Matrix3) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = (a[r][c] + b[r][c]) / 2.0;
        }
    }
    out
}

/// 3×3 matrix inverse via the adjugate/determinant, `None` when singular.
pub(crate) fn invert(m: Matrix3) -> Option<Matrix3> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ])
}

/// Reads a 9-entry SRATIONAL tag (a DCP color/forward matrix) as a
/// row-major 3×3, `None` when the tag is absent.
fn read_matrix<R: std::io::Read + std::io::Seek>(
    decoder: &mut Decoder<R>,
    tag: u16,
) -> Result<Option<Matrix3>> {
    let Some(value) = decoder
        .find_tag(Tag::Unknown(tag))
        .map_err(|e| DcpError::Invalid(e.to_string()))?
    else {
        return Ok(None);
    };
    let entries = srational_values(value)?;
    if entries.len() != 9 {
        return Err(DcpError::Invalid(format!(
            "tag {tag} has {} entries, expected 9 (a 3x3 matrix)",
            entries.len()
        )));
    }
    Ok(Some([
        [entries[0], entries[1], entries[2]],
        [entries[3], entries[4], entries[5]],
        [entries[6], entries[7], entries[8]],
    ]))
}

/// Converts a tag's raw value into a flat list of decimal values — DCP
/// matrices are stored as SRATIONAL (signed numerator/denominator) arrays,
/// which the `tiff` crate's own `into_f64_vec` does not convert (it only
/// accepts `DOUBLE`), so this reads the rationals itself.
fn srational_values(value: ifd::Value) -> Result<Vec<f64>> {
    match value {
        ifd::Value::List(items) => items.into_iter().map(srational_scalar).collect(),
        other => Ok(vec![srational_scalar(other)?]),
    }
}

fn srational_scalar(value: ifd::Value) -> Result<f64> {
    match value {
        ifd::Value::SRational(n, d) => Ok(f64::from(n) / f64::from(d)),
        ifd::Value::SRationalBig(n, d) => Ok(n as f64 / d as f64),
        ifd::Value::Rational(n, d) => Ok(f64::from(n) / f64::from(d)),
        ifd::Value::RationalBig(n, d) => Ok(n as f64 / d as f64),
        ifd::Value::Double(v) => Ok(v),
        other => Err(DcpError::Invalid(format!(
            "expected a rational matrix entry, got {other:?}"
        ))),
    }
}

fn read_u32<R: std::io::Read + std::io::Seek>(decoder: &mut Decoder<R>, tag: u16) -> Option<u32> {
    decoder.get_tag_u32(Tag::Unknown(tag)).ok()
}

fn read_ascii<R: std::io::Read + std::io::Seek>(
    decoder: &mut Decoder<R>,
    tag: u16,
) -> Option<String> {
    decoder.get_tag_ascii_string(Tag::Unknown(tag)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiff::encoder::TiffEncoder;
    use tiff::encoder::colortype::Gray8;

    /// Builds a minimal but spec-valid TIFF/EP container: a 1x1 baseline
    /// grayscale image (satisfying the decoder's mandatory-tag checks)
    /// plus whichever DCP tags the caller adds via `extra`.
    fn sample_dcp(
        extra: impl FnOnce(
            &mut tiff::encoder::DirectoryEncoder<
                '_,
                &mut Cursor<Vec<u8>>,
                tiff::encoder::TiffKindStandard,
            >,
        ),
    ) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut encoder = TiffEncoder::new(&mut buffer).unwrap();
            let mut image = encoder.new_image::<Gray8>(1, 1).unwrap();
            extra(image.encoder());
            image.write_data(&[0u8]).unwrap();
        }
        buffer.into_inner()
    }

    fn srational_tag(matrix: Matrix3) -> Vec<tiff::encoder::SRational> {
        matrix
            .into_iter()
            .flatten()
            .map(|v| tiff::encoder::SRational {
                n: (v * 10000.0).round() as i32,
                d: 10000,
            })
            .collect()
    }

    const IDENTITY: Matrix3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    #[test]
    fn parses_a_minimal_profile_with_only_color_matrix_1() {
        let bytes = sample_dcp(|dir| {
            dir.write_tag(
                Tag::Unknown(tag_id::COLOR_MATRIX_1),
                srational_tag(IDENTITY).as_slice(),
            )
            .unwrap();
        });
        let profile = DcpProfile::parse(&bytes).unwrap();
        // Camera->XYZ is ColorMatrix1's inverse; the inverse of the
        // identity is the identity.
        let out = profile.camera_to_linear_srgb([1.0, 0.0, 0.0]);
        let expected = apply(XYZ_D50_TO_LINEAR_SRGB, [1.0, 0.0, 0.0]);
        for (a, b) in out.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn missing_color_matrix_1_is_an_error() {
        let bytes = sample_dcp(|_dir| {});
        assert!(DcpProfile::parse(&bytes).is_err());
    }

    #[test]
    fn reads_the_profile_name_when_present() {
        let bytes = sample_dcp(|dir| {
            dir.write_tag(
                Tag::Unknown(tag_id::COLOR_MATRIX_1),
                srational_tag(IDENTITY).as_slice(),
            )
            .unwrap();
            dir.write_tag(Tag::Unknown(tag_id::PROFILE_NAME), "Test Camera Profile")
                .unwrap();
        });
        let profile = DcpProfile::parse(&bytes).unwrap();
        assert_eq!(profile.name.as_deref(), Some("Test Camera Profile"));
    }

    #[test]
    fn forward_matrix_takes_precedence_over_color_matrix() {
        let scale2 = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]];
        let bytes = sample_dcp(|dir| {
            dir.write_tag(
                Tag::Unknown(tag_id::COLOR_MATRIX_1),
                srational_tag(IDENTITY).as_slice(),
            )
            .unwrap();
            dir.write_tag(
                Tag::Unknown(tag_id::FORWARD_MATRIX_1),
                srational_tag(scale2).as_slice(),
            )
            .unwrap();
        });
        let profile = DcpProfile::parse(&bytes).unwrap();
        let out = profile.camera_to_linear_srgb([1.0, 1.0, 1.0]);
        // ForwardMatrix (scale by 2) then XYZ->sRGB, not ColorMatrix1's
        // (identity) inverse.
        let expected = apply(XYZ_D50_TO_LINEAR_SRGB, [2.0, 2.0, 2.0]);
        for (a, b) in out.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn invert_recovers_the_original_matrix() {
        let m = [[2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [1.0, 0.0, 1.0]];
        let inv = invert(m).unwrap();
        // `inv` undoes `m`: applying both to each standard basis vector
        // must recover that same vector (m * inv = identity), checked via
        // `apply` (already tested on its own) instead of a hand-rolled
        // matrix-multiply loop.
        for basis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
            let round_tripped = apply(m, apply(inv, basis));
            for (actual, expected) in round_tripped.iter().zip(basis.iter()) {
                assert!(
                    (actual - expected).abs() < 1e-9,
                    "m * inv * {basis:?} = {round_tripped:?}"
                );
            }
        }
    }

    #[test]
    fn invert_of_a_singular_matrix_is_none() {
        let singular = [[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [1.0, 1.0, 1.0]];
        assert!(invert(singular).is_none());
    }

    #[test]
    fn average_of_two_matrices_is_elementwise() {
        let a = [[2.0, 2.0, 2.0], [2.0, 2.0, 2.0], [2.0, 2.0, 2.0]];
        let b = [[4.0, 4.0, 4.0], [4.0, 4.0, 4.0], [4.0, 4.0, 4.0]];
        assert_eq!(average(a, b), [[3.0, 3.0, 3.0]; 3]);
    }

    #[test]
    fn not_a_tiff_file_is_rejected() {
        assert!(DcpProfile::parse(b"not a tiff file at all").is_err());
    }
}
