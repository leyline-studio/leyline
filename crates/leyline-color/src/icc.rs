//! Reading what a file says its colour is (ADR 0115).
//!
//! An RGB image written by a camera, a phone or an editor carries a
//! **matrix/TRC** ICC profile: three primary colorants and one tone curve.
//! That is all a colour space is, and this module reads exactly that — the
//! `rXYZ`/`gXYZ`/`bXYZ` colorants and the `rTRC` curve — and turns them into
//! the matrix and transfer function the `input` stage needs.
//!
//! **LittleCMS is deliberately not used here** (ADR 0115 §3). It stays at the
//! output, where export and soft proofing use it; putting it inside the
//! develop pipeline would make every future golden depend on its version, as
//! [ADR 0086](../../../docs/adr/0086-decoder-in-the-promise.md) had to accept
//! for the RAW decoder. One such dependency is the price of reading RAW
//! files; a second, for a matrix multiplication, is not.
//!
//! A profile this module cannot reduce — LUT-based, CMYK, anything that is
//! not three colorants and a curve — returns `None`, and the caller treats
//! the file as sRGB, which is what it did for every file before this ADR.

use crate::Matrix3;
use crate::working_space::{XYZ_D65_TO_REC2020, apply_matrix};

/// How a source's numbers encode light.
#[derive(Debug, Clone, PartialEq)]
pub enum Transfer {
    /// The sRGB piecewise curve — by far the most common, and the one an
    /// untagged file is assumed to use.
    Srgb,
    /// A plain exponent.
    Gamma(f64),
    /// The ICC parametric forms (types 0–4), which cover sRGB-shaped curves
    /// with their own coefficients.
    Parametric {
        /// Exponent.
        g: f64,
        /// Slope of the exponential segment.
        a: f64,
        /// Offset inside the exponential segment.
        b: f64,
        /// Slope of the linear segment.
        c: f64,
        /// Where the two segments meet.
        d: f64,
        /// Offset of the exponential segment.
        e: f64,
        /// Offset of the linear segment.
        f: f64,
    },
    /// A sampled curve, encoded value to linear, at the profile's own
    /// resolution.
    Table(Vec<f32>),
}

impl Transfer {
    /// Decodes one sample to linear light. Values outside `[0, 1]` are
    /// carried through the same formula rather than clamped: the working
    /// space is unbounded above (ADR 0044) and nothing here is the place to
    /// decide otherwise.
    pub fn to_linear(&self, v: f32) -> f32 {
        match self {
            Transfer::Srgb => {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            }
            Transfer::Gamma(g) => {
                if v <= 0.0 {
                    v
                } else {
                    v.powf(*g as f32)
                }
            }
            Transfer::Parametric {
                g,
                a,
                b,
                c,
                d,
                e,
                f,
            } => {
                let (g, a, b, c, d, e, f) = (
                    *g as f32, *a as f32, *b as f32, *c as f32, *d as f32, *e as f32, *f as f32,
                );
                if v >= d {
                    let base = a * v + b;
                    if base <= 0.0 { e } else { base.powf(g) + e }
                } else {
                    c * v + f
                }
            }
            Transfer::Table(table) => sample(table, v),
        }
    }
}

/// Linear interpolation into a sampled curve, clamped at both ends.
fn sample(table: &[f32], v: f32) -> f32 {
    if table.is_empty() {
        return v;
    }
    if table.len() == 1 {
        return table[0];
    }
    let last = table.len() - 1;
    let position = (v.clamp(0.0, 1.0) * last as f32).clamp(0.0, last as f32);
    let index = position.floor() as usize;
    if index >= last {
        return table[last];
    }
    let t = position - index as f32;
    table[index] * (1.0 - t) + table[index + 1] * t
}

/// What a tagged file says it is: where its primaries sit, and how its
/// numbers encode light.
#[derive(Debug, Clone, PartialEq)]
pub struct TaggedSource {
    /// The source's linear RGB → linear Rec. 2020 (D65) matrix — the working
    /// space, in **one** rotation rather than a detour through sRGB, which
    /// would clip everything outside it (ADR 0115 §2).
    pub to_rec2020: Matrix3,
    /// How to linearize the source's samples first.
    pub transfer: Transfer,
}

/// XYZ (D50) → linear Rec. 2020, as a matrix.
///
/// Built from the already-tested conversion by feeding it the three basis
/// vectors: the columns of a linear map *are* its images of the basis, so
/// this is the same arithmetic the DCP path uses, not a second copy of it.
fn xyz_d50_to_rec2020() -> Matrix3 {
    let mut matrix = [[0.0f64; 3]; 3];
    for axis in 0..3 {
        let mut basis = [0.0f64; 3];
        basis[axis] = 1.0;
        let mapped = crate::dcp::xyz_d50_to_linear_rec2020(basis);
        for row in 0..3 {
            matrix[row][axis] = mapped[row];
        }
    }
    matrix
}

