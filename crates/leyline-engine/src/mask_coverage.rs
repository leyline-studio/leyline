//! Reads, checksums and decodes a revision's stored mask coverages
//! (ADR 0070) — the engine-side half of the fail-closed contract
//! `leyline_core::Mask::Coverage`/`LeylineError::MaskCoverageFailed` document.
//!
//! Deliberately the same shape as [`crate::camera_profile`] and
//! [`crate::lut`]: a missing file, a checksum mismatch or a wrong pixel format
//! never renders, it errors. The failure mode this avoids is worse than
//! theirs — a local adjustment whose mask quietly disappeared does not lose a
//! look, it applies an exposure push to the entire photo.
//!
//! Reading happens here, once per render, and never from the pixel path.

use std::collections::HashMap;
use std::path::Path;

use leyline_core::{LeylineError, Mask, Result, Settings};

/// One decoded coverage: normalized `[0, 1]` samples, row-major.
///
/// Kept as `f32` rather than the file's `u16` because that is what
/// [`crate::mask`] blends with, and converting once here beats converting per
/// sampled pixel at every render size.
#[derive(Debug)]
pub(crate) struct Coverage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) samples: Vec<f32>,
}

impl Coverage {
    /// Samples the coverage bilinearly at a normalized canvas position.
    ///
    /// `(u, v)` outside `[0, 1]` clamps to the edge: a canvas is larger than
    /// the image once rotated (ADR 0026), so the corners genuinely fall
    /// outside, and repeating or zeroing there would draw a seam.
    pub(crate) fn sample(&self, u: f64, v: f64) -> f32 {
        let (w, h) = (self.width as usize, self.height as usize);
        if w == 0 || h == 0 {
            return 0.0;
        }
        // Pixel centers sit at (i + 0.5) / n, so the sample position in
        // pixel space is u * n - 0.5.
        let fx = (u * w as f64 - 0.5).clamp(0.0, (w - 1) as f64);
        let fy = (v * h as f64 - 0.5).clamp(0.0, (h - 1) as f64);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
        let at = |x: usize, y: usize| self.samples[y * w + x];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
        top + (bottom - top) * ty
    }
}

/// Every stored coverage a revision references, keyed by its library-relative
/// path — the already-resolved form a render receives, next to the decoded
/// camera profile and LUT.
///
/// Keyed by path rather than by adjustment index so one file referenced twice
/// is read, verified and decoded once.
#[derive(Default)]
pub struct MaskCoverages {
    coverages: HashMap<String, Coverage>,
}

impl MaskCoverages {
    /// The coverage a `Mask::Coverage` refers to.
    ///
    /// `None` cannot happen for a revision resolved by
    /// [`resolve_from_settings`]; a stage version treats it as no coverage
    /// rather than panicking, since the render must not abort on a bug here.
    pub(crate) fn get(&self, path: &str) -> Option<&Coverage> {
        self.coverages.get(path)
    }

    /// Whether anything was resolved — lets a caller skip the whole
    /// mechanism on the overwhelmingly common revision that has no stored
    /// mask.
    pub fn is_empty(&self) -> bool {
        self.coverages.is_empty()
    }
}

/// Resolves every [`Mask::Coverage`] of `settings` against `library_root`:
/// reads each referenced PNG, verifies its BLAKE3 checksum against what the
/// revision recorded, and decodes it to normalized samples.
///
/// An empty map when the revision references none, which is the usual case.
pub fn resolve_from_settings(library_root: &Path, settings: &Settings) -> Result<MaskCoverages> {
    let mut coverages = HashMap::new();
    for adjustment in &settings.local_adjustments {
        let Mask::Coverage { path, checksum } = &adjustment.mask else {
            continue;
        };
        if coverages.contains_key(path) {
            continue;
        }
        let file = library_root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        coverages.insert(path.clone(), resolve(&file, path, checksum)?);
    }
    Ok(MaskCoverages { coverages })
}

