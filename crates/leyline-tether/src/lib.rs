//! USB tethered capture (`docs/adr/0038-tethered-capture.md`,
//! `docs/adr/0087-tethered-capture-bar.md`), backed by libgphoto2 through
//! the `gphoto2` crate — the same "wrap a system C library behind a small
//! Rust surface" shape as `leyline-raw`/LibRaw, `leyline-lens`/Lensfun,
//! `leyline-color`/LittleCMS.
//!
//! This crate only talks to the camera and stages captured files on disk;
//! it does not touch the catalog. The engine (`leyline-engine`) drives a
//! [`TetherSession`] and imports each staged file through its ordinary
//! import core, so a tethered shot becomes a catalog asset exactly like one
//! dropped into a watched folder — no separate "live" data path.
//!
//! A session is a **driver**, not just a receiver (ADR 0087): it reads and
//! writes the four exposure settings a photographer changes between frames,
//! fires the shutter, and pumps a live view. All of that runs on the one
//! thread that owns the camera, because libgphoto2 is not reentrant on a
//! single camera; clients enqueue commands and read state back out of the
//! session's slots.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gphoto2::Context;
use gphoto2::camera::{Camera, CameraEvent};
use gphoto2::widget::Widget;
use leyline_core::{CameraSetting, CameraSettings, TetherSetting};

/// How long one poll waits for a camera event when live view is off.
///
/// Short enough that `TetherSession::stop`/`drop` returns promptly, long
/// enough not to busy-loop libgphoto2's USB transport between shots.
const IDLE_POLL: Duration = Duration::from_millis(500);

/// How long one poll waits for a camera event when live view is on: the
/// loop grabs one preview frame per cycle, so this timeout *is* the frame
/// interval's floor (ADR 0087 §1). 50 ms leaves the feed around 10–15 fps
/// on a body that answers quickly, without spinning when it does not.
const LIVE_POLL: Duration = Duration::from_millis(50);

/// How often settings are re-read from the body even when nothing was set
/// from here — the half of "read from the camera" that a one-shot read
/// misses, because a photographer turning a dial on the body is the normal
/// case in a studio (ADR 0087 §2).
const SETTINGS_REFRESH: Duration = Duration::from_secs(2);

/// A file the camera produced during a tether session, already downloaded
/// to local disk and ready to hand to an importer.
#[derive(Debug, Clone)]
pub struct TetheredFile {
    /// Where the file was staged on disk.
    pub path: PathBuf,
}

/// Where each setting hides in a body's libgphoto2 configuration, most
/// standard name first (ADR 0087 §3).
///
/// The first name the body answers to wins; a body answering to none simply
/// does not expose that setting, which is not an error (a phone or a webcam
/// driven by libgphoto2 has no aperture).
///
/// A free function rather than a method on [`TetherSetting`]: the enum is a
/// `leyline-core` type, shared with clients built without any libgphoto2
/// backend at all, and libgphoto2's spelling is this crate's business.
fn config_keys(setting: TetherSetting) -> &'static [&'static str] {
    match setting {
        TetherSetting::Shutter => &["shutterspeed", "shutterspeed2", "eos-shutterspeed"],
        TetherSetting::Aperture => &["aperture", "f-number", "eos-aperture"],
        TetherSetting::Iso => &["iso", "isospeed", "eos-iso"],
        TetherSetting::WhiteBalance => &["whitebalance", "whitebalancemode"],
    }
}