/// Composes `a ∘ b` — apply `b` first.
fn compose(a: Matrix3, b: Matrix3) -> Matrix3 {
    let mut out = [[0.0f64; 3]; 3];
    for (row, cells) in out.iter_mut().enumerate() {
        for (column, cell) in cells.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[row][k] * b[k][column]).sum();
        }
    }
    out
}

/// Reads a matrix/TRC RGB profile.
///
/// `None` for anything else — a LUT profile, a CMYK profile, a truncated
/// blob — which the caller reads as "this file says nothing I can use".
pub fn read_rgb_profile(icc: &[u8]) -> Option<TaggedSource> {
    let tags = tag_table(icc)?;
    let red = colorant(icc, &tags, b"rXYZ")?;
    let green = colorant(icc, &tags, b"gXYZ")?;
    let blue = colorant(icc, &tags, b"bXYZ")?;
    // The colorants are the columns of RGB → XYZ, already adapted to the D50
    // connection space by whoever wrote the profile: that is what makes an
    // ICC matrix profile self-contained.
    let to_xyz_d50 = [
        [red[0], green[0], blue[0]],
        [red[1], green[1], blue[1]],
        [red[2], green[2], blue[2]],
    ];
    let transfer = curve(icc, &tags, b"rTRC")?;
    Some(TaggedSource {
        to_rec2020: compose(xyz_d50_to_rec2020(), to_xyz_d50),
        transfer,
    })
}

/// Builds the same thing from chromaticities and a white point — what HEIF's
/// `nclx` box gives, with no ICC anywhere (ADR 0115 §1).
///
/// Only a **D65** white is accepted: the three primary sets that occur in
/// practice (BT.709, BT.2020, Display P3) all use it, and adapting an
/// arbitrary white here would add a chromatic adaptation this ADR does not
/// need. Anything else returns `None`, and the file is read as sRGB.
pub fn from_chromaticities(
    primaries: [[f64; 2]; 3],
    white: [f64; 2],
    transfer: Transfer,
) -> Option<TaggedSource> {
    const D65: [f64; 2] = [0.3127, 0.3290];
    if (white[0] - D65[0]).abs() > 0.002 || (white[1] - D65[1]).abs() > 0.002 {
        return None;
    }
    // The standard construction: primaries as XYZ columns, scaled so that
    // RGB (1, 1, 1) lands exactly on the white point.
    let xyz_of = |xy: [f64; 2]| -> [f64; 3] {
        if xy[1].abs() < 1e-9 {
            return [0.0, 0.0, 0.0];
        }
        [xy[0] / xy[1], 1.0, (1.0 - xy[0] - xy[1]) / xy[1]]
    };
    let (r, g, b) = (
        xyz_of(primaries[0]),
        xyz_of(primaries[1]),
        xyz_of(primaries[2]),
    );
    let m = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let white_xyz = xyz_of(white);
    let scale = apply_matrix(crate::dcp::invert(m)?, white_xyz);
    let to_xyz_d65 = [
        [m[0][0] * scale[0], m[0][1] * scale[1], m[0][2] * scale[2]],
        [m[1][0] * scale[0], m[1][1] * scale[1], m[1][2] * scale[2]],
        [m[2][0] * scale[0], m[2][1] * scale[1], m[2][2] * scale[2]],
    ];
    Some(TaggedSource {
        to_rec2020: compose(XYZ_D65_TO_REC2020, to_xyz_d65),
        transfer,
    })
}

/// `(signature, offset, size)` for every tag in the profile.
fn tag_table(icc: &[u8]) -> Option<Vec<([u8; 4], usize, usize)>> {
    // 128-byte header, then a tag count, then 12 bytes per tag.
    if icc.len() < 132 {
        return None;
    }
    let count = be_u32(icc, 128)? as usize;
    // A profile with a thousand tags is not one this reader is looking at.
    if count > 1024 || icc.len() < 132 + count * 12 {
        return None;
    }
    let mut tags = Vec::with_capacity(count);
    for i in 0..count {
        let at = 132 + i * 12;
        let signature = [icc[at], icc[at + 1], icc[at + 2], icc[at + 3]];
        let offset = be_u32(icc, at + 4)? as usize;
        let size = be_u32(icc, at + 8)? as usize;
        if offset.checked_add(size)? > icc.len() {
            return None;
        }
        tags.push((signature, offset, size));
    }
    Some(tags)
}

