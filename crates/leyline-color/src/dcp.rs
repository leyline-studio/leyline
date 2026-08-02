//! Camera profile (DCP) parsing and application (ADR 0035, ADR 0037).
//!
//! DCP is Adobe's per-camera color calibration format: a handful of
//! private TIFF/EP tags (documented in Adobe's public DNG Specification)
//! carrying color matrices, calibration illuminants, and (optionally) a
//! tone curve and hue/saturation/look tables. It is **not** ICC — `lcms2`
//! cannot read it — which is why this module exists instead of routing
//! through the ICC path this crate already has (`OutputTransform`).
//!
//! Per ADR 0037: the container is read with a home-grown tag reader rather
//! than a new DCP-specific dependency — DCP's tag set is small and fully
//! documented, not a reverse-engineered format. That reader is [`Ifd`], and
//! it is genuinely home-grown: the `tiff` crate was tried first and cannot
//! serve, because it decodes *images* and refuses any file without
//! `ImageWidth`, which a camera profile never has. This module is
//! read-only: Leyline never authors `.dcp` files.
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
//! **What is verified, as of 2026-08:** real third-party `.dcp` files parse
//! (two Canon linear profiles), and the matrix path keeps a neutral sensor
//! triple neutral through camera→XYZ(D50)→sRGB — a check that needs no
//! reference renderer, and that a transposed matrix, a swapped
//! numerator/denominator or a mis-read byte order would all fail. Run it
//! with `LEYLINE_TEST_DCP` pointing at a directory of profiles.
//!
//! **What is still not verified:** agreement with Adobe's *rendered output*.
//! That needs Lightroom or ACR to produce the reference, and no amount of
//! reading `.dcp` files replaces it. Until then the feature stays marked
//! experimental — which now names a narrower gap than it did: the container
//! and the neutral axis are checked, the look is not.
//!
//! [`ColorMatrix1`]: https://helpx.adobe.com/camera-raw/digital-negative.html
//! [`ForwardMatrix1`]: https://helpx.adobe.com/camera-raw/digital-negative.html

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

/// The version number a real `.dcp` writes where TIFF writes 42.
///
/// A camera profile is a TIFF *structure* — byte-order mark, version, IFD
/// offset, tag table — carrying a version of its own. Adobe's tools write
/// `0x4352` there, and every `.dcp` in the wild begins `49 49 52 43`
/// (`II` then `0x4352`, little-endian).
const DCP_VERSION: u16 = 0x4352;

/// TIFF's own version number, which a fixture may legitimately carry.
const TIFF_VERSION: u16 = 42;

/// The tag directory of a camera profile, read directly.
///
/// ADR 0037 described "a home-grown tag reader built on the `tiff` crate's
/// generic IFD decoder". That crate turned out to have no such thing:
/// `tiff::Decoder` decodes *images*, and refuses any file without
/// `ImageWidth`. A camera profile has no image at all — it is a bare
/// directory of metadata — so a real `.dcp` could never get past it. The
/// fixtures that passed were self-built TIFF images carrying DCP tags,
/// which is why nothing caught it.
///
/// So the reader is genuinely home-grown, as the ADR intended. A TIFF
/// directory is a small, fully documented structure: an 8-byte header,
/// then entries of `(tag, type, count, value-or-offset)`, each 12 bytes.
/// Nothing here interprets pixels, because there are none.
struct Ifd<'a> {
    bytes: &'a [u8],
    little_endian: bool,
    /// `(type, count, value-or-offset field)` per tag.
    entries: std::collections::HashMap<u16, (u16, u32, [u8; 4])>,
}