/// Reads and verifies one coverage file, then decodes it.
fn resolve(file: &Path, path: &str, checksum: &str) -> Result<Coverage> {
    let failed = |reason: String| LeylineError::MaskCoverageFailed {
        path: path.to_owned(),
        reason,
    };
    let bytes = std::fs::read(file).map_err(|e| failed(format!("could not read file: {e}")))?;
    let actual = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    if actual != checksum {
        return Err(failed(
            "the file on disk no longer matches this revision's recorded checksum".to_owned(),
        ));
    }
    decode(&bytes).map_err(failed)
}

/// Decodes the 16-bit grayscale PNG a coverage is stored as.
///
/// The format is checked rather than converted from: accepting an 8-bit or
/// RGB file would silently render a mask that is not the one
/// [`store_coverage`] wrote, and the whole point of the checksum is that no
/// substitution passes unnoticed.
fn decode(bytes: &[u8]) -> std::result::Result<Coverage, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("not a readable PNG: {e}"))?;
    let info = reader.info();
    if info.color_type != png::ColorType::Grayscale || info.bit_depth != png::BitDepth::Sixteen {
        return Err(format!(
            "a stored coverage is a 16-bit grayscale PNG, this one is {:?}/{:?}",
            info.color_type, info.bit_depth
        ));
    }
    let (width, height) = (info.width, info.height);
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG dimensions overflow this platform".to_owned())?;
    let mut buffer = vec![0u8; size];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|e| format!("could not decode: {e}"))?;
    let samples = buffer[..frame.buffer_size()]
        .chunks_exact(2)
        // PNG is big-endian, whatever the host is.
        .map(|pair| f32::from(u16::from_be_bytes([pair[0], pair[1]])) / f32::from(u16::MAX))
        .collect();
    Ok(Coverage {
        width,
        height,
        samples,
    })
}