fn find(tags: &[([u8; 4], usize, usize)], want: &[u8; 4]) -> Option<(usize, usize)> {
    tags.iter()
        .find(|(signature, _, _)| signature == want)
        .map(|(_, offset, size)| (*offset, *size))
}

/// One `XYZ ` tag, as three doubles.
fn colorant(icc: &[u8], tags: &[([u8; 4], usize, usize)], want: &[u8; 4]) -> Option<[f64; 3]> {
    let (offset, size) = find(tags, want)?;
    if size < 20 || &icc[offset..offset + 4] != b"XYZ " {
        return None;
    }
    Some([
        s15fixed16(icc, offset + 8)?,
        s15fixed16(icc, offset + 12)?,
        s15fixed16(icc, offset + 16)?,
    ])
}

/// One `curv` or `para` tag, as a [`Transfer`].
fn curve(icc: &[u8], tags: &[([u8; 4], usize, usize)], want: &[u8; 4]) -> Option<Transfer> {
    let (offset, size) = find(tags, want)?;
    if size < 12 {
        return None;
    }
    match &icc[offset..offset + 4] {
        b"curv" => {
            let count = be_u32(icc, offset + 8)? as usize;
            match count {
                // No points: the identity, i.e. the samples are already
                // linear light.
                0 => Some(Transfer::Gamma(1.0)),
                // One point: a plain exponent, u8Fixed8.
                1 => {
                    let raw = be_u16(icc, offset + 12)?;
                    Some(Transfer::Gamma(f64::from(raw) / 256.0))
                }
                _ => {
                    if size < 12 + count * 2 {
                        return None;
                    }
                    let mut table = Vec::with_capacity(count);
                    for i in 0..count {
                        table.push(f32::from(be_u16(icc, offset + 12 + i * 2)?) / 65535.0);
                    }
                    Some(Transfer::Table(table))
                }
            }
        }
        b"para" => {
            let kind = be_u16(icc, offset + 8)?;
            let parameter = |i: usize| s15fixed16(icc, offset + 12 + i * 4);
            let g = parameter(0)?;
            Some(match kind {
                0 => Transfer::Gamma(g),
                1 => {
                    let (a, b) = (parameter(1)?, parameter(2)?);
                    Transfer::Parametric {
                        g,
                        a,
                        b,
                        c: 0.0,
                        d: if a.abs() < 1e-12 { 0.0 } else { -b / a },
                        e: 0.0,
                        f: 0.0,
                    }
                }
                2 => {
                    let (a, b, c) = (parameter(1)?, parameter(2)?, parameter(3)?);
                    Transfer::Parametric {
                        g,
                        a,
                        b,
                        c: 0.0,
                        d: if a.abs() < 1e-12 { 0.0 } else { -b / a },
                        e: c,
                        f: c,
                    }
                }
                3 => {
                    let (a, b, c, d) = (parameter(1)?, parameter(2)?, parameter(3)?, parameter(4)?);
                    Transfer::Parametric {
                        g,
                        a,
                        b,
                        c,
                        d,
                        e: 0.0,
                        f: 0.0,
                    }
                }
                4 => Transfer::Parametric {
                    g,
                    a: parameter(1)?,
                    b: parameter(2)?,
                    c: parameter(3)?,
                    d: parameter(4)?,
                    e: parameter(5)?,
                    f: parameter(6)?,
                },
                _ => return None,
            })
        }
        _ => None,
    }
}

fn be_u16(icc: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(icc.get(at..at + 2)?.try_into().ok()?))
}

fn be_u32(icc: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(icc.get(at..at + 4)?.try_into().ok()?))
}

