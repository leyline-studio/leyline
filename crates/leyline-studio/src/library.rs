//! Opening, creating and remembering libraries (ADR 0045 §4).
//!
//! Everything here is about *which* library Studio is pointed at, never about
//! its contents — including the relaunch that switches to another one.

use std::path::Path;
use std::path::PathBuf;

use crate::app::report_error;
use crate::ui::StudioWindow;
use leyline_sdk::Library;

/// The name given to a library created by the no-argument fallback (shown
/// in the window title and the About dialog, same field a library created
/// through `leyline-cli new` gets from its own `--name`).
pub(crate) const DEFAULT_LIBRARY_NAME: &str = "Leyline Library";

/// Where Studio opens a library when launched with no path argument:
/// `<Documents>/Leyline Library`, falling back to `<home>/Leyline Library`
/// when the platform (or a minimal container image) has no Documents
/// folder. Pure and taking the candidate directories as parameters — rather
/// than calling `directories::UserDirs` itself — so tests can point it at a
/// scratch `TempDir` instead of the real test runner's home directory;
/// [`default_library_dir`] is the thin wrapper that resolves the real ones.
pub(crate) fn default_library_root(
    documents_dir: Option<&Path>,
    home_dir: Option<&Path>,
) -> Option<PathBuf> {
    documents_dir
        .or(home_dir)
        .map(|base| base.join(DEFAULT_LIBRARY_NAME))
}

/// Resolves [`default_library_root`] against the real user directories.
pub(crate) fn default_library_dir() -> Result<PathBuf, String> {
    let user_dirs = directories::UserDirs::new()
        .ok_or_else(|| "cannot determine the user's home directory".to_owned())?;
    default_library_root(user_dirs.document_dir(), Some(user_dirs.home_dir()))
        .ok_or_else(|| "cannot determine a default library location".to_owned())
}

/// Opens the library at `root`, creating it first if this is the first time
/// (no `catalog.db` there yet) — used only for the no-argument fallback
/// location; a library path given explicitly on the command line still goes
/// through the plain [`Library::open`] below, unchanged, so an explicit
/// argument that points at a typo'd or missing directory keeps failing
/// loudly rather than silently creating a new, empty library there.
pub(crate) fn open_or_create_library(root: &Path) -> Result<Library, String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    if root.join("catalog.db").is_file() {
        Library::open(root).map_err(|e| e.to_string())
    } else {
        Library::create(root, DEFAULT_LIBRARY_NAME).map_err(|e| e.to_string())
    }
}

/// Where the "Open Recent" library list is persisted: a small JSON array of
/// paths, one per user, next to other per-user app config rather than inside
/// any single library folder (a library is portable/self-contained per
/// `docs/catalog.md` §37 — it must not gain a side file recording other
/// libraries' locations).
pub(crate) fn recent_libraries_path() -> Result<PathBuf, String> {
    let dirs = directories::ProjectDirs::from("", "", "Leyline")
        .ok_or_else(|| "cannot determine the user's config directory".to_owned())?;
    Ok(dirs.config_dir().join("recent_libraries.json"))
}

/// Reads the recent-libraries list, tolerating a missing or corrupt file
/// (first launch, or a manually edited/truncated file) by returning an empty
/// list rather than failing Studio's startup over a non-essential feature.
pub(crate) fn load_recent_libraries(path: &Path) -> Vec<PathBuf> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&contents)
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

