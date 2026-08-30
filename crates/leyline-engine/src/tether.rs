//! The session-level decisions of tethered capture
//! (`docs/adr/0087-tethered-capture-bar.md` §4): what a capture session is
//! called, where its shots are filed, and how two shots the camera named
//! identically are told apart.
//!
//! Deliberately free of libgphoto2: this is the part that has to be right
//! whether or not the `tether` backend is compiled in, and the part that is
//! testable without a camera plugged into the machine.

#[cfg(feature = "tether")]
use std::path::Path;

use leyline_core::{LeylineError, PresetId, Result, validate_library_relative_path};

/// The folder a session files its shots under when the photographer names
/// none — a name, not an empty string, because `Photos/` itself is where
/// every ordinary import lands and a tethered session deserves its own
/// shelf even unnamed.
pub const DEFAULT_SESSION: &str = "Tethered";

/// What is decided before a session opens (ADR 0087 §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TetherOptions {
    /// The session's name, which is also the folder under `Photos/` its
    /// shots are filed in. Blank means [`DEFAULT_SESSION`].
    pub session: String,
    /// A develop preset applied to every shot as it arrives, before the
    /// client is told the photo exists (ADR 0087 §5) — so a tethered shot
    /// never flashes its neutral render first.
    pub preset: Option<PresetId>,
}

impl Default for TetherOptions {
    fn default() -> Self {
        TetherOptions {
            session: DEFAULT_SESSION.to_owned(),
            preset: None,
        }
    }
}

/// Validates a session name and returns the folder name to use.
///
/// A session name is the one place a user string becomes a directory, so it
/// is held to the same discipline as any stored path (`..`, absolute forms,
/// drive letters, backslashes) *and* to one more: it must be a single
/// component. A session called `2026/Studio` would file its shots two
/// levels down, where the folder tree shows a `2026` no import created.
pub fn session_folder(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Ok(DEFAULT_SESSION.to_owned());
    }
    if trimmed.contains('/') {
        return Err(LeylineError::InvalidSettings(format!(
            "a tether session name is one folder name, not a path: got {trimmed:?}"
        )));
    }
    validate_library_relative_path("tether session name", trimmed)?;
    Ok(trimmed.to_owned())
}

/// A name for `file_name` that is free inside `folder`.
///
/// Cameras reuse filenames — reformat a card mid-session and `IMG_0001.CR2`
/// comes round again. The import core answers a name collision with a
/// `Skip`, which for a tethered shot means the frame is silently lost, so
/// the collision is resolved here, before the import is ever asked.
///
/// Only a build with the libgphoto2 backend has shots to file, so this is
/// gated with it — unlike [`session_folder`], which a client calls to
/// validate what the photographer typed whether or not it can then connect.
#[cfg(feature = "tether")]
pub fn free_name(folder: &Path, file_name: &str) -> String {
    if !folder.join(file_name).exists() {
        return file_name.to_owned();
    }
    let (stem, extension) = match file_name.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (file_name, String::new()),
    };
    for suffix in 1..=9_999u32 {
        let candidate = format!("{stem}-{suffix}{extension}");
        if !folder.join(&candidate).exists() {
            return candidate;
        }
    }
    // Ten thousand shots sharing one filename in one session is not a case
    // worth a smarter search, but it is a case worth not losing a frame
    // over: the clock breaks the tie where counting gave up.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    format!("{stem}-{stamp}{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_session_name_is_its_own_folder() {
        assert_eq!(
            session_folder("Creative Live Himani").expect("accepted"),
            "Creative Live Himani"
        );
        assert_eq!(session_folder("  Studio  ").expect("trimmed"), "Studio");
    }

    #[test]
    fn a_blank_session_name_falls_back_rather_than_failing() {
        assert_eq!(session_folder("").expect("accepted"), DEFAULT_SESSION);
        assert_eq!(session_folder("   ").expect("accepted"), DEFAULT_SESSION);
    }

    /// The session name is the one user string that becomes a directory
    /// under the library root; everything `validate_library_relative_path`
    /// refuses for a stored path is refused here too, plus separators.
    #[test]
    fn a_session_name_that_is_a_path_is_refused() {
        for bad in ["2026/Studio", "..", ".", "../escape", "C:", "a\\b", "/abs"] {
            assert!(
                session_folder(bad).is_err(),
                "{bad:?} should not become a folder"
            );
        }
    }

    #[cfg(feature = "tether")]
    #[test]
    fn a_free_filename_is_left_exactly_as_the_camera_named_it() {
        let dir = std::env::temp_dir().join("leyline-tether-free-name-a");
        std::fs::create_dir_all(&dir).expect("dir");
        let _ = std::fs::remove_file(dir.join("IMG_0001.CR2"));
        assert_eq!(free_name(&dir, "IMG_0001.CR2"), "IMG_0001.CR2");
    }

    /// A reformatted card sending `IMG_0001.CR2` a second time must produce
    /// a second photo, not a skipped import.
    #[cfg(feature = "tether")]
    #[test]
    fn a_taken_filename_is_suffixed_before_its_extension() {
        let dir = std::env::temp_dir().join("leyline-tether-free-name-b");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("IMG_0001.CR2"), b"raw").expect("first");
        assert_eq!(free_name(&dir, "IMG_0001.CR2"), "IMG_0001-1.CR2");
        std::fs::write(dir.join("IMG_0001-1.CR2"), b"raw").expect("second");
        assert_eq!(free_name(&dir, "IMG_0001.CR2"), "IMG_0001-2.CR2");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[cfg(feature = "tether")]
    #[test]
    fn a_name_without_an_extension_keeps_its_shape() {
        let dir = std::env::temp_dir().join("leyline-tether-free-name-c");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("CAPTURE"), b"raw").expect("first");
        assert_eq!(free_name(&dir, "CAPTURE"), "CAPTURE-1");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