/// ICC's fixed-point number: a signed 15.16.
fn s15fixed16(icc: &[u8], at: usize) -> Option<f64> {
    let raw = i32::from_be_bytes(icc.get(at..at + 4)?.try_into().ok()?);
    Some(f64::from(raw) / 65536.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Display P3: P3 primaries, D65 white, the sRGB curve — what a phone
    /// tags its photographs with.
    fn display_p3_icc() -> Vec<u8> {
        let white = lcms2::CIExyY {
            x: 0.3127,
            y: 0.3290,
            Y: 1.0,
        };
        let primaries = lcms2::CIExyYTRIPLE {
            Red: lcms2::CIExyY {
                x: 0.680,
                y: 0.320,
                Y: 1.0,
            },
            Green: lcms2::CIExyY {
                x: 0.265,
                y: 0.690,
                Y: 1.0,
            },
            Blue: lcms2::CIExyY {
                x: 0.150,
                y: 0.060,
                Y: 1.0,
            },
        };
        // The sRGB curve, as its ICC parametric form.
        let curve = lcms2::ToneCurve::new_parametric(
            4,
            &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045],
        )
        .expect("the sRGB parametric curve is well formed");
        lcms2::Profile::new_rgb(&white, &primaries, &[&curve, &curve, &curve])
            .expect("a matrix/TRC profile")
            .icc()
            .expect("serializes")
    }

    /// The reader against **LittleCMS on the same profile**: the reference
    /// implementation is used to check the arithmetic without being used to
    /// perform it (ADR 0115 §3).
    #[test]
    fn a_display_p3_profile_reads_the_way_littlecms_transforms_it() {
        let icc = display_p3_icc();
        let read = read_rgb_profile(&icc).expect("a matrix/TRC profile is readable");

        // LittleCMS: P3 → linear Rec. 2020, through its own machinery.
        let source = lcms2::Profile::new_icc(&icc).unwrap();
        let linear = lcms2::ToneCurve::new(1.0);
        let rec2020 = lcms2::Profile::new_rgb(
            &lcms2::CIExyY {
                x: 0.3127,
                y: 0.3290,
                Y: 1.0,
            },
            &lcms2::CIExyYTRIPLE {
                Red: lcms2::CIExyY {
                    x: 0.708,
                    y: 0.292,
                    Y: 1.0,
                },
                Green: lcms2::CIExyY {
                    x: 0.170,
                    y: 0.797,
                    Y: 1.0,
                },
                Blue: lcms2::CIExyY {
                    x: 0.131,
                    y: 0.046,
                    Y: 1.0,
                },
            },
            &[&linear, &linear, &linear],
        )
        .unwrap();
        let transform: lcms2::Transform<[f32; 3], [f32; 3]> = lcms2::Transform::new(
            &source,
            lcms2::PixelFormat::RGB_FLT,
            &rec2020,
            lcms2::PixelFormat::RGB_FLT,
            lcms2::Intent::RelativeColorimetric,
        )
        .unwrap();

        for encoded in [
            [1.0f32, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.5, 0.25, 0.75],
            [1.0, 1.0, 1.0],
        ] {
            let mut reference = [[0.0f32; 3]];
            transform.transform_pixels(&[encoded], &mut reference);

            let linearized = encoded.map(|v| f64::from(read.transfer.to_linear(v)));
            let ours = apply_matrix(read.to_rec2020, linearized);

            for channel in 0..3 {
                assert!(
                    (ours[channel] - f64::from(reference[0][channel])).abs() < 0.002,
                    "{encoded:?}: ours {ours:?} vs LittleCMS {:?}",
                    reference[0]
                );
            }
        }
    }

    /// The nclx path has to agree with the ICC path on the same colour
    /// space, or one of the two is wrong.
    #[test]
    fn nclx_p3_agrees_with_the_p3_profile() {
        let from_icc = read_rgb_profile(&display_p3_icc()).unwrap();
        let from_nclx = from_chromaticities(
            [[0.680, 0.320], [0.265, 0.690], [0.150, 0.060]],
            [0.3127, 0.3290],
            Transfer::Srgb,
        )
        .unwrap();
        for row in 0..3 {
            for column in 0..3 {
                assert!(
                    (from_icc.to_rec2020[row][column] - from_nclx.to_rec2020[row][column]).abs()
                        < 0.005,
                    "{:?} vs {:?}",
                    from_icc.to_rec2020,
                    from_nclx.to_rec2020
                );
            }
        }
    }

    #[test]
    fn a_white_that_is_not_d65_is_refused_rather_than_adapted() {
        assert!(
            from_chromaticities(
                [[0.640, 0.330], [0.300, 0.600], [0.150, 0.060]],
                [0.3457, 0.3585],
                Transfer::Srgb,
            )
            .is_none(),
            "a D50 white needs an adaptation this reader does not do"
        );
    }

    #[test]
    fn anything_that_is_not_a_matrix_trc_profile_reads_as_nothing() {
        assert!(read_rgb_profile(&[]).is_none());
        assert!(read_rgb_profile(&[0u8; 200]).is_none());
        // A truncated but plausible header.
        let mut icc = crate::srgb_icc_profile().to_vec();
        icc.truncate(140);
        assert!(read_rgb_profile(&icc).is_none());
    }

    /// And the profile every untagged file is assumed to have reads as the
    /// identity into the working space, give or take rounding.
    #[test]
    fn the_srgb_profile_reads_as_srgb() {
        let read = read_rgb_profile(crate::srgb_icc_profile()).expect("sRGB is matrix/TRC");
        let ours = apply_matrix(read.to_rec2020, [1.0, 1.0, 1.0]);
        for channel in ours {
            assert!((channel - 1.0).abs() < 0.005, "white stays white: {ours:?}");
        }
        assert!((read.transfer.to_linear(0.5) - 0.2140).abs() < 0.01);
    }
}
