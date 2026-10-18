//! `verify`: is this file one the open library wrote? (ADR 0153 §4)
//!
//! Three answers and no fourth: *intact* (the file's checksum is in the
//! journal), *modified* (only its revision fingerprint is), *unknown to this
//! library*. Never "authentic": BLAKE3 says the bytes are the same, not who
//! wrote them.

use leyline_catalog::{CHECKSUM_LEN, Catalog, ExportRecord};
use leyline_core::Result;

/// What [`crate::Library::verify_export`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportVerdict {
    /// The file is byte for byte one this library wrote; the journal row of
    /// the most recent export that wrote it.
    Intact(ExportRecord),
    /// The file names a revision this library exported, but its bytes are
    /// not the ones written: re-saved, re-compressed or edited elsewhere.
    /// The journal row of the most recent export of that revision.
    Modified(ExportRecord),
    /// Neither its checksum nor its fingerprint is in this library's
    /// journal: another library, an export from before migration v14, or a
    /// file Leyline never wrote.
    Unknown,
}

/// The verdict for a file whose bytes are `file` and whose BLAKE3 is
/// `checksum` — hashed by the caller before the catalog lock is taken.
pub(crate) fn verdict(
    catalog: &Catalog,
    file: &[u8],
    checksum: &[u8; CHECKSUM_LEN],
) -> Result<ExportVerdict> {
    if let Some(row) = catalog
        .exports_by_output_checksum(checksum)?
        .into_iter()
        .next()
    {
        return Ok(ExportVerdict::Intact(row));
    }
    let Some(fingerprint) = revision_fingerprint_in(file) else {
        return Ok(ExportVerdict::Unknown);
    };
    Ok(catalog
        .exports_by_revision_fingerprint(&fingerprint)?
        .into_iter()
        .next()
        .map_or(ExportVerdict::Unknown, ExportVerdict::Modified))
}

/// The `leyline:Revision` a file carries, if any (ADR 0153 §3).
///
/// Found by scanning the bytes rather than by parsing each carrier: every
/// format Leyline writes stores its XMP packet uncompressed. Both XMP forms
/// are read — the element Leyline writes and the attribute another tool may
/// rewrite it as — because a file re-saved elsewhere is exactly the one
/// `verify` must still recognise as *modified*.
fn revision_fingerprint_in(file: &[u8]) -> Option<[u8; CHECKSUM_LEN]> {
    const NAME: &[u8] = b"leyline:Revision";
    let mut rest = file;
    while let Some(at) = find(rest, NAME) {
        let after = &rest[at + NAME.len()..];
        let value = after
            .strip_prefix(b">")
            .or_else(|| after.strip_prefix(b"=\""))
            .or_else(|| after.strip_prefix(b"='"));
        if let Some(fingerprint) = value.and_then(hex_fingerprint) {
            return Some(fingerprint);
        }
        rest = after;
    }
    None
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 64 hexadecimal digits at the start of `text`, as 32 bytes.
fn hex_fingerprint(text: &[u8]) -> Option<[u8; CHECKSUM_LEN]> {
    let digits = text.get(..CHECKSUM_LEN * 2)?;
    let mut out = [0u8; CHECKSUM_LEN];
    for (byte, pair) in out.iter_mut().zip(digits.chunks(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_is_read_as_element_or_attribute() {
        let hex = "ab".repeat(32);
        for packet in [
            format!("<leyline:Revision>{hex}</leyline:Revision>"),
            format!("<rdf:Description leyline:Revision=\"{hex}\"/>"),
        ] {
            assert_eq!(
                revision_fingerprint_in(packet.as_bytes()),
                Some([0xAB; CHECKSUM_LEN])
            );
        }
    }

    #[test]
    fn a_truncated_or_foreign_value_is_no_fingerprint() {
        assert_eq!(
            revision_fingerprint_in(b"<leyline:Revision>abcd</leyline:Revision>"),
            None
        );
        assert_eq!(
            revision_fingerprint_in(format!("leyline:Revision>{}", "zz".repeat(32)).as_bytes()),
            None
        );
        assert_eq!(revision_fingerprint_in(b"no packet at all"), None);
    }
}
