//! Color management (ICC) for the Leyline engine.
//!
//! V1's scope is fixed, not general: the render pipeline (`leyline-engine`
//! `process1`/`process2`) already works and outputs in sRGB — LibRaw is
//! asked for sRGB output on decode, and the tone step applies the sRGB
//! transfer function. This crate's only job is to make that assumption
//! explicit and machine-checkable by handing exporters the canonical sRGB
//! ICC profile to embed, generated through LittleCMS rather than a profile
//! shipped as a binary blob. See `docs/adr/0015-color-management-srgb.md`.

use std::sync::OnceLock;

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
}
