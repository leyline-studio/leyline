//! Where an export comes from (ADR 0153): the revision fingerprint, and the
//! checksum of the file written.

use std::path::Path;

use leyline_catalog::CHECKSUM_LEN;
use leyline_core::Result;

/// `BLAKE3(asset checksum ‖ 0x00 ‖ settings_json)` (ADR 0153 §1).
///
/// The settings are the revision's string **as stored**, never a
/// re-serialization: a revision is immutable, so this input never moves,
/// whereas re-serializing `Settings` with a later build could add a field
/// and give the same revision a new fingerprint. The separator keeps the two
/// inputs from sliding into each other, though the first has a fixed length.
pub(crate) fn revision_fingerprint(
    asset_checksum: &[u8; CHECKSUM_LEN],
    settings_json: &str,
) -> [u8; CHECKSUM_LEN] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(asset_checksum);
    hasher.update(&[0]);
    hasher.update(settings_json.as_bytes());
    *hasher.finalize().as_bytes()
}

/// BLAKE3 of the file written — what `verify` compares (ADR 0153 §4).
/// Read before the catalog lock is taken: hashing a large TIFF has no
/// business holding it (ADR 0024).
pub(crate) fn file_checksum(path: &Path) -> Result<[u8; CHECKSUM_LEN]> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(std::fs::File::open(path)?)?;
    Ok(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frozen vector: every exported file records this function's output,
    /// so changing the formula would silently orphan all of them. If this
    /// test fails, the change needs an ADR, not a new expected value.
    #[test]
    fn the_fingerprint_formula_is_frozen() {
        let fingerprint = revision_fingerprint(&[7; CHECKSUM_LEN], r#"{"schema":1}"#);
        assert_eq!(
            blake3::Hash::from(fingerprint).to_hex().as_str(),
            FROZEN_VECTOR
        );
    }

    /// Computed outside this function, by hashing the 45 concatenated bytes
    /// in one call rather than through an incremental `Hasher`.
    const FROZEN_VECTOR: &str = "58eb7b464e7fff193ab2c590be1eaa370ff471340b9f0a9b1c5ff1eab64aec13";

    #[test]
    fn a_stage_version_alone_changes_the_fingerprint() {
        let checksum = [1; CHECKSUM_LEN];
        assert_ne!(
            revision_fingerprint(&checksum, r#"{"stages":{"tone":1}}"#),
            revision_fingerprint(&checksum, r#"{"stages":{"tone":2}}"#)
        );
    }
}
