//! Files arriving: tethered capture and watched folders.

use super::*;

impl Library {
    /// Connects to the first USB camera libgphoto2 finds and starts a
    /// tether session (`docs/adr/0038-tethered-capture.md`,
    /// `docs/adr/0087-tethered-capture-bar.md`): every shot the camera
    /// reports from here on is downloaded and imported automatically — a
    /// tethered shot is not a distinct kind of asset, just a different
    /// import source, so it lands in the catalog exactly like a file
    /// dropped into a watched folder.
    ///
    /// `options` decides what is settled before the session opens: the
    /// folder under `Photos/` its shots are filed in, and the develop
    /// preset each one arrives already developed with (ADR 0087 §4–5).
    ///
    /// Emits `Event::TetherConnected` on success, then — per captured shot
    /// — one `Event::AssetsAdded` (the same event a normal import fires),
    /// and `Event::TetherSettingsChanged`/`TetherLiveFrame` as the body's
    /// state and live view move, until `Event::TetherDisconnected` ends the
    /// session (`tether_disconnect`, an unplug, or a transport error).
    ///
    /// Refuses a second session while one is already open: one camera at a
    /// time per library in V1 (`docs/adr/0038`).
    #[cfg(feature = "tether")]
    pub fn tether_connect(&self, options: &crate::tether::TetherOptions) -> Result<()> {
        // Validated before the camera is touched: a session name that
        // cannot become a folder is the caller's mistake, and finding it
        // out after opening the USB connection would leave a live session
        // to tear down for nothing.
        let session = crate::tether::session_folder(&options.session)?;
        let mut slot = lock(&self.inner.tether);
        if slot.is_some() {
            return Err(LeylineError::Tether(
                "a tether session is already open on this library".to_owned(),
            ));
        }
        let staging = self.inner.root.join("Cache").join("Tether").join(&session);
        *lock(&self.inner.tether_options) = crate::tether::TetherOptions {
            session,
            preset: options.preset,
        };
        let library = self.clone();
        let session = leyline_tether::TetherSession::connect(&staging, move |event| {
            library.handle_tether_event(event);
        })
        .map_err(|error| LeylineError::Tether(error.to_string()))?;
        *slot = Some(session);
        drop(slot);
        self.emit(Event::TetherConnected);
        Ok(())
    }

    /// Same call, in a build without the `tether` feature: reports that this
    /// build has no libgphoto2 backend.
    ///
    /// The message names the cause rather than saying only "failed", because
    /// a bare connection error would send the user unplugging and replugging
    /// a camera that was never the problem.
    #[cfg(not(feature = "tether"))]
    pub fn tether_connect(&self, _options: &crate::tether::TetherOptions) -> Result<()> {
        Err(LeylineError::Tether(
            "tethered capture is not available in this build of Leyline: no \
             libgphoto2 backend is packaged for this platform yet"
                .to_owned(),
        ))
    }

    /// Ends the running tether session, if any (`docs/adr/0038`) — a no-op
    /// when none is open. Blocks briefly (at most one poll interval) for
    /// the background thread to actually stop; by the time this returns,
    /// `Event::TetherDisconnected { reason: None }` has already fired.
    #[cfg(feature = "tether")]
    pub fn tether_disconnect(&self) {
        // Deliberately two statements, not `if let Some(s) =
        // lock(..).take() { s.stop() }`: that form is a real deadlock, not
        // just a style nit — a `MutexGuard` that is the scrutinee of an
        // `if let` lives for the *whole* block (Rust's temporary lifetime
        // extension), so the lock would still be held while `stop()`
        // blocks joining the session's background thread — and that
        // thread's own `Disconnected` handler needs the very same lock to
        // report itself gone. Splitting the `take()` into its own `let`
        // drops the guard before `stop()` runs.
        let session = lock(&self.inner.tether).take();
        if let Some(session) = session {
            session.stop();
        }
    }

