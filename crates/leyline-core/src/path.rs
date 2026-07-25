//! Validation of library-relative paths — the string form every reference
//! stored in a revision or the catalog uses to point at a file inside the
//! library root.

use crate::{LeylineError, Result};

/// Checks that `value` is a safe library-relative path: forward-slashed,
/// strictly inside the library root, and portable to Windows.
///
/// Every path Leyline stores in `settings_json` or the catalog is resolved
/// by joining it onto the library root, so an unvalidated one is a file-read
/// primitive: `PathBuf::join` *replaces* the root outright when handed an
/// absolute path, and `..` walks out of it one component at a time. Both are
/// also reproducibility breaks — a revision that reads a file outside the
/// library cannot be rendered identically on another machine.
///
/// `name` is the caller's field name, used in the error message.
pub fn validate_library_relative_path(name: &str, value: &str) -> Result<()> {
    let reject = |reason: &str| {
        Err(LeylineError::InvalidSettings(format!(
            "{name} must be a library-relative path: {reason}, got {value:?}"
        )))
    };

    if value.trim().is_empty() {
        return reject("it is empty");
    }
    // Backslashes are a separator on Windows but an ordinary filename
    // character on Unix: a path containing one cannot mean the same thing on
    // both, so it is never a valid stored path regardless of what it points
    // at.
    if value.contains('\\') {
        return reject("it contains a backslash; use '/' as the separator");
    }
    if value.starts_with('/') {
        return reject("it is absolute");
    }
    // `C:` / `\\?\` and friends. Checked explicitly rather than via
    // `Path::has_root`, which only recognizes the *host's* conventions —
    // a Windows-style prefix must be rejected when validating on Unix too.
    if value.len() >= 2 && value.as_bytes()[1] == b':' && value.as_bytes()[0].is_ascii_alphabetic()
    {
        return reject("it starts with a drive letter");
    }
    for component in value.split('/') {
        match component {
            "" => return reject("it has an empty component"),
            "." | ".." => return reject("it has a '.' or '..' component"),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_library_relative_path_is_accepted() {
        for ok in [
            "Profiles/Camera/EOS 60D.dcp",
            "Photos/2026/IMG_0001.CR2",
            "pack.mbtiles",
            "a/b/c/d.dcp",
        ] {
            assert!(
                validate_library_relative_path("field", ok).is_ok(),
                "{ok} should be accepted"
            );
        }
    }

    #[test]
    fn traversal_and_absolute_paths_are_refused() {
        for bad in [
            "../outside.dcp",
            "Profiles/../../outside.dcp",
            "./here.dcp",
            "/etc/passwd",
            "/tmp/x.dcp",
            "C:\\x.dcp",
            "C:/x.dcp",
            "Profiles\\Camera\\x.dcp",
            "Profiles//x.dcp",
            "",
            "   ",
        ] {
            assert!(
                validate_library_relative_path("field", bad).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    #[test]
    fn the_error_names_the_offending_field() {
        let error = validate_library_relative_path("camera_profile.path", "../x.dcp").unwrap_err();
        assert!(error.to_string().contains("camera_profile.path"), "{error}");
    }
}
