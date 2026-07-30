//! `.cube` LUT reader (ADR 0053 §5).
//!
//! Read-only, no dependency: a `.cube` file is a header of a few directives
//! followed by RGB triplets, one per lattice point, and reading it is the same
//! kind of bounded problem the DCP reader solves next door
//! ([ADR 0037](../../../docs/adr/0037-dcp-parsing-dependency.md)).
//!
//! What this module does **not** decide is where the LUT applies. The values in
//! a `.cube` are display-referred, and putting the pipeline on that axis before
//! sampling is the render stage's business (ADR 0053 §3) — this module only
//! answers "what does this LUT map that triplet to".

use std::path::Path;

/// Errors from reading or interpolating a `.cube` LUT.
#[derive(Debug, thiserror::Error)]
pub enum LutError {
    /// The file could not be read.
    #[error("cannot read LUT {path}: {source}")]
    Read {
        /// The file that could not be read.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The file is not a `.cube` this reader can use, with the reason named.
    #[error("invalid .cube LUT: {0}")]
    Invalid(String),
}

/// The largest lattice size accepted per axis.
///
/// A 3D LUT costs `size³` triplets: 128 is already 6 million samples (24 MB),
/// past anything a real look ships as, and the bound is what keeps a malformed
/// header from asking for gigabytes.
const MAX_SIZE: usize = 128;

/// A parsed `.cube` LUT, ready to sample (ADR 0053).
#[derive(Debug, Clone, PartialEq)]
pub struct CubeLut {
    /// Lattice points per axis.
    size: usize,
    /// `size³` triplets for a 3D LUT, `size` for a 1D one, in the file's own
    /// order (red fastest).
    data: Vec<[f32; 3]>,
    /// Input domain, from `DOMAIN_MIN`/`DOMAIN_MAX`; `[0, 1]` by default.
    domain: ([f32; 3], [f32; 3]),
    /// Whether the table is one-dimensional (one curve applied per channel)
    /// rather than a cube.
    one_dimensional: bool,
}

impl CubeLut {
    /// Reads a `.cube` file from disk.
    pub fn load(path: &Path) -> Result<CubeLut, LutError> {
        let text = std::fs::read_to_string(path).map_err(|source| LutError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text)
    }

    /// Parses `.cube` text.
    ///
    /// Accepts what the format actually contains in the wild: `TITLE`,
    /// `LUT_3D_SIZE` or `LUT_1D_SIZE`, `DOMAIN_MIN`/`DOMAIN_MAX`, `#`
    /// comments, blank lines, and the triplets themselves. Anything else is an
    /// error naming what was wrong, never a table filled in by guesswork
    /// (ADR 0053 §5).
    pub fn parse(text: &str) -> Result<CubeLut, LutError> {
        let mut size = None;
        let mut one_dimensional = false;
        let mut domain_min = [0.0f32; 3];
        let mut domain_max = [1.0f32; 3];
        let mut data: Vec<[f32; 3]> = Vec::new();

        for (number, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut fields = line.split_whitespace();
            let keyword = fields.next().unwrap_or("");
            match keyword {
                "TITLE" => {}
                "LUT_3D_SIZE" | "LUT_1D_SIZE" => {
                    if size.is_some() {
                        return Err(LutError::Invalid(format!(
                            "line {}: the LUT size is declared twice",
                            number + 1
                        )));
                    }
                    let declared: usize = fields
                        .next()
                        .and_then(|value| value.parse().ok())
                        .ok_or_else(|| {
                            LutError::Invalid(format!("line {}: unreadable size", number + 1))
                        })?;
                    if !(2..=MAX_SIZE).contains(&declared) {
                        return Err(LutError::Invalid(format!(
                            "line {}: size {declared} is outside [2, {MAX_SIZE}]",
                            number + 1
                        )));
                    }
                    one_dimensional = keyword == "LUT_1D_SIZE";
                    size = Some(declared);
                }
                "DOMAIN_MIN" | "DOMAIN_MAX" => {
                    let values = triplet(&mut fields).ok_or_else(|| {
                        LutError::Invalid(format!(
                            "line {}: {keyword} needs three numbers",
                            number + 1
                        ))
                    })?;
                    if keyword == "DOMAIN_MIN" {
                        domain_min = values;
                    } else {
                        domain_max = values;
                    }
                }
                // A data line: the keyword was already the first number.
                _ => {
                    let mut all = line.split_whitespace();
                    let values = triplet(&mut all).ok_or_else(|| {
                        LutError::Invalid(format!(
                            "line {}: expected three numbers or a known keyword, got {line:?}",
                            number + 1
                        ))
                    })?;
                    data.push(values);
                }
            }
        }

        let size = size.ok_or_else(|| {
            LutError::Invalid("no LUT_3D_SIZE or LUT_1D_SIZE declared".to_owned())
        })?;
        for (axis, (min, max)) in domain_min.iter().zip(&domain_max).enumerate() {
            if !(min.is_finite() && max.is_finite()) || max <= min {
                return Err(LutError::Invalid(format!(
                    "domain on axis {axis} must satisfy min < max, got {min}..{max}"
                )));
            }
        }
        let expected = if one_dimensional {
            size
        } else {
            size * size * size
        };
        if data.len() != expected {
            return Err(LutError::Invalid(format!(
                "expected {expected} entries for a size-{size} {}D LUT, got {}",
                if one_dimensional { 1 } else { 3 },
                data.len()
            )));
        }
        Ok(CubeLut {
            size,
            data,
            domain: (domain_min, domain_max),
            one_dimensional,
        })
    }