pub(crate) fn save_recent_libraries(path: &Path, libraries: &[PathBuf]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let strings: Vec<String> = libraries.iter().map(|p| p.display().to_string()).collect();
    let json = serde_json::to_string_pretty(&strings).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Moves `opened` to the front of `existing`, de-duplicates, and caps the
/// list — pure so it's testable without touching the real config directory.
pub(crate) fn record_recent_library(existing: &[PathBuf], opened: &Path) -> Vec<PathBuf> {
    const MAX_RECENT: usize = 8;
    let mut updated = vec![opened.to_path_buf()];
    updated.extend(existing.iter().filter(|p| p.as_path() != opened).cloned());
    updated.truncate(MAX_RECENT);
    updated
}

/// Surfaces a callback failure in the status line, where a GUI user can
/// see it; stderr keeps a copy for terminal logs.
/// Switches to a different library the same way Lightroom's "Open Catalog…"
/// does: spawn a fresh Studio process pointed at the new library path, then
/// quit this one. In-place switching would mean tearing down and rebuilding
/// every piece of `App` state (grid query, develop session, event
/// subscription…) that `run()` currently only ever sets up once; relaunching
/// reuses that same one-time startup path instead of duplicating it.
pub(crate) fn relaunch_into(window: &StudioWindow, library_root: &Path) {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            report_error(window, &error.to_string());
            return;
        }
    };
    match std::process::Command::new(exe).arg(library_root).spawn() {
        Ok(_) => {
            let _ = slint::quit_event_loop();
        }
        Err(error) => report_error(window, &error.to_string()),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn default_library_root_prefers_documents_over_home() {
        let documents = Path::new("/scratch/Documents");
        let home = Path::new("/scratch/home/user");
        assert_eq!(
            default_library_root(Some(documents), Some(home)),
            Some(documents.join(DEFAULT_LIBRARY_NAME))
        );
    }

    #[test]
    fn default_library_root_falls_back_to_home_without_documents() {
        let home = Path::new("/scratch/home/user");
        assert_eq!(
            default_library_root(None, Some(home)),
            Some(home.join(DEFAULT_LIBRARY_NAME))
        );
    }

    #[test]
    fn default_library_root_is_none_without_any_candidate() {
        assert_eq!(default_library_root(None, None), None);
    }

    #[test]
    fn open_or_create_library_creates_a_real_library_on_first_launch() {
        // Mirrors what `run()` does for a no-argument launch: the fallback
        // directory doesn't exist yet, and doesn't even have a parent
        // directory on disk — `open_or_create_library` must create both and
        // the catalog inside, exactly as if that path had been passed
        // explicitly. Uses a scratch `TempDir` so this never touches the
        // real test runner's home directory.
        let scratch = tempfile::tempdir().expect("tempdir");
        let root = scratch.path().join("Documents").join(DEFAULT_LIBRARY_NAME);
        assert!(!root.exists());

        let library = open_or_create_library(&root).expect("first launch creates the library");
        assert!(root.join("catalog.db").is_file());
        let info = library.catalog().library().expect("read library info");
        assert_eq!(info.name, DEFAULT_LIBRARY_NAME);
        drop(library);

        // A second launch at the same path must open, not re-create.
        let reopened = open_or_create_library(&root).expect("second launch opens the library");
        let info = reopened.catalog().library().expect("read library info");
        assert_eq!(info.name, DEFAULT_LIBRARY_NAME);
    }

    #[test]
    fn record_recent_library_puts_the_opened_path_first() {
        let existing = vec![PathBuf::from("/libs/b"), PathBuf::from("/libs/c")];
        let updated = record_recent_library(&existing, Path::new("/libs/a"));
        assert_eq!(
            updated,
            vec![
                PathBuf::from("/libs/a"),
                PathBuf::from("/libs/b"),
                PathBuf::from("/libs/c"),
            ]
        );
    }

    #[test]
    fn record_recent_library_deduplicates_and_moves_to_front() {
        let existing = vec![PathBuf::from("/libs/a"), PathBuf::from("/libs/b")];
        let updated = record_recent_library(&existing, Path::new("/libs/b"));
        assert_eq!(
            updated,
            vec![PathBuf::from("/libs/b"), PathBuf::from("/libs/a")]
        );
    }

    #[test]
    fn record_recent_library_caps_the_list() {
        let existing: Vec<PathBuf> = (0..10)
            .map(|i| PathBuf::from(format!("/libs/{i}")))
            .collect();
        let updated = record_recent_library(&existing, Path::new("/libs/new"));
        assert_eq!(updated.len(), 8);
        assert_eq!(updated[0], PathBuf::from("/libs/new"));
    }

    #[test]
    fn load_recent_libraries_returns_empty_when_file_is_missing() {
        let scratch = tempfile::tempdir().expect("tempdir");
        let path = scratch.path().join("does-not-exist.json");
        assert!(load_recent_libraries(&path).is_empty());
    }

    #[test]
    fn save_then_load_recent_libraries_round_trips() {
        let scratch = tempfile::tempdir().expect("tempdir");
        let path = scratch.path().join("recent_libraries.json");
        let libraries = vec![PathBuf::from("/libs/a"), PathBuf::from("/libs/b")];
        save_recent_libraries(&path, &libraries).expect("save");
        assert_eq!(load_recent_libraries(&path), libraries);
    }
}
