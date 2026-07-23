//! USB tethered capture (`docs/adr/0038-tethered-capture.md`), backed by
//! libgphoto2 through the `gphoto2` crate — the same "wrap a system C
//! library behind a small Rust surface" shape as `leyline-raw`/LibRaw,
//! `leyline-lens`/Lensfun, `leyline-color`/LittleCMS.
//!
//! This crate only talks to the camera and stages captured files on disk;
//! it does not touch the catalog. The engine (`leyline-engine`) drives a
//! [`TetherSession`] and imports each staged file through its ordinary
//! import core, so a tethered shot becomes a catalog asset exactly like one
//! dropped into a watched folder — no separate "live" data path.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gphoto2::Context;
use gphoto2::camera::{Camera, CameraEvent};

/// How long one poll waits for a camera event before checking `stop` again.
///
/// Short enough that `TetherSession::stop`/`drop` returns promptly, long
/// enough not to busy-loop libgphoto2's USB transport between shots.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// A file the camera produced during a tether session, already downloaded
/// to local disk and ready to hand to an importer.
#[derive(Debug, Clone)]
pub struct TetheredFile {
    /// Where the file was staged on disk.
    pub path: PathBuf,
}

/// One notification from a running [`TetherSession`].
#[derive(Debug)]
pub enum TetherEvent {
    /// A new file was downloaded and is ready to import.
    Captured(TetheredFile),
    /// The session ended: unplugged, powered off, or a transport error.
    /// The background thread has already stopped by the time this fires —
    /// `None` means a clean [`TetherSession::stop`], `Some` carries the
    /// libgphoto2 error that ended it unexpectedly.
    Disconnected(Option<String>),
}

/// What can go wrong connecting to a camera.
#[derive(Debug, thiserror::Error)]
pub enum TetherError {
    /// libgphoto2 found no USB camera to autodetect.
    #[error("no camera detected over USB")]
    NoCamera,
    /// libgphoto2 itself reported an error.
    #[error("libgphoto2 error: {0}")]
    Gphoto2(String),
    /// Staging a downloaded file failed.
    #[error("i/o error staging a captured file: {0}")]
    Io(#[from] std::io::Error),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, TetherError>;

/// A live tether connection to one USB camera.
///
/// V1 scope is a single camera at a time — connecting a second session
/// while one is live is a caller error, not something this crate arbitrates
/// (`docs/adr/0038`). Dropping the session (or calling
/// [`TetherSession::stop`] explicitly) ends the background polling thread
/// and releases the camera.
pub struct TetherSession {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for TetherSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TetherSession")
            .field("stopped", &self.stop.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl TetherSession {
    /// Autodetects the first USB camera libgphoto2 finds and starts polling
    /// it on a background thread. Each captured file is downloaded into
    /// `staging_dir` (created if missing) and reported through `on_event`.
    ///
    /// `on_event` runs on the background thread: keep it non-blocking — its
    /// job is handing the path to the caller's own import call and
    /// returning, not doing the import itself.
    pub fn connect(
        staging_dir: &Path,
        on_event: impl Fn(TetherEvent) + Send + 'static,
    ) -> Result<TetherSession> {
        std::fs::create_dir_all(staging_dir)?;
        let context = Context::new().map_err(gphoto_err)?;
        let camera = context
            .autodetect_camera()
            .wait()
            .map_err(|_| TetherError::NoCamera)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let staging_dir = staging_dir.to_owned();
        let handle = std::thread::Builder::new()
            .name("leyline-tether".into())
            .spawn(move || run(camera, staging_dir, stop_thread, on_event))
            .expect("spawning the tether polling thread");
        Ok(TetherSession {
            stop,
            handle: Some(handle),
        })
    }

    /// Ends the session: stops polling and releases the camera. Blocks
    /// until the background thread has actually exited (at most one poll
    /// interval), and its final `Disconnected(None)` has already fired by
    /// the time this returns.
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

impl Drop for TetherSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.join();
    }
}

/// The polling loop itself, run on `TetherSession`'s background thread
/// until `stop` is set or the camera connection ends on its own.
fn run(
    camera: Camera,
    staging_dir: PathBuf,
    stop: Arc<AtomicBool>,
    on_event: impl Fn(TetherEvent),
) {
    // A running counter, not the camera's own filename, avoids collisions
    // when a camera reuses names across a card format (e.g. `IMG_0001.CR2`
    // twice in one session).
    let mut sequence: u64 = 0;
    while !stop.load(Ordering::SeqCst) {
        match camera.wait_event(POLL_INTERVAL).wait() {
            Ok(CameraEvent::NewFile(file_path)) => {
                sequence += 1;
                let folder = file_path.folder();
                let name = file_path.name();
                let dest = staging_dir.join(format!("{sequence:06}-{name}"));
                match camera.fs().download_to(&folder, &name, &dest).wait() {
                    Ok(_) => on_event(TetherEvent::Captured(TetheredFile { path: dest })),
                    Err(error) => {
                        on_event(TetherEvent::Disconnected(Some(error.to_string())));
                        return;
                    }
                }
            }
            Ok(_) => continue,
            Err(error) => {
                on_event(TetherEvent::Disconnected(Some(error.to_string())));
                return;
            }
        }
    }
    on_event(TetherEvent::Disconnected(None));
}

fn gphoto_err(error: gphoto2::Error) -> TetherError {
    TetherError::Gphoto2(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No USB camera is attached in CI or a plain dev machine — libgphoto2
    /// itself is the only thing this crate can exercise without hardware,
    /// so this pins the one behavior that's always true offline: a clean
    /// [`TetherError::NoCamera`], never a panic or a hang.
    #[test]
    fn connect_without_a_camera_fails_cleanly() {
        let staging = std::env::temp_dir().join("leyline-tether-test-staging");
        let result = TetherSession::connect(&staging, |_event| {});
        assert!(matches!(result, Err(TetherError::NoCamera)));
    }
}
