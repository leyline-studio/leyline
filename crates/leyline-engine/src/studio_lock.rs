//! The lock Studio holds on the library it has open (ADR 0154 §7, step 1).
//!
//! An OS advisory lock (`flock` / `LockFileEx`) on `studio.lock` at the
//! library root. The operating system releases it when the process ends,
//! however it ends, so a crash never leaves a stale lock behind — which is
//! why it is a lock and not a marker file. It protects nothing by itself: it
//! is how another program asks « is Studio editing this library right now? »
//! before deciding to open it read-only.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// The lock file's name, at the library root.
pub const STUDIO_LOCK_FILE: &str = "studio.lock";

/// Studio's hold on a library, released when dropped or when the process
/// ends.
#[derive(Debug)]
pub struct StudioLock {
    _file: File,
}

impl StudioLock {
    /// Takes the lock without waiting. `Ok(None)` when another process
    /// already holds it — another Studio on the same library.
    pub fn acquire(library_root: &Path) -> io::Result<Option<StudioLock>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(library_root.join(STUDIO_LOCK_FILE))?;
        // Fully qualified: on a toolchain newer than the MSRV, std's own
        // `File::try_lock` (Rust 1.89) would win the method lookup.
        match fs4::FileExt::try_lock(&file) {
            Ok(()) => Ok(Some(StudioLock { _file: file })),
            Err(fs4::TryLockError::WouldBlock) => Ok(None),
            Err(fs4::TryLockError::Error(e)) => Err(e),
        }
    }

    /// Whether a Studio holds the library at `library_root` now. Never
    /// blocks and never creates the file: no file means no Studio.
    pub fn is_held(library_root: &Path) -> io::Result<bool> {
        let file = match File::open(library_root.join(STUDIO_LOCK_FILE)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        // A shared probe: two probes at once never see each other.
        match fs4::FileExt::try_lock_shared(&file) {
            Ok(()) => Ok(false),
            Err(fs4::TryLockError::WouldBlock) => Ok(true),
            Err(fs4::TryLockError::Error(e)) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_is_seen_while_held_and_gone_once_dropped() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!StudioLock::is_held(dir.path()).unwrap());

        let lock = StudioLock::acquire(dir.path()).unwrap().unwrap();
        assert!(StudioLock::is_held(dir.path()).unwrap());
        // A second Studio on the same library does not get it.
        assert!(StudioLock::acquire(dir.path()).unwrap().is_none());

        drop(lock);
        assert!(!StudioLock::is_held(dir.path()).unwrap());
        assert!(StudioLock::acquire(dir.path()).unwrap().is_some());
    }
}
