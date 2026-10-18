//! `last-panic.txt`: what the panic hook leaves behind for the next launch
//! (ADR 0162 §1).
//!
//! One file in the config directory, overwritten by each panic: the last one
//! is the one being reported, and a history of them would be a log by another
//! name — a decision ADR 0162 leaves to be taken on its own.

use std::path::{Path, PathBuf};

/// The file's name inside the config directory.
const FILE_NAME: &str = "last-panic.txt";

/// The five lines `Help ▸ About ▸ Build` shows.
///
/// One function for the dialog and for the file, so a report built from
/// either says the same thing (ADR 0162 §1). The decoder line is read at run
/// time, unlike the four build-time constants around it, and that difference
/// is the point: LibRaw is linked dynamically (ADR 0004), so it is a property
/// of this machine rather than of this build. `docs/pipeline.md` §5.1 counts
/// it among the terms of the bit-for-bit promise (ADR 0086), which makes it
/// the one fact about a render a user cannot otherwise discover. Every
/// build-time field degrades to "inconnu" in `build.rs` rather than failing a
/// build made from a source tarball with no git checkout.
pub(crate) fn build_details() -> String {
    format!(
        "Version {}\nCommit {}\nTarget {}\n{}\nLibRaw {}",
        env!("CARGO_PKG_VERSION"),
        env!("LEYLINE_COMMIT"),
        env!("LEYLINE_TARGET"),
        env!("LEYLINE_RUSTC"),
        leyline_sdk::decoder_version(),
    )
}

/// Where the file lives, or `None` when no config directory can be resolved —
/// in which case the panic is still printed to stderr, and nothing else.
pub(crate) fn last_panic_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "Leyline").map(|dirs| dirs.config_dir().join(FILE_NAME))
}

/// The file a previous session left behind, if there is one: what the notice
/// at startup looks for (ADR 0162 §2).
pub(crate) fn left_by_last_session() -> Option<PathBuf> {
    last_panic_path().filter(|path| path.is_file())
}

/// Deletes the file — on Dismiss, or once a report carrying it has been
/// written (ADR 0162 §3). Best-effort: a file that stays only means the
/// notice comes back at the next launch.
pub(crate) fn forget(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Installs the panic hook so that, besides stderr, each panic overwrites
/// `last-panic.txt`. Studio only: the CLI's user is at the terminal the
/// backtrace goes to.
pub(crate) fn install() {
    let path = last_panic_path();
    // Built now, while nothing is wrong, rather than inside the hook.
    let build = build_details();
    leyline_sdk::install_panic_hook_recording(move |panic| {
        if let Some(path) = &path {
            write(path, &build, panic);
        }
    });
}

/// Writes the file. Best-effort: the hook runs while something is already
/// wrong and must never make it worse (ADR 0122 §4), so every failure is
/// ignored. No clock in the text — the file's own date says when.
fn write(path: &Path, build: &str, panic: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, format!("{build}\n\n{panic}\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_holds_the_build_then_the_panic_and_is_overwritten() {
        let config = tempfile::tempdir().expect("temporary directory");
        let path = config.path().join("leyline").join(FILE_NAME);

        write(
            &path,
            "Version 9.9.9",
            "thread 'main' panicked at a.rs:1:1:\nfirst",
        );
        write(
            &path,
            "Version 9.9.9",
            "thread 'main' panicked at b.rs:2:2:\nsecond",
        );

        let text = std::fs::read_to_string(&path).expect("the file was written");
        assert_eq!(
            text,
            "Version 9.9.9\n\nthread 'main' panicked at b.rs:2:2:\nsecond\n"
        );
    }

    #[test]
    fn a_write_that_cannot_happen_is_silent() {
        let config = tempfile::tempdir().expect("temporary directory");
        // A regular file where the parent directory should be: both the
        // directory creation and the write fail, and neither may panic.
        let blocker = config.path().join("blocker");
        std::fs::write(&blocker, "").expect("blocker file");
        write(&blocker.join(FILE_NAME), "build", "panic");
        assert!(!blocker.join(FILE_NAME).exists());
    }

    #[test]
    fn the_build_block_has_the_five_about_lines() {
        let build = build_details();
        assert_eq!(build.lines().count(), 5);
        assert!(build.starts_with("Version "));
        assert!(
            build
                .lines()
                .last()
                .is_some_and(|line| line.starts_with("LibRaw "))
        );
    }
}
