//! Watched-folder auto-import (`docs/adr/0039-watched-folder-import.md`).
//!
//! Mirrors `leyline_tether`'s background-thread shape (ADR 0038): a
//! [`WatchSession`] watches a folder on a background thread and reports each
//! settled file through `on_event`, non-blocking, exactly like
//! `TetherSession::connect`. `Library::watch_start` (`library.rs`) then
//! imports each one through the ordinary import core, so a file dropped
//! into a watched folder becomes a catalog asset exactly like an explicit
//! import or a tethered shot — no separate "watched" asset kind.
//!
//! Files are reported one at a time as they settle, not batched: the caller
//! is expected to import them individually, keeping each catalog-lock
//! window short (`docs/engine-api.md` §3.3 covers why that matters — the
//! catalog mutex is also on interactive develop-mode work's path). See the
//! `import` bench group in `leyline-engine/benches/import.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// How long a file's size must stay unchanged before it's considered
/// settled and reported — long enough to ride out a slow card-reader or
/// network copy, short enough that a single dropped-in photo does not sit
/// around for minutes before it shows up in the catalog.
const STABILITY_WINDOW: Duration = Duration::from_secs(2);

/// How often the settle check runs: bounds both the worst-case detection
/// latency and how promptly [`WatchSession::stop`] returns.
const TICK_INTERVAL: Duration = Duration::from_millis(500);

/// A file the watcher considers ready to import: its size has not changed
/// for at least [`STABILITY_WINDOW`].
#[derive(Debug, Clone)]
pub struct WatchedFile {
    /// Where the file sits on disk, inside the watched folder.
    pub path: PathBuf,
}

/// One notification from a running [`WatchSession`].
#[derive(Debug)]
pub enum WatchSessionEvent {
    /// A new file settled and is ready to import.
    Ready(WatchedFile),
    /// The session ended: a clean [`WatchSession::stop`] (`None`), or the
    /// underlying OS watcher failed unexpectedly (`Some`). The background
    /// thread has already stopped by the time this fires.
    Stopped(Option<String>),
}

/// What can go wrong starting a watch session.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// The OS-level filesystem watcher failed to start or attach.
    #[error("filesystem watcher error: {0}")]
    Notify(String),
}

/// Result alias for this module.
pub type Result<T> = std::result::Result<T, WatchError>;

/// A live watch session over one folder.
///
/// V1 scope is a single watched folder at a time per library, same
/// one-session contract as `leyline_tether::TetherSession` (`docs/adr/0039`,
/// `docs/adr/0038`). Dropping the session (or calling
/// [`WatchSession::stop`] explicitly) stops the background thread and
/// releases the OS watcher.
pub struct WatchSession {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    // Kept alive for the session's duration: dropping it stops delivery of
    // filesystem events to `run`'s channel.
    _watcher: RecommendedWatcher,
}

impl std::fmt::Debug for WatchSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchSession")
            .field("stopped", &self.stop.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl WatchSession {
    /// Starts watching `folder` (recursively) on a background thread.
    /// `on_event` runs on that thread: keep it non-blocking, same
    /// contract as `TetherSession::connect`.
    pub fn watch(
        folder: &Path,
        on_event: impl Fn(WatchSessionEvent) + Send + 'static,
    ) -> Result<WatchSession> {
        let (tx, rx) = channel::<notify::Result<NotifyEvent>>();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })
        .map_err(notify_err)?;
        watcher
            .watch(folder, RecursiveMode::Recursive)
            .map_err(notify_err)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("leyline-watch".into())
            .spawn(move || run(rx, stop_thread, on_event))
            .expect("spawning the watch polling thread");
        Ok(WatchSession {
            stop,
            handle: Some(handle),
            _watcher: watcher,
        })
    }

    /// Ends the session: stops watching and releases the OS watcher. Blocks
    /// until the background thread has actually exited (at most one tick
    /// interval), and its final `Stopped(None)` has already fired by the
    /// time this returns.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.join();
    }

    fn join(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WatchSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.join();
    }
}

/// One candidate file being watched for stability: its size the last time
/// it was checked, and when that size was last seen to change.
type Pending = HashMap<PathBuf, (u64, Instant)>;