/// Writes a coverage into `library_root` under `Masks/<blake3>.png` and
/// returns the [`Mask::Coverage`] that references it (ADR 0070 §5).
///
/// Content-addressed, so storing the same coverage twice writes one file and
/// the second call is a no-op. The caller never learns the layout — that is
/// what lets it change later without touching a single stored revision.
pub fn store_coverage(
    library_root: &Path,
    width: u32,
    height: u32,
    coverage: &[u16],
) -> Result<Mask> {
    let expected = width as usize * height as usize;
    if width == 0 || height == 0 || coverage.len() != expected {
        return Err(LeylineError::InvalidSettings(format!(
            "a {width}x{height} coverage needs {expected} samples, got {}",
            coverage.len()
        )));
    }

    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder
            .write_header()
            .map_err(|e| LeylineError::InvalidImage(e.to_string()))?;
        let big_endian: Vec<u8> = coverage.iter().flat_map(|s| s.to_be_bytes()).collect();
        writer
            .write_image_data(&big_endian)
            .map_err(|e| LeylineError::InvalidImage(e.to_string()))?;
    }

    let hash = blake3::hash(&png_bytes);
    let path = format!("Masks/{}.png", hash.to_hex());
    let file = library_root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !file.exists() {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&file, &png_bytes)?;
    }
    Ok(Mask::Coverage {
        path,
        checksum: format!("blake3:{}", hash.to_hex()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The round trip ADR 0070 §5 promises: what `store_coverage` writes,
    /// `resolve_from_settings` reads back, at the same values.
    #[test]
    fn a_stored_coverage_resolves_back_to_its_samples() {
        let dir = tempfile::tempdir().unwrap();
        let samples: Vec<u16> = (0..16u16).map(|i| i * 4369).collect();
        let mask = store_coverage(dir.path(), 4, 4, &samples).unwrap();
        let Mask::Coverage { path, checksum } = &mask else {
            panic!("store_coverage returns a coverage mask");
        };
        assert!(path.starts_with("Masks/") && path.ends_with(".png"));
        assert!(checksum.starts_with("blake3:"));

        let settings = Settings {
            stages: leyline_core::StageVersions::from([("local_adjustments".to_owned(), 3)]),
            local_adjustments: vec![leyline_core::LocalAdjustment {
                mask: mask.clone(),
                range: None,
                opacity: 1.0,
                adjustments: leyline_core::LocalAdjustmentValues::default(),
            }],
            ..Settings::default()
        };
        let resolved = resolve_from_settings(dir.path(), &settings).unwrap();
        let coverage = resolved.get(path).expect("the reference resolves");
        assert_eq!((coverage.width, coverage.height), (4, 4));
        for (stored, decoded) in samples.iter().zip(&coverage.samples) {
            assert!(
                (f32::from(*stored) / f32::from(u16::MAX) - decoded).abs() < 1e-6,
                "{stored} came back as {decoded}"
            );
        }
    }

    /// Content addressing: the same coverage stored twice is one file.
    #[test]
    fn storing_the_same_coverage_twice_writes_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let samples = vec![1234u16; 9];
        let first = store_coverage(dir.path(), 3, 3, &samples).unwrap();
        let second = store_coverage(dir.path(), 3, 3, &samples).unwrap();
        assert_eq!(first, second);
        let files: Vec<_> = std::fs::read_dir(dir.path().join("Masks"))
            .unwrap()
            .collect();
        assert_eq!(files.len(), 1);
    }

    /// The fail-closed contract: a file edited after the revision recorded
    /// it is an error, never a render through a different mask.
    #[test]
    fn a_changed_file_is_refused_rather_than_rendered() {
        let dir = tempfile::tempdir().unwrap();
        let mask = store_coverage(dir.path(), 2, 2, &[0, 1, 2, 3]).unwrap();
        let Mask::Coverage { path, .. } = &mask else {
            unreachable!()
        };
        let settings = Settings {
            stages: leyline_core::StageVersions::from([("local_adjustments".to_owned(), 3)]),
            local_adjustments: vec![leyline_core::LocalAdjustment {
                mask: mask.clone(),
                range: None,
                opacity: 1.0,
                adjustments: leyline_core::LocalAdjustmentValues::default(),
            }],
            ..Settings::default()
        };
        assert!(resolve_from_settings(dir.path(), &settings).is_ok());

        // Same dimensions, different pixels: only the checksum can tell.
        let replacement = store_coverage(dir.path(), 2, 2, &[9, 9, 9, 9]).unwrap();
        let Mask::Coverage { path: other, .. } = &replacement else {
            unreachable!()
        };
        std::fs::copy(
            dir.path().join(other.replace('/', std::path::MAIN_SEPARATOR_STR)),
            dir.path().join(path.replace('/', std::path::MAIN_SEPARATOR_STR)),
        )
        .unwrap();
        assert!(matches!(
            resolve_from_settings(dir.path(), &settings),
            Err(LeylineError::MaskCoverageFailed { .. })
        ));
    }

    /// An 8-bit file is refused rather than promoted: it is not what was
    /// written, and the point of the checksum is that nothing substitutes
    /// silently.
    #[test]
    fn only_a_16_bit_grayscale_png_is_accepted() {
        let mut eight_bit = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut eight_bit, 2, 2);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0, 64, 128, 255]).unwrap();
        }
        let error = decode(&eight_bit).unwrap_err();
        assert!(error.contains("16-bit grayscale"), "{error}");
    }

    #[test]
    fn sampling_is_bilinear_and_clamps_outside_the_unit_square() {
        let coverage = Coverage {
            width: 2,
            height: 1,
            samples: vec![0.0, 1.0],
        };
        // Pixel centers at u = 0.25 and u = 0.75.
        assert!((coverage.sample(0.25, 0.5) - 0.0).abs() < 1e-6);
        assert!((coverage.sample(0.75, 0.5) - 1.0).abs() < 1e-6);
        assert!((coverage.sample(0.5, 0.5) - 0.5).abs() < 1e-6);
        // Outside clamps to the edge rather than wrapping or vanishing.
        assert!((coverage.sample(-1.0, 0.5) - 0.0).abs() < 1e-6);
        assert!((coverage.sample(2.0, 0.5) - 1.0).abs() < 1e-6);
    }
}