    /// Same call, in a build without the `tether` feature: nothing can be
    /// open, so this is the no-op it already is when no session is running.
    #[cfg(not(feature = "tether"))]
    pub fn tether_disconnect(&self) {}

    /// What the connected body last reported: its model, what it can do,
    /// and the four exposure settings with the values it will accept
    /// (ADR 0087 §2–3).
    ///
    /// A read of the session's own slot, never a trip over USB — cheap
    /// enough for an interface thread to call on every repaint. Default
    /// (an empty model, no settings) when no session is open.
    #[cfg(feature = "tether")]
    pub fn tether_settings(&self) -> CameraSettings {
        lock(&self.inner.tether)
            .as_ref()
            .map(leyline_tether::TetherSession::settings)
            .unwrap_or_default()
    }

    /// Same call, in a build without the `tether` feature: no session can
    /// be open, so nothing is connected to report.
    #[cfg(not(feature = "tether"))]
    pub fn tether_settings(&self) -> CameraSettings {
        CameraSettings::default()
    }

    /// The newest live-view frame, as the JPEG bytes the camera produced,
    /// or `None` when live view is off (ADR 0087 §6).
    ///
    /// Frames never reach the catalog and are never written to the cache: a
    /// live view is a viewfinder, and the catalog learns of a frame only if
    /// the shutter actually fires.
    #[cfg(feature = "tether")]
    pub fn tether_live_frame(&self) -> Option<Arc<Vec<u8>>> {
        lock(&self.inner.tether)
            .as_ref()
            .and_then(leyline_tether::TetherSession::live_frame)
    }

    /// Same call, in a build without the `tether` feature.
    #[cfg(not(feature = "tether"))]
    pub fn tether_live_frame(&self) -> Option<Arc<Vec<u8>>> {
        None
    }

    /// Fires the shutter of the connected body (ADR 0087 §1).
    ///
    /// Enqueues and returns: the shot arrives as an ordinary
    /// `Event::AssetsAdded`, exactly as if the button on the camera had
    /// been pressed. A no-op when no session is open.
    #[cfg(feature = "tether")]
    pub fn tether_capture(&self) {
        if let Some(session) = lock(&self.inner.tether).as_ref() {
            session.capture();
        }
    }

    /// Same call, in a build without the `tether` feature.
    #[cfg(not(feature = "tether"))]
    pub fn tether_capture(&self) {}

    /// Sets one exposure setting on the connected body to one of the values
    /// it offers (ADR 0087 §3).
    ///
    /// Enqueues and returns: success shows up as
    /// `Event::TetherSettingsChanged` carrying the new value in
    /// [`Library::tether_settings`], refusal as
    /// `Event::TetherCommandFailed` — which is not a disconnect.
    #[cfg(feature = "tether")]
    pub fn tether_set(&self, setting: TetherSetting, value: &str) {
        if let Some(session) = lock(&self.inner.tether).as_ref() {
            session.set_setting(setting, value);
        }
    }

    /// Same call, in a build without the `tether` feature.
    #[cfg(not(feature = "tether"))]
    pub fn tether_set(&self, _setting: TetherSetting, _value: &str) {}

    /// Changes the develop preset the running session applies to arriving
    /// shots, or clears it with `None` (ADR 0087 §5).
    ///
    /// Mid-session because the setup changes mid-session: a new background
    /// goes up, and the next frame should already be developed for it.
    /// Shots already imported are untouched — this decides what the *next*
    /// arrival gets, exactly like changing a preset decides what its next
    /// application writes.
    #[cfg(feature = "tether")]
    pub fn tether_set_preset(&self, preset: Option<PresetId>) {
        lock(&self.inner.tether_options).preset = preset;
    }

    /// Same call, in a build without the `tether` feature: no session can
    /// be open, so there is nothing to re-aim.
    #[cfg(not(feature = "tether"))]
    pub fn tether_set_preset(&self, _preset: Option<PresetId>) {}