/// The polling loop, run on `WatchSession`'s background thread until `stop`
/// is set. Debounces raw filesystem events into settled files: a create or
/// modify event (re)starts tracking a path, and each tick promotes any
/// tracked path whose size has been unchanged for `STABILITY_WINDOW` to a
/// `Ready` event.
fn run(
    rx: std::sync::mpsc::Receiver<notify::Result<NotifyEvent>>,
    stop: Arc<AtomicBool>,
    on_event: impl Fn(WatchSessionEvent),
) {
    let mut pending: Pending = HashMap::new();
    while !stop.load(Ordering::SeqCst) {
        match rx.recv_timeout(TICK_INTERVAL) {
            Ok(Ok(event)) => track(event, &mut pending),
            // A transient OS-level error on one event does not end the
            // session — same best-effort stance as a per-file import skip.
            Ok(Err(_)) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                on_event(WatchSessionEvent::Stopped(Some(
                    "the filesystem watcher stopped sending events".to_owned(),
                )));
                return;
            }
        }
        settle(&mut pending, &on_event);
    }
    on_event(WatchSessionEvent::Stopped(None));
}

/// Starts or refreshes tracking for every file path a create/modify event
/// touched. Only recognized media extensions are tracked — sidecar files,
/// temp files, and anything else `leyline_engine::import` would skip never
/// enters `pending` at all.
fn track(event: NotifyEvent, pending: &mut Pending) {
    if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
        return;
    }
    for path in event.paths {
        let recognized = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .is_some_and(|ext| crate::import::media_type(&ext).is_some());
        if !recognized {
            continue;
        }
        if let Ok(metadata) = std::fs::metadata(&path) {
            if metadata.is_file() {
                pending.insert(path, (metadata.len(), Instant::now()));
            }
        }
    }
}

/// Checks every tracked file against its last known size, promoting the
/// ones that have been stable for `STABILITY_WINDOW` to `Ready` and
/// dropping the ones that vanished (a rename, a cancelled copy).
fn settle(pending: &mut Pending, on_event: &impl Fn(WatchSessionEvent)) {
    let now = Instant::now();
    let mut ready = Vec::new();
    pending.retain(
        |path, (last_size, last_change)| match std::fs::metadata(path) {
            Ok(metadata) if metadata.len() == *last_size => {
                if now.duration_since(*last_change) >= STABILITY_WINDOW {
                    ready.push(path.clone());
                    false
                } else {
                    true
                }
            }
            Ok(metadata) => {
                *last_size = metadata.len();
                *last_change = now;
                true
            }
            Err(_) => false,
        },
    );
    for path in ready {
        on_event(WatchSessionEvent::Ready(WatchedFile { path }));
    }
}

fn notify_err(error: notify::Error) -> WatchError {
    WatchError::Notify(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel as std_channel;

    /// A file dropped into the watched folder is reported once it has
    /// stopped changing size — the end-to-end path a real card-reader copy
    /// or `cp` into the folder exercises.
    #[test]
    fn a_new_file_is_reported_once_settled() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std_channel();
        let session = WatchSession::watch(dir.path(), move |event| {
            let _ = tx.send(event);
        })
        .unwrap();

        std::fs::write(dir.path().join("shot.jpg"), b"not a real jpeg, just bytes").unwrap();

        let event = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a Ready event within the stability window");
        match event {
            WatchSessionEvent::Ready(file) => {
                // Both sides are canonicalized: on macOS the temp directory
                // lives under `/var`, which is a symlink to `/private/var`,
                // and `notify` reports the resolved path while `TempDir`
                // hands back the symlinked one. Comparing them raw is a
                // platform difference, not a behavior.
                assert_eq!(
                    file.path.canonicalize().unwrap(),
                    dir.path().join("shot.jpg").canonicalize().unwrap()
                );
            }
            other => panic!("expected Ready, got {other:?}"),
        }

        session.stop();
    }

    /// Files with an extension the import core does not recognize (a
    /// sidecar, a stray text file) are never reported.
    #[test]
    fn unrecognized_extensions_are_never_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std_channel();
        let session = WatchSession::watch(dir.path(), move |event| {
            let _ = tx.send(event);
        })
        .unwrap();

        std::fs::write(dir.path().join("notes.txt"), b"hello").unwrap();

        let outcome = rx.recv_timeout(Duration::from_secs(3));
        assert!(
            outcome.is_err(),
            "a .txt file must never produce a Ready event"
        );

        session.stop();
    }

    /// `stop` returns only after the background thread's final `Stopped`
    /// event has already fired — no race for a caller that tears down
    /// state right after calling it.
    #[test]
    fn stop_fires_a_clean_stopped_event_first() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std_channel();
        let session = WatchSession::watch(dir.path(), move |event| {
            let _ = tx.send(event);
        })
        .unwrap();
        session.stop();

        match rx.try_recv() {
            Ok(WatchSessionEvent::Stopped(None)) => {}
            other => panic!("expected a clean Stopped(None), got {other:?}"),
        }
    }
}