/// One notification from a running [`TetherSession`].
///
/// Notifications only: everything with a payload bigger than a word is read
/// back out of the session (`settings`, `live_frame`) — ADR 0011, and
/// ADR 0087 §2 for why a live-view frame in particular must not travel as
/// event data.
#[derive(Debug)]
pub enum TetherEvent {
    /// A new file was downloaded and is ready to import.
    Captured(TetheredFile),
    /// [`TetherSession::settings`] changed: re-read it.
    SettingsChanged,
    /// [`TetherSession::live_frame`] holds a newer frame.
    LiveFrame,
    /// A command failed without ending the session — a body refusing a
    /// shutter speed its current mode does not allow, say. Never a
    /// disconnect: the session is still running.
    CommandFailed {
        /// What libgphoto2 said, for the client to show as-is.
        message: String,
    },
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

/// One order for the thread that owns the camera (ADR 0087 §1).
enum Command {
    /// Fire the shutter.
    Capture,
    /// Set one setting to one value the body offers.
    SetSetting(TetherSetting, String),
    /// Start or stop pumping live-view frames.
    SetLiveView(bool),
    /// Re-read the settings now.
    RefreshSettings,
}

/// What the session publishes and clients read back (ADR 0087 §2).
#[derive(Default)]
struct SessionState {
    settings: Mutex<CameraSettings>,
    frame: Mutex<Option<Arc<Vec<u8>>>>,
}

/// A live tether connection to one USB camera.
///
/// V1 scope is a single camera at a time — connecting a second session
/// while one is live is a caller error, not something this crate arbitrates
/// (`docs/adr/0038`). Dropping the session (or calling
/// [`TetherSession::stop`] explicitly) ends the background thread and
/// releases the camera.
pub struct TetherSession {
    stop: Arc<AtomicBool>,
    commands: Sender<Command>,
    state: Arc<SessionState>,
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
    /// Autodetects the first USB camera libgphoto2 finds and starts driving
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
        let state = Arc::new(SessionState::default());
        let state_thread = Arc::clone(&state);
        let (commands, inbox) = channel();
        let staging_dir = staging_dir.to_owned();
        let handle = std::thread::Builder::new()
            .name("leyline-tether".into())
            .spawn(move || {
                run(
                    camera,
                    context,
                    staging_dir,
                    stop_thread,
                    inbox,
                    state_thread,
                    on_event,
                )
            })
            .expect("spawning the tether polling thread");
        Ok(TetherSession {
            stop,
            commands,
            state,
            handle: Some(handle),
        })
    }

    /// What the camera last reported (ADR 0087 §2) — a clone of the
    /// session's slot, never a trip over USB, so a client may call this on
    /// its interface thread as often as it repaints.
    ///
    /// Empty until the first `SettingsChanged` notification.
    pub fn settings(&self) -> CameraSettings {
        lock(&self.state.settings).clone()
    }

    /// The newest live-view frame, as the JPEG bytes the camera produced,
    /// or `None` when live view is off or no frame has arrived yet.
    ///
    /// Cheap to call and safe to miss: a client that skipped ten
    /// `LiveFrame` notifications reads the newest frame here and is
    /// correct, which is what a viewfinder wants (ADR 0087 §2).
    pub fn live_frame(&self) -> Option<Arc<Vec<u8>>> {
        lock(&self.state.frame).clone()
    }

    /// Fires the shutter. Enqueues and returns: the resulting file arrives
    /// as an ordinary [`TetherEvent::Captured`], exactly as if the button
    /// on the body had been pressed.
    pub fn capture(&self) {
        self.send(Command::Capture);
    }

    /// Sets one setting to one of the values the body offers. Enqueues and
    /// returns; success shows up as a `SettingsChanged` with the new value,
    /// refusal as a `CommandFailed`.
    pub fn set_setting(&self, setting: TetherSetting, value: &str) {
        self.send(Command::SetSetting(setting, value.to_owned()));
    }

    /// Starts or stops the live view. Enqueues and returns.
    pub fn set_live_view(&self, on: bool) {
        self.send(Command::SetLiveView(on));
    }

    /// Re-reads the settings from the body now, rather than waiting for the
    /// periodic refresh.
    pub fn refresh_settings(&self) {
        self.send(Command::RefreshSettings);
    }

