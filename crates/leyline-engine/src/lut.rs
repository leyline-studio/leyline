//! Reads, checksums, and parses a revision's referenced creative LUT
//! (ADR 0053) — the engine-side half of the fail-closed contract
//! `leyline_core::Lut`/`LeylineError::LutFailed` document.
//!
//! Deliberately the same shape as [`crate::camera_profile`]: a missing file, a
//! checksum mismatch or a malformed table never renders, it errors. A look that
//! silently stopped applying would leave the user comparing an export against a
//! preview and finding no explanation.

use std::path::Path;

use leyline_color::CubeLut;
use leyline_core::{LeylineError, Lut, Result, Settings};

/// Resolves `settings.lut` against `library_root`: reads the referenced
/// `.cube`, verifies its BLAKE3 checksum against what the revision recorded,
/// and parses it. `None` when no LUT is referenced or the reference is present
/// but disabled — the neutral-value-skips-the-stage rule every operator
/// follows.
pub(crate) fn resolve_from_settings(
    library_root: &Path,
    settings: &Settings,
) -> Result<Option<CubeLut>> {
    let Some(lut) = &settings.lut else {
        return Ok(None);
    };
    if !lut.enabled {
        return Ok(None);
    }
    let path = library_root.join(lut.path.replace('/', std::path::MAIN_SEPARATOR_STR));
    resolve(&path, lut).map(Some)
}

/// Reads and verifies the `.cube` file at `path` against `lut`'s recorded
/// checksum, then parses it.
fn resolve(path: &Path, lut: &Lut) -> Result<CubeLut> {
    let bytes = std::fs::read(path).map_err(|e| LeylineError::LutFailed {
        path: lut.path.clone(),
        reason: format!("could not read file: {e}"),
    })?;
    let checksum = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    if checksum != lut.checksum {
        return Err(LeylineError::LutFailed {
            path: lut.path.clone(),
            reason: "the file on disk no longer matches this revision's recorded checksum"
                .to_owned(),
        });
    }
    let text = String::from_utf8(bytes).map_err(|_| LeylineError::LutFailed {
        path: lut.path.clone(),
        reason: "a .cube file is text; this one is not valid UTF-8".to_owned(),
    })?;
    CubeLut::parse(&text).map_err(|e| LeylineError::LutFailed {
        path: lut.path.clone(),
        reason: e.to_string(),
    })
}