    /// Maps one display-referred triplet through the LUT.
    ///
    /// Trilinear for a cube, linear per channel for a 1D table (ADR 0053 §5).
    /// Inputs outside the declared domain are clamped to it: a LUT says nothing
    /// about what lies beyond its own lattice, and extrapolating would invent a
    /// look its author never described.
    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        let (min, max) = self.domain;
        let last = (self.size - 1) as f32;
        // Position along each axis, in lattice units.
        let mut position = [0.0f32; 3];
        for (axis, value) in rgb.iter().enumerate() {
            let normalized = (value - min[axis]) / (max[axis] - min[axis]);
            position[axis] = normalized.clamp(0.0, 1.0) * last;
        }

        if self.one_dimensional {
            let mut out = [0.0f32; 3];
            for (axis, out) in out.iter_mut().enumerate() {
                let low = position[axis].floor() as usize;
                let high = (low + 1).min(self.size - 1);
                let fraction = position[axis] - low as f32;
                *out = self.data[low][axis] * (1.0 - fraction) + self.data[high][axis] * fraction;
            }
            return out;
        }

        // Trilinear: the eight lattice corners around the sample, weighted by
        // the fractional position inside their cell.
        let low = position.map(|p| p.floor() as usize);
        let high = low.map(|l| (l + 1).min(self.size - 1));
        let fraction = [
            position[0] - low[0] as f32,
            position[1] - low[1] as f32,
            position[2] - low[2] as f32,
        ];
        let mut out = [0.0f32; 3];
        for corner in 0..8 {
            let (r, wr) = if corner & 1 == 0 {
                (low[0], 1.0 - fraction[0])
            } else {
                (high[0], fraction[0])
            };
            let (g, wg) = if corner & 2 == 0 {
                (low[1], 1.0 - fraction[1])
            } else {
                (high[1], fraction[1])
            };
            let (b, wb) = if corner & 4 == 0 {
                (low[2], 1.0 - fraction[2])
            } else {
                (high[2], fraction[2])
            };
            let weight = wr * wg * wb;
            if weight == 0.0 {
                continue;
            }
            // `.cube` stores red fastest, then green, then blue.
            let entry = self.data[r + self.size * (g + self.size * b)];
            for (out, value) in out.iter_mut().zip(entry) {
                *out += value * weight;
            }
        }
        out
    }
}

