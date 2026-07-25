//! Reads, checksums, and parses a revision's referenced camera profile
//! (ADR 0035) — the engine-side half of the fail-closed contract
//! `leyline_core::CameraProfile`/`LeylineError::CameraProfileFailed`
//! document: a missing file, a checksum mismatch, or a parse failure never
//! renders, it errors — the same "never modify, never guess" posture
//! `docs/pipeline.md` §3.4 already gives `NewerSettings`.

use std::path::Path;

use leyline_core::{CameraProfile, LeylineError, Result, Settings};

/// Resolves `settings.camera_profile` against `library_root`: reads the
/// referenced `.dcp` file, verifies its BLAKE3 checksum against what the
/// revision recorded, and parses it. `None` when there's no profile
/// referenced, or it's present but not `enabled` — the same
/// neutral-value-skips-the-stage rule every other operator in this engine
/// follows.
pub(crate) fn resolve_from_settings(
    library_root: &Path,
    settings: &Settings,
) -> Result<Option<leyline_color::DcpProfile>> {
    let Some(profile) = &settings.camera_profile else {
        return Ok(None);
    };
    if !profile.enabled {
        return Ok(None);
    }
    let path = library_root.join(profile.path.replace('/', std::path::MAIN_SEPARATOR_STR));
    resolve(&path, profile).map(Some)
}

/// Reads and verifies the `.dcp` file at `path` against `profile`'s
/// recorded checksum, then parses it.
fn resolve(path: &Path, profile: &CameraProfile) -> Result<leyline_color::DcpProfile> {
    let bytes = std::fs::read(path).map_err(|e| LeylineError::CameraProfileFailed {
        path: profile.path.clone(),
        reason: format!("could not read file: {e}"),
    })?;
    let checksum = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    if checksum != profile.checksum {
        return Err(LeylineError::CameraProfileFailed {
            path: profile.path.clone(),
            reason: "the file on disk no longer matches this revision's recorded checksum"
                .to_owned(),
        });
    }
    leyline_color::DcpProfile::parse(&bytes).map_err(|e| LeylineError::CameraProfileFailed {
        path: profile.path.clone(),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_camera_profile_resolves_to_none() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::default();
        assert_eq!(resolve_from_settings(dir.path(), &settings).unwrap(), None);
    }

    #[test]
    fn disabled_camera_profile_resolves_to_none_without_reading_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            camera_profile: Some(CameraProfile {
                enabled: false,
                path: "Profiles/Camera/missing.dcp".to_owned(),
                checksum: format!("blake3:{}", "a".repeat(64)),
            }),
            ..Settings::default()
        };
        // The referenced file does not exist on disk; a disabled profile
        // must never even try to read it.
        assert_eq!(resolve_from_settings(dir.path(), &settings).unwrap(), None);
    }

    #[test]
    fn a_missing_file_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            camera_profile: Some(CameraProfile {
                enabled: true,
                path: "Profiles/Camera/missing.dcp".to_owned(),
                checksum: format!("blake3:{}", "a".repeat(64)),
            }),
            ..Settings::default()
        };
        assert!(matches!(
            resolve_from_settings(dir.path(), &settings),
            Err(LeylineError::CameraProfileFailed { .. })
        ));
    }

    #[test]
    fn a_checksum_mismatch_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Profiles/Camera")).unwrap();
        std::fs::write(
            dir.path().join("Profiles/Camera/mine.dcp"),
            b"not a real dcp",
        )
        .unwrap();
        let settings = Settings {
            camera_profile: Some(CameraProfile {
                enabled: true,
                path: "Profiles/Camera/mine.dcp".to_owned(),
                // Deliberately wrong: the checksum of `b"not a real dcp"`
                // is something else entirely.
                checksum: format!("blake3:{}", "a".repeat(64)),
            }),
            ..Settings::default()
        };
        assert!(matches!(
            resolve_from_settings(dir.path(), &settings),
            Err(LeylineError::CameraProfileFailed { .. })
        ));
    }

    #[test]
    fn a_matching_checksum_but_unparseable_file_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Profiles/Camera")).unwrap();
        let bytes = b"not a real dcp";
        std::fs::write(dir.path().join("Profiles/Camera/mine.dcp"), bytes).unwrap();
        let checksum = format!("blake3:{}", blake3::hash(bytes).to_hex());
        let settings = Settings {
            camera_profile: Some(CameraProfile {
                enabled: true,
                path: "Profiles/Camera/mine.dcp".to_owned(),
                checksum,
            }),
            ..Settings::default()
        };
        assert!(matches!(
            resolve_from_settings(dir.path(), &settings),
            Err(LeylineError::CameraProfileFailed { .. })
        ));
    }
}