    /// Starts or stops the body's live view (ADR 0087 §6). Enqueues and
    /// returns; frames arrive as `Event::TetherLiveFrame`.
    #[cfg(feature = "tether")]
    pub fn tether_live_view(&self, on: bool) {
        if let Some(session) = lock(&self.inner.tether).as_ref() {
            session.set_live_view(on);
        }
    }

    /// Same call, in a build without the `tether` feature.
    #[cfg(not(feature = "tether"))]
    pub fn tether_live_view(&self, _on: bool) {}

    /// Turns one `leyline_tether::TetherEvent` into catalog state and an
    /// engine event (`docs/adr/0038`, `docs/adr/0087`). Runs on the tether
    /// session's own background thread.
    #[cfg(feature = "tether")]
    pub(super) fn handle_tether_event(&self, event: leyline_tether::TetherEvent) {
        match event {
            leyline_tether::TetherEvent::Captured(file) => self.import_tethered_shot(&file.path),
            leyline_tether::TetherEvent::SettingsChanged => {
                self.emit(Event::TetherSettingsChanged);
            }
            leyline_tether::TetherEvent::LiveFrame => self.emit(Event::TetherLiveFrame),
            leyline_tether::TetherEvent::CommandFailed { message } => {
                self.emit(Event::TetherCommandFailed { message });
            }
            leyline_tether::TetherEvent::Disconnected(reason) => {
                lock(&self.inner.tether).take();
                self.emit(Event::TetherDisconnected { reason });
            }
        }
    }

    /// Imports one shot the camera just produced, files it under the
    /// session's folder and develops it with the session's preset
    /// (ADR 0087 §4–5).
    #[cfg(feature = "tether")]
    pub(super) fn import_tethered_shot(&self, staged: &Path) {
        let options = lock(&self.inner.tether_options).clone();
        let staging_root = self.inner.root.join("Cache").join("Tether");
        let Some(name) = staged.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        // The import core mirrors a file's position relative to the source
        // directory (`import::copy_into_photos`), so importing
        // `Cache/Tether/<session>/<name>` *from* `Cache/Tether` files the
        // shot at `Photos/<session>/<name>` — the session folder falls out
        // of the existing copy rule instead of needing a second one.
        let destination = self.inner.root.join("Photos").join(&options.session);
        let free = crate::tether::free_name(&destination, name);
        let staged = if free == name {
            staged.to_path_buf()
        } else {
            let renamed = staged.with_file_name(&free);
            if std::fs::rename(staged, &renamed).is_err() {
                return;
            }
            renamed
        };

        let import_options = ImportOptions {
            copy_files: true,
            recursive: false,
            pair_companions: true,
            thumbnails: true,
        };
        // A failed import of one captured shot (e.g. an undecodable file)
        // is dropped rather than surfaced as a disconnect — the same
        // best-effort stance the import core already takes for a thumbnail
        // render failing after a successful import.
        if let Ok(report) = self.import_files(
            &staging_root,
            std::slice::from_ref(&staged),
            &import_options,
            |_, _| {},
        ) {
            let versions: Vec<VersionId> = report
                .imported
                .iter()
                .map(|imported| imported.registered.version)
                .collect();
            // Before `AssetsAdded`, deliberately (ADR 0087 §5): a client
            // told about the photo first would render it neutral and
            // settle a moment later, and a tethered session exists to
            // judge the shot as it is taken.
            if let Some(preset) = options.preset
                && !versions.is_empty()
            {
                let _ = self.apply_preset(preset, &versions, |_, _| {});
            }
            let asset_ids: Vec<AssetId> = report
                .imported
                .iter()
                .map(|imported| imported.registered.asset)
                .collect();
            if !asset_ids.is_empty() {
                self.emit(Event::AssetsAdded { asset_ids });
            }
        }
        // The import core already copied the bytes into `Photos/`
        // (`copy_files: true` above); leaving the staged copy behind would
        // grow `Cache/Tether/` without bound for the life of the session.
        let _ = std::fs::remove_file(&staged);
    }