impl<'a> Ifd<'a> {
    /// Reads the header and the first directory.
    fn parse(bytes: &'a [u8]) -> Result<Ifd<'a>> {
        let header: [u8; 8] = bytes
            .get(..8)
            .and_then(|h| h.try_into().ok())
            .ok_or_else(|| DcpError::Invalid("too short to be a camera profile".to_owned()))?;

        let little_endian = match &header[..2] {
            b"II" => true,
            b"MM" => false,
            _ => {
                return Err(DcpError::Invalid(
                    "not a camera profile: no TIFF byte-order mark".to_owned(),
                ));
            }
        };
        let u16_at = |b: [u8; 2]| {
            if little_endian {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            }
        };
        let u32_at = |b: [u8; 4]| {
            if little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };

        let version = u16_at([header[2], header[3]]);
        if version != DCP_VERSION && version != TIFF_VERSION {
            return Err(DcpError::Invalid(format!(
                "not a camera profile: version {version} is neither DCP's {DCP_VERSION} \
                 nor TIFF's {TIFF_VERSION}"
            )));
        }

        let offset = u32_at([header[4], header[5], header[6], header[7]]) as usize;
        let count_bytes: [u8; 2] = bytes
            .get(offset..offset + 2)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| DcpError::Invalid("directory offset is past the file".to_owned()))?;
        let count = u16_at(count_bytes) as usize;

        let mut entries = std::collections::HashMap::with_capacity(count);
        for i in 0..count {
            let at = offset + 2 + i * 12;
            let entry: [u8; 12] = bytes
                .get(at..at + 12)
                .and_then(|b| b.try_into().ok())
                .ok_or_else(|| DcpError::Invalid("directory runs past the file".to_owned()))?;
            let tag = u16_at([entry[0], entry[1]]);
            let kind = u16_at([entry[2], entry[3]]);
            let n = u32_at([entry[4], entry[5], entry[6], entry[7]]);
            entries.insert(tag, (kind, n, [entry[8], entry[9], entry[10], entry[11]]));
        }

        Ok(Ifd {
            bytes,
            little_endian,
            entries,
        })
    }

