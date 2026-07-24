//! Color management (ICC) for the Leyline engine.
//!
//! V1's scope was fixed, not general (ADR 0015): the render pipeline
//! (`leyline-engine` `process1`/`process2`) works entirely in sRGB, and this
//! crate's only job was to make that assumption explicit and
//! machine-checkable by handing exporters the canonical sRGB ICC profile to
//! embed, generated through LittleCMS rather than a profile shipped as a
//! binary blob.
//!
//! ADR 0027 widens this crate's surface, at the **output** boundary only —
//! the render pipeline's internal working space is unchanged (still sRGB,
//! still no new process version): loading an arbitrary destination ICC
//! profile and transforming rendered sRGB pixels into it, for non-sRGB
//! export, screen soft-proofing, and the print module (ADR 0036), which all
//! share this one primitive instead of each inventing their own.

use std::path::Path;
use std::sync::OnceLock;

use lcms2::{Intent as LcmsIntent, PixelFormat, Profile, Transform};
use serde::{Deserialize, Serialize};

/// Returns the canonical sRGB ICC profile, encoded as ICC bytes.
///
/// Generated once via LittleCMS's built-in sRGB primaries/transfer curve
/// and cached: every export shares the same profile bytes.
pub fn srgb_icc_profile() -> &'static [u8] {
    static PROFILE: OnceLock<Vec<u8>> = OnceLock::new();
    PROFILE
        .get_or_init(|| {
            lcms2::Profile::new_srgb()
                .icc()
                .expect("LittleCMS's built-in sRGB profile always serializes")
        })
        .as_slice()
}

/// The rendering intent an [`OutputTransform`] is built with — the same
/// four ICC intents every profile-aware tool exposes (a print preset stores
/// one by name, ADR 0036).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderingIntent {
    /// Preserves the visual relationship between colors, compressing the
    /// source gamut into the destination's — the usual choice for
    /// photographic output.
    Perceptual,
    /// Preserves in-gamut colors exactly, clips out-of-gamut ones.
    RelativeColorimetric,
    /// Maximizes saturation, at the expense of hue/lightness accuracy.
    Saturation,
    /// Like relative colorimetric, but also preserves the absolute white
    /// point (no adaptation) — mainly useful for proofing.
    AbsoluteColorimetric,
}

impl Default for RenderingIntent {
    /// Relative colorimetric: the conventional default for photographic
    /// output, preserving in-gamut colors exactly.
    fn default() -> Self {
        RenderingIntent::RelativeColorimetric
    }
}

impl RenderingIntent {
    fn to_lcms(self) -> LcmsIntent {
        match self {
            RenderingIntent::Perceptual => LcmsIntent::Perceptual,
            RenderingIntent::RelativeColorimetric => LcmsIntent::RelativeColorimetric,
            RenderingIntent::Saturation => LcmsIntent::Saturation,
            RenderingIntent::AbsoluteColorimetric => LcmsIntent::AbsoluteColorimetric,
        }
    }
}

/// An error loading a destination profile or building a transform out of
/// it — always a problem with the profile itself, never with the pixels
/// being converted.
#[derive(Debug, thiserror::Error)]
pub enum ColorError {
    /// The profile file could not be read from disk.
    #[error("failed to read ICC profile at {path}: {source}")]
    Read {
        /// The path that could not be read.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// LittleCMS rejected the profile bytes, or refused to build a
    /// transform between the two profiles (e.g. mismatched color spaces).
    #[error("invalid ICC profile or transform: {0}")]
    InvalidProfile(String),
}

/// A destination ICC profile loaded and ready to transform sRGB pixels
/// into, for export/proofing/print (ADR 0027).
pub struct OutputTransform {
    transform: Transform<[u8; 3], [u8; 3]>,
}

impl OutputTransform {
    /// Loads a destination ICC profile from disk and builds a transform
    /// from the render pipeline's working space (sRGB, gamma-encoded) into
    /// it, under the given rendering intent.
    pub fn load(profile_path: &Path, intent: RenderingIntent) -> Result<Self, ColorError> {
        let bytes = std::fs::read(profile_path).map_err(|source| ColorError::Read {
            path: profile_path.to_path_buf(),
            source,
        })?;
        Self::from_icc_bytes(&bytes, intent)
    }

    /// Builds a transform from the render pipeline's working space (sRGB,
    /// gamma-encoded) into the given destination profile's ICC bytes.
    pub fn from_icc_bytes(icc_bytes: &[u8], intent: RenderingIntent) -> Result<Self, ColorError> {
        let source = Profile::new_srgb();
        let destination =
            Profile::new_icc(icc_bytes).map_err(|e| ColorError::InvalidProfile(e.to_string()))?;
        let transform = Transform::new(
            &source,
            PixelFormat::RGB_8,
            &destination,
            PixelFormat::RGB_8,
            intent.to_lcms(),
        )
        .map_err(|e| ColorError::InvalidProfile(e.to_string()))?;
        Ok(Self { transform })
    }

    /// Transforms `rgb8` (tightly packed `[r, g, b, r, g, b, ...]`) in
    /// place, from sRGB into this transform's destination profile.
    ///
    /// Panics if `rgb8.len()` is not a multiple of 3 — every caller in this
    /// codebase builds `rgb8` from a validated `Rgb8`/image buffer, so this
    /// can never happen in practice.
    pub fn apply(&self, rgb8: &mut [u8]) {
        assert!(
            rgb8.len() % 3 == 0,
            "an RGB8 buffer's length is always a multiple of 3"
        );
        let pixels: &mut [[u8; 3]] = bytemuck::cast_slice_mut(rgb8);
        self.transform.transform_in_place(pixels);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_has_a_valid_icc_header() {
        let icc = srgb_icc_profile();
        assert!(icc.len() > 128, "an ICC profile has at least a header");
        assert_eq!(&icc[36..40], b"acsp", "ICC magic number at offset 36");
    }

    #[test]
    fn repeated_calls_return_the_same_bytes() {
        assert_eq!(srgb_icc_profile(), srgb_icc_profile());
    }

    #[test]
    fn srgb_to_srgb_transform_is_near_identity() {
        let transform = OutputTransform::from_icc_bytes(
            srgb_icc_profile(),
            RenderingIntent::RelativeColorimetric,
        )
        .expect("sRGB ICC bytes are a valid profile");
        let original = [10u8, 128, 250, 0, 0, 0, 255, 255, 255];
        let mut pixels = original;
        transform.apply(&mut pixels);
        for (actual, expected) in pixels.iter().zip(original.iter()) {
            assert!(
                actual.abs_diff(*expected) <= 2,
                "sRGB->sRGB should be near-identity, got {actual} vs {expected}"
            );
        }
    }

    #[test]
    fn rejects_garbage_profile_bytes() {
        let result =
            OutputTransform::from_icc_bytes(b"not an icc profile", RenderingIntent::Perceptual);
        assert!(result.is_err());
    }
}