    /// Starts watching `folder` for new files (`docs/adr/0039-watched-
    /// folder-import.md`): every file that settles there from now on is
    /// imported automatically — a watched-folder import is not a distinct
    /// kind of asset, just a different import source, so it lands in the
    /// catalog exactly like an explicit import or a tethered shot. Emits
    /// `Event::WatchStarted` on success, then one `Event::AssetsAdded` per
    /// imported file, then `Event::WatchStopped` once the session ends
    /// (`watch_stop`, or the OS watcher failing).
    ///
    /// Each settled file is imported on its own — never batched — the same
    /// short-catalog-lock shape `handle_tether_event` uses, so a folder
    /// receiving many files in a row never holds the catalog mutex for
    /// longer than one file's import, and interactive develop-mode work
    /// sharing that mutex is never kept waiting behind the whole batch.
    ///
    /// Refuses a second session while one is already open: one watched
    /// folder at a time per library in V1, same contract as
    /// `tether_connect`.
    pub fn watch_start(&self, folder: &Path) -> Result<()> {
        let mut slot = lock(&self.inner.watch);
        if slot.is_some() {
            return Err(LeylineError::Watch(
                "a watched folder is already active on this library".to_owned(),
            ));
        }
        let library = self.clone();
        let session = crate::watch::WatchSession::watch(folder, move |event| {
            library.handle_watch_event(event);
        })
        .map_err(|error| LeylineError::Watch(error.to_string()))?;
        *slot = Some(session);
        drop(slot);
        self.emit(Event::WatchStarted {
            folder: folder.to_owned(),
        });
        Ok(())
    }

    /// Ends the running watched-folder session, if any (`docs/adr/0039`) —
    /// a no-op when none is active. Blocks briefly (at most one tick
    /// interval) for the background thread to actually stop; by the time
    /// this returns, `Event::WatchStopped { reason: None }` has already
    /// fired.
    pub fn watch_stop(&self) {
        // See the comment on `tether_disconnect`: the `take()` must be its
        // own statement so the mutex guard drops before `stop()` blocks
        // joining the background thread, which needs this same lock to
        // report `WatchSessionEvent::Stopped`.
        let session = lock(&self.inner.watch).take();
        if let Some(session) = session {
            session.stop();
        }
    }

    /// Turns one `crate::watch::WatchSessionEvent` into catalog state and
    /// an engine event (`docs/adr/0039`). Runs on the watch session's own
    /// background thread.
    pub(super) fn handle_watch_event(&self, event: crate::watch::WatchSessionEvent) {
        match event {
            crate::watch::WatchSessionEvent::Ready(file) => {
                let options = ImportOptions {
                    copy_files: true,
                    recursive: false,
                    pair_companions: true,
                    thumbnails: true,
                };
                // A failed import of one settled file (e.g. an undecodable
                // one) is dropped rather than surfaced as a session error —
                // same best-effort stance `handle_tether_event` takes.
                if let Ok(report) = self.import(&file.path, &options, |_, _| {}) {
                    let asset_ids: Vec<AssetId> = report
                        .imported
                        .iter()
                        .map(|imported| imported.registered.asset)
                        .collect();
                    if !asset_ids.is_empty() {
                        self.emit(Event::AssetsAdded { asset_ids });
                    }
                }
                // Unlike the tether staging directory, the watched folder
                // is a location the caller chose on purpose (their own
                // "drop zone" or a memory card mount) — the original file
                // is left in place, never deleted.
            }
            crate::watch::WatchSessionEvent::Stopped(reason) => {
                lock(&self.inner.watch).take();
                self.emit(Event::WatchStopped { reason });
            }
        }
    }
}