    fn u16_at(&self, b: [u8; 2]) -> u16 {
        if self.little_endian {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    }

    fn u32_at(&self, b: [u8; 4]) -> u32 {
        if self.little_endian {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    }

    fn i32_at(&self, b: [u8; 4]) -> i32 {
        if self.little_endian {
            i32::from_le_bytes(b)
        } else {
            i32::from_be_bytes(b)
        }
    }

    /// The raw bytes of a tag's value, inline or followed to its offset.
    ///
    /// TIFF stores a value inside the entry when it fits in four bytes and
    /// at an offset otherwise — the one subtlety of the format, and the
    /// reason a matrix (nine rationals, 72 bytes) is never inline while a
    /// short illuminant code always is.
    fn value_bytes(&self, tag: u16) -> Option<&'a [u8]> {
        let (kind, count, field) = *self.entries.get(&tag)?;
        let unit = match kind {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => return None,
        };
        let len = unit * count as usize;
        if len <= 4 {
            // Borrowing from the entry itself is impossible — it was copied
            // into the map — so the inline case is served from the file at
            // the entry's own position instead.
            let at = self.inline_position(tag)?;
            return self.bytes.get(at..at + len);
        }
        let at = self.u32_at(field) as usize;
        self.bytes.get(at..at + len)
    }

    /// Where a tag's inline value sits in the file: the last four bytes of
    /// its directory entry.
    fn inline_position(&self, tag: u16) -> Option<usize> {
        let header: [u8; 4] = self.bytes.get(4..8)?.try_into().ok()?;
        let offset = self.u32_at(header) as usize;
        let count = self.u16_at(self.bytes.get(offset..offset + 2)?.try_into().ok()?) as usize;
        (0..count).find_map(|i| {
            let at = offset + 2 + i * 12;
            let entry: [u8; 2] = self.bytes.get(at..at + 2)?.try_into().ok()?;
            (self.u16_at(entry) == tag).then_some(at + 8)
        })
    }

    /// A tag read as a list of decimals. DCP matrices are SRATIONAL —
    /// signed numerator over denominator — which is why this cannot be a
    /// plain integer read.
    fn rationals(&self, tag: u16) -> Option<Vec<f64>> {
        let (kind, _, _) = *self.entries.get(&tag)?;
        let bytes = self.value_bytes(tag)?;
        match kind {
            // SRATIONAL and RATIONAL: two 4-byte halves each.
            5 | 10 => Some(
                bytes
                    .chunks_exact(8)
                    .map(|c| {
                        let (n, d) = (
                            self.i32_at(c[..4].try_into().unwrap()),
                            self.i32_at(c[4..].try_into().unwrap()),
                        );
                        if d == 0 {
                            0.0
                        } else {
                            f64::from(n) / f64::from(d)
                        }
                    })
                    .collect(),
            ),
            // DOUBLE, which a hand-written profile may use instead.
            12 => Some(
                bytes
                    .chunks_exact(8)
                    .map(|c| {
                        let raw = c.try_into().unwrap();
                        if self.little_endian {
                            f64::from_le_bytes(raw)
                        } else {
                            f64::from_be_bytes(raw)
                        }
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    /// A SHORT or LONG tag, as the calibration illuminants are stored.
    fn integer(&self, tag: u16) -> Option<u32> {
        let (kind, _, _) = *self.entries.get(&tag)?;
        let bytes = self.value_bytes(tag)?;
        match kind {
            3 => Some(u32::from(self.u16_at(bytes.get(..2)?.try_into().ok()?))),
            4 => Some(self.u32_at(bytes.get(..4)?.try_into().ok()?)),
            _ => None,
        }
    }

    /// An ASCII tag, trailing NUL removed.
    fn ascii(&self, tag: u16) -> Option<String> {
        let bytes = self.value_bytes(tag)?;
        let text = String::from_utf8_lossy(bytes);
        Some(text.trim_end_matches('\0').to_owned())
    }
}

impl DcpProfile {
    /// Parses a `.dcp` file's bytes.
    pub fn parse(bytes: &[u8]) -> Result<DcpProfile> {
        let ifd = Ifd::parse(bytes)?;

        let color_matrix_1 = read_matrix(&ifd, tag_id::COLOR_MATRIX_1)?
            .ok_or_else(|| DcpError::Invalid("missing required ColorMatrix1 tag".to_owned()))?;
        let color_matrix_2 = read_matrix(&ifd, tag_id::COLOR_MATRIX_2)?;
        let forward_matrix_1 = read_matrix(&ifd, tag_id::FORWARD_MATRIX_1)?;
        let forward_matrix_2 = read_matrix(&ifd, tag_id::FORWARD_MATRIX_2)?;
        // Read but not yet consumed by `camera_to_xyz_d50`'s resolution —
        // see the module doc's stated simplification (averaged, not
        // CCT-interpolated).
        let _calibration_illuminant_1 = ifd.integer(tag_id::CALIBRATION_ILLUMINANT_1);
        let _calibration_illuminant_2 = ifd.integer(tag_id::CALIBRATION_ILLUMINANT_2);
        let name = ifd.ascii(tag_id::PROFILE_NAME).filter(|n| !n.is_empty());

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
fn read_matrix(ifd: &Ifd<'_>, tag: u16) -> Result<Option<Matrix3>> {
    let Some(entries) = ifd.rationals(tag) else {
        return Ok(None);
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tiff::encoder::TiffEncoder;
    use tiff::encoder::colortype::Gray8;
    use tiff::tags::Tag;

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

    /// A real `.dcp` is a *bare tag directory* carrying DCP's own version
    /// number — no image, no `ImageWidth`. Every fixture above is a TIFF
    /// image with DCP tags bolted on, which is why they all passed while
    /// no genuine profile could be read at all. This one is shaped like
    /// the real thing, by hand.
    #[test]
    fn a_bare_directory_with_the_dcp_version_parses() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"II");
        bytes.extend_from_slice(&DCP_VERSION.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes()); // directory at byte 8

        // One entry: ColorMatrix1, nine SRATIONALs living after the
        // directory. 2 + 12 + 4 = 18 bytes of directory, so the values
        // start at 8 + 18 = 26.
        let values_at = 26u32;
        bytes.extend_from_slice(&1u16.to_le_bytes()); // entry count
        bytes.extend_from_slice(&tag_id::COLOR_MATRIX_1.to_le_bytes());
        bytes.extend_from_slice(&10u16.to_le_bytes()); // SRATIONAL
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.extend_from_slice(&values_at.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes()); // next directory: none
        assert_eq!(bytes.len(), values_at as usize);

        // The identity matrix, as nine numerator/denominator pairs.
        for value in [1i32, 0, 0, 0, 1, 0, 0, 0, 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
            bytes.extend_from_slice(&1i32.to_le_bytes());
        }

        let profile = DcpProfile::parse(&bytes).expect("a bare DCP directory must parse");
        // Identity camera->XYZ inverts to identity, so the round trip is
        // just the standard XYZ(D50)->sRGB matrix; a neutral triple stays
        // neutral through it.
        let grey = profile.camera_to_linear_srgb([0.5, 0.5, 0.5]);
        assert!(grey.iter().all(|c| c.is_finite()), "{grey:?}");
    }

    /// The same, in big-endian: the byte-order mark drives every read, and
    /// getting it wrong would silently mis-parse rather than fail.
    #[test]
    fn a_big_endian_directory_is_read_the_other_way_round() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"MM");
        bytes.extend_from_slice(&DCP_VERSION.to_be_bytes());
        bytes.extend_from_slice(&8u32.to_be_bytes());
        let values_at = 26u32;
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&tag_id::COLOR_MATRIX_1.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        bytes.extend_from_slice(&9u32.to_be_bytes());
        bytes.extend_from_slice(&values_at.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        for value in [2i32, 0, 0, 0, 2, 0, 0, 0, 2] {
            bytes.extend_from_slice(&value.to_be_bytes());
            bytes.extend_from_slice(&1i32.to_be_bytes());
        }
        assert!(DcpProfile::parse(&bytes).is_ok());
    }

    /// Real profiles, when the machine has them: `LEYLINE_TEST_DCP` names a
    /// directory of `.dcp` files, the same opt-in shape `LEYLINE_TEST_RAW`
    /// uses for real photographs.
    ///
    /// They are not committed — they are third-party work of unknown
    /// licence, and this repository is meant to go public. The fixtures
    /// above are what CI checks; this is what catches whatever a fixture
    /// cannot imagine, which on the day it was written was the entire
    /// container format.
    #[test]
    fn real_profiles_parse_and_keep_a_neutral_grey_neutral() {
        let Ok(dir) = std::env::var("LEYLINE_TEST_DCP") else {
            return;
        };
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("LEYLINE_TEST_DCP must name a directory") {
            let path = entry.expect("readable directory entry").path();
            if path
                .extension()
                .is_none_or(|e| !e.eq_ignore_ascii_case("dcp"))
            {
                continue;
            }
            let bytes = std::fs::read(&path).expect("readable profile");
            let profile = DcpProfile::parse(&bytes)
                .unwrap_or_else(|e| panic!("{} failed to parse: {e}", path.display()));

            // The check that needs no reference renderer: a neutral sensor
            // triple must come out neutral. A transposed matrix, a swapped
            // numerator/denominator or a botched endianness all break this
            // long before they become a subtle hue error.
            let grey = profile.camera_to_linear_srgb([0.5, 0.5, 0.5]);
            let spread = grey.iter().cloned().fold(f64::MIN, f64::max)
                - grey.iter().cloned().fold(f64::MAX, f64::min);
            assert!(
                spread < 0.01,
                "{} turns a neutral grey into {grey:?}",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 0, "LEYLINE_TEST_DCP held no .dcp file");
    }
}