/// Reads three whitespace-separated finite numbers.
fn triplet<'a>(fields: &mut impl Iterator<Item = &'a str>) -> Option<[f32; 3]> {
    let mut values = [0.0f32; 3];
    for value in values.iter_mut() {
        *value = fields
            .next()?
            .parse()
            .ok()
            .filter(|v: &f32| v.is_finite())?;
    }
    // A fourth number would mean the line is not what this reader thinks.
    if fields.next().is_some() {
        return None;
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×2×2 identity cube, written the way a real file is.
    const IDENTITY: &str = "\
# a comment
TITLE \"identity\"
LUT_3D_SIZE 2

0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";

    #[test]
    fn an_identity_cube_returns_what_it_is_given() {
        let lut = CubeLut::parse(IDENTITY).unwrap();
        for input in [
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.25, 0.5, 0.75],
            [0.9, 0.1, 0.4],
        ] {
            let out = lut.sample(input);
            for (a, b) in out.iter().zip(&input) {
                assert!((a - b).abs() < 1e-5, "{out:?} should be {input:?}");
            }
        }
    }

    #[test]
    fn a_cube_that_swaps_channels_is_sampled_in_the_files_own_order() {
        // Red fastest: entry index = r + size * (g + size * b). This LUT maps
        // every corner to (b, r, g), which only comes out right if the index
        // order above is the one used.
        let mut text = String::from("LUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    text.push_str(&format!("{} {} {}\n", b, r, g));
                }
            }
        }
        let lut = CubeLut::parse(&text).unwrap();
        let out = lut.sample([1.0, 0.0, 0.0]);
        assert_eq!(out, [0.0, 1.0, 0.0]);
        let out = lut.sample([0.0, 0.0, 1.0]);
        assert_eq!(out, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_1d_lut_applies_one_curve_per_channel() {
        // Inverting curve, three points.
        let lut = CubeLut::parse("LUT_1D_SIZE 3\n1.0 1.0 1.0\n0.5 0.5 0.5\n0.0 0.0 0.0\n").unwrap();
        assert_eq!(lut.sample([0.0, 0.0, 0.0]), [1.0, 1.0, 1.0]);
        assert_eq!(lut.sample([1.0, 1.0, 1.0]), [0.0, 0.0, 0.0]);
        let mid = lut.sample([0.5, 0.5, 0.5]);
        assert!((mid[0] - 0.5).abs() < 1e-5, "{mid:?}");
    }

    #[test]
    fn a_declared_domain_rescales_the_input() {
        let text = IDENTITY.replace("LUT_3D_SIZE 2", "LUT_3D_SIZE 2\nDOMAIN_MAX 2.0 2.0 2.0");
        let lut = CubeLut::parse(&text).unwrap();
        // Half of the domain is the lattice's midpoint.
        let out = lut.sample([1.0, 1.0, 1.0]);
        for channel in out {
            assert!((channel - 0.5).abs() < 1e-5, "{out:?}");
        }
    }

    #[test]
    fn values_outside_the_domain_clamp_rather_than_extrapolate() {
        let lut = CubeLut::parse(IDENTITY).unwrap();
        assert_eq!(lut.sample([4.0, -2.0, 0.5])[0], 1.0);
        assert_eq!(lut.sample([4.0, -2.0, 0.5])[1], 0.0);
    }

    #[test]
    fn malformed_files_are_refused_by_name() {
        let cases = [
            ("", "no LUT_3D_SIZE"),
            ("LUT_3D_SIZE 1\n", "outside"),
            ("LUT_3D_SIZE 999\n", "outside"),
            ("LUT_3D_SIZE 2\n0 0 0\n", "expected 8 entries"),
            ("LUT_3D_SIZE 2\nLUT_3D_SIZE 2\n", "twice"),
            ("LUT_3D_SIZE two\n", "unreadable size"),
            ("LUT_3D_SIZE 2\nDOMAIN_MAX 0 0 0\n", "min < max"),
        ];
        for (text, expected) in cases {
            let error = CubeLut::parse(text).unwrap_err().to_string();
            assert!(
                error.contains(expected),
                "{text:?} should mention {expected:?}, said {error:?}"
            );
        }
        // A line that is neither a keyword nor a triplet.
        let error = CubeLut::parse("LUT_3D_SIZE 2\nWHAT IS THIS\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected three numbers"), "{error}");
    }

    #[test]
    fn a_missing_file_is_an_io_error_naming_the_path() {
        let error = CubeLut::load(Path::new("/no/such/look.cube"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("/no/such/look.cube"), "{error}");
    }
}