    /// A command posted to a session whose thread has already ended is
    /// dropped: the `Disconnected` event has told the client the session is
    /// over, and failing louder here would only race that notification.
    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// Ends the session: stops driving the camera and releases it. Blocks
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

/// The command loop itself, run on `TetherSession`'s background thread
/// until `stop` is set or the camera connection ends on its own.
///
/// One cycle: drain the inbox, grab a live frame if live view is on,
/// refresh settings if they are stale, then wait once for a camera event.
/// The camera is touched from nowhere else (ADR 0087 §1).
#[allow(clippy::too_many_arguments)]
fn run(
    camera: Camera,
    context: Context,
    staging_dir: PathBuf,
    stop: Arc<AtomicBool>,
    inbox: Receiver<Command>,
    state: Arc<SessionState>,
    on_event: impl Fn(TetherEvent),
) {
    // A running counter, not the camera's own filename, breaks ties when a
    // camera reuses names across a card format (e.g. `IMG_0001.CR2` twice
    // in one session). The clean name is tried first: what lands in the
    // library should read like the file the camera made.
    let mut sequence: u64 = 0;
    let mut live = false;
    let mut last_refresh = Instant::now();

    publish_settings(&camera, &state, &on_event);

    while !stop.load(Ordering::SeqCst) {
        let mut dirty = false;
        loop {
            match inbox.try_recv() {
                Ok(Command::Capture) => {
                    if let Err(error) = fire(&camera, &staging_dir, &mut sequence, &on_event) {
                        on_event(TetherEvent::CommandFailed {
                            message: error.to_string(),
                        });
                    }
                    dirty = true;
                }
                Ok(Command::SetSetting(setting, value)) => {
                    match apply_setting(&camera, setting, &value) {
                        Ok(()) => dirty = true,
                        Err(error) => on_event(TetherEvent::CommandFailed {
                            message: error.to_string(),
                        }),
                    }
                }
                Ok(Command::SetLiveView(on)) => {
                    live = on;
                    if !on {
                        *lock(&state.frame) = None;
                    }
                }
                Ok(Command::RefreshSettings) => dirty = true,
                // The session itself was dropped: its `Drop` has already
                // set `stop`, and this loop must not keep the camera.
                Err(TryRecvError::Disconnected) => {
                    stop.store(true, Ordering::SeqCst);
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }

        if live {
            match camera.capture_preview().wait() {
                Ok(file) => match file.get_data(&context).wait() {
                    Ok(bytes) => {
                        *lock(&state.frame) = Some(Arc::new(bytes.into_vec()));
                        on_event(TetherEvent::LiveFrame);
                    }
                    // A dropped frame is not a broken session: the next
                    // cycle asks for another one 50 ms later.
                    Err(_) => continue,
                },
                Err(error) => {
                    live = false;
                    *lock(&state.frame) = None;
                    on_event(TetherEvent::CommandFailed {
                        message: error.to_string(),
                    });
                }
            }
        }

        if dirty || last_refresh.elapsed() >= SETTINGS_REFRESH {
            publish_settings(&camera, &state, &on_event);
            last_refresh = Instant::now();
        }

        let timeout = if live { LIVE_POLL } else { IDLE_POLL };
        match camera.wait_event(timeout).wait() {
            Ok(CameraEvent::NewFile(file_path)) => {
                sequence += 1;
                let folder = file_path.folder();
                let name = file_path.name();
                let dest = stage_path(&staging_dir, &name, sequence);
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

/// Fires the shutter, by whichever route the body supports.
///
/// `trigger_capture` is the one that belongs in a tether loop: it returns
/// as soon as the shutter fires and lets the file arrive through the same
/// `NewFile` event a shot taken on the body produces — one code path for
/// both. A body without it falls back to `capture_image`, which hands back
/// the path directly and therefore has to download it here.
fn fire(
    camera: &Camera,
    staging_dir: &Path,
    sequence: &mut u64,
    on_event: &impl Fn(TetherEvent),
) -> std::result::Result<(), gphoto2::Error> {
    if camera.abilities().camera_operations().trigger_capture() {
        return camera.trigger_capture().wait();
    }
    let path = camera.capture_image().wait()?;
    *sequence += 1;
    let name = path.name();
    let dest = stage_path(staging_dir, &name, *sequence);
    camera
        .fs()
        .download_to(&path.folder(), &name, &dest)
        .wait()?;
    on_event(TetherEvent::Captured(TetheredFile { path: dest }));
    Ok(())
}

/// Where to stage one downloaded file: the camera's own name when it is
/// free, and the name plus the session's counter when it is not.
fn stage_path(staging_dir: &Path, name: &str, sequence: u64) -> PathBuf {
    let clean = staging_dir.join(name);
    if !clean.exists() {
        return clean;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (name, String::new()),
    };
    staging_dir.join(format!("{stem}-{sequence:04}{extension}"))
}

/// Sets one setting on the body, probing the config names it might live
/// under (ADR 0087 §3).
fn apply_setting(
    camera: &Camera,
    setting: TetherSetting,
    value: &str,
) -> std::result::Result<(), gphoto2::Error> {
    let Some(widget) = find_widget(camera, setting) else {
        return Err(gphoto2::Error::from(format!(
            "this camera does not expose {}",
            setting.as_str()
        )));
    };
    match &widget {
        Widget::Radio(radio) => radio.set_choice(value)?,
        Widget::Text(text) => text.set_value(value)?,
        _ => {
            return Err(gphoto2::Error::from(format!(
                "{} is not a settable value on this camera",
                setting.as_str()
            )));
        }
    }
    camera.set_config(&widget).wait()
}

/// Reads the body's settings and publishes them, notifying only when they
/// actually changed — the periodic refresh runs every 2 s and a bar that
/// repainted on every tick would be a bar that flickers for nothing.
fn publish_settings(camera: &Camera, state: &SessionState, on_event: &impl Fn(TetherEvent)) {
    let fresh = read_settings(camera);
    let mut slot = lock(&state.settings);
    if *slot == fresh {
        return;
    }
    *slot = fresh;
    drop(slot);
    on_event(TetherEvent::SettingsChanged);
}

/// Everything the bar shows, read from the body in one pass.
fn read_settings(camera: &Camera) -> CameraSettings {
    let abilities = camera.abilities();
    let operations = abilities.camera_operations();
    let mut settings = CameraSettings {
        model: abilities.model().into_owned(),
        can_capture: operations.trigger_capture() || operations.capture_image(),
        can_live_view: operations.capture_preview(),
        settings: Vec::new(),
    };
    for setting in TetherSetting::ALL {
        if let Some(widget) = find_widget(camera, setting)
            && let Some(read) = read_widget(&widget)
        {
            settings.settings.push((setting, read));
        }
    }
    settings
}

/// The first config widget the body answers to for this setting.
fn find_widget(camera: &Camera, setting: TetherSetting) -> Option<Widget> {
    config_keys(setting)
        .iter()
        .find_map(|key| camera.config_key::<Widget>(key).wait().ok())
}

/// One config widget as the bar understands it. Anything that is neither a
/// list of choices nor a plain value is not a control this bar can draw,
/// and is reported as absent rather than as a broken one.
fn read_widget(widget: &Widget) -> Option<CameraSetting> {
    match widget {
        Widget::Radio(radio) => Some(CameraSetting {
            value: radio.choice(),
            choices: radio.choices_iter().collect(),
            readonly: radio.readonly(),
        }),
        Widget::Text(text) => Some(CameraSetting {
            value: text.value(),
            choices: Vec::new(),
            readonly: text.readonly(),
        }),
        _ => None,
    }
}

/// A poisoned session mutex is not worth propagating: the data behind it is
/// the last thing the camera said, and a panic while holding it leaves that
/// still readable and still true.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    /// Every setting must have at least one config name to probe, and no
    /// two settings may claim the same one — a shared key would mean the
    /// bar drove one control from another's widget.
    #[test]
    fn every_setting_probes_its_own_keys() {
        let mut seen: Vec<&str> = Vec::new();
        for setting in TetherSetting::ALL {
            assert!(
                !config_keys(setting).is_empty(),
                "{setting:?} probes nothing"
            );
            for key in config_keys(setting) {
                assert!(!seen.contains(key), "{key} is claimed twice");
                seen.push(key);
            }
        }
    }

    /// Two shots the camera named identically (a card reformatted
    /// mid-session) must stage as two files, not one overwriting the other.
    #[test]
    fn a_reused_camera_filename_stages_beside_the_first() {
        let staging = std::env::temp_dir().join("leyline-tether-stage-path");
        std::fs::create_dir_all(&staging).expect("staging dir");
        let first = stage_path(&staging, "IMG_0001.CR2", 1);
        assert_eq!(first.file_name().unwrap(), "IMG_0001.CR2");
        std::fs::write(&first, b"raw").expect("first shot");
        let second = stage_path(&staging, "IMG_0001.CR2", 2);
        assert_eq!(second.file_name().unwrap(), "IMG_0001-0002.CR2");
        std::fs::remove_file(&first).expect("cleanup");
    }

    /// An extensionless name keeps its shape when it has to be suffixed.
    #[test]
    fn staging_a_nameless_extension_does_not_invent_a_dot() {
        let staging = std::env::temp_dir().join("leyline-tether-stage-noext");
        std::fs::create_dir_all(&staging).expect("staging dir");
        let first = staging.join("CAPTURE");
        std::fs::write(&first, b"raw").expect("first shot");
        let second = stage_path(&staging, "CAPTURE", 7);
        assert_eq!(second.file_name().unwrap(), "CAPTURE-0007");
        std::fs::remove_file(&first).expect("cleanup");
    }
}
