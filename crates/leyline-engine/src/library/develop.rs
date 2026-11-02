//! Developing: edit sessions, develop presets, reprocessing, XMP sidecars,
//! camera profiles and LUTs.

use super::*;

impl Library {
    /// Splits a batch's targets into the ones it may write and the ones a
    /// session is holding (ADR 0120 §2).
    ///
    /// Before the claim existed, these batches simply *blocked* behind an
    /// open session, because everything shared one lock. Now they would write
    /// behind its back — and a session's whole promise is that its in-memory
    /// state is the authoritative one — so a claimed version is left alone
    /// and reported, the way any other per-version failure is.
    pub(super) fn split_claimed(&self, versions: &[VersionId]) -> (Vec<VersionId>, Vec<VersionId>) {
        let claims = lock(&self.inner.claims);
        versions
            .iter()
            .partition(|version| !claims.contains(version))
    }

    /// Opens an edit session on a version (§10.1).
    ///
    /// The session claims **that version** and nothing else (ADR 0120): every
    /// other call on this library — a grid query, a preview, an export job —
    /// goes on working while it is open, and a second session on the same
    /// version is refused by name with [`LeylineError::VersionBusy`] rather
    /// than left to block. Every history write of the session emits
    /// `VersionChanged` (§3.2).
    ///
    /// The claim is released when the session is dropped, after `Drop` has
    /// committed whatever was pending — so nothing is lost and nothing stays
    /// claimed.
    pub fn edit(&self, version: VersionId) -> Result<EditSession<VersionClaim>> {
        let claim = VersionClaim::take(self, version)?;
        let library = self.clone();
        Ok(EditSession::open(claim, version)?
            .with_notifier(move |version_id| library.emit(Event::VersionChanged { version_id })))
    }

    /// Captures the fields of `groups` from `from`'s head into a named,
    /// stored preset (`docs/engine-api.md` §10.3).
    pub fn create_preset(
        &self,
        name: &str,
        from: VersionId,
        groups: &[SettingsGroup],
    ) -> Result<PresetId> {
        let captured = crate::presets::capture(&self.catalog(), from, groups)?;
        self.catalog_mut().create_preset(name, &captured.to_json())
    }

    /// Captures the fields of `groups` from `from`'s head, without
    /// persisting anything — the read half of [`Library::create_preset`],
    /// used for a one-shot "copy settings" (Studio's copy/paste, as opposed
    /// to a named, stored preset).
    pub fn capture_settings(
        &self,
        from: VersionId,
        groups: &[SettingsGroup],
    ) -> Result<PresetSettings> {
        crate::presets::capture(&self.catalog(), from, groups)
    }

    /// Applies an ad-hoc, unsaved [`PresetSettings`] to several versions —
    /// the "paste settings" half of copy/paste, sharing
    /// [`Library::apply_preset`]'s mechanics without requiring the fields be
    /// stored as a named preset first.
    pub fn apply_settings<F: Into<Flow>>(
        &self,
        settings: &PresetSettings,
        versions: &[VersionId],
        mut progress: impl FnMut(u64, u64) -> F,
    ) -> Result<PresetApplyReport> {
        let (free, busy) = self.split_claimed(versions);
        let mut catalog = lock(&self.inner.catalog);
        let mut report = crate::presets::apply_batch(&mut catalog, settings, &free, &mut progress);
        drop(catalog);
        report.failed.extend(busy.into_iter().map(busy_apply));
        for &version in &report.applied {
            self.emit(Event::VersionChanged {
                version_id: version,
            });
        }
        Ok(report)
    }

    /// Lists every stored develop preset, ordered by name (§10.3).
    /// Reads one stored develop preset, its shelf and its version included.
    pub fn preset(&self, preset: PresetId) -> Result<Preset> {
        self.catalog().preset(preset)
    }

    /// Every stored develop preset, favourites first then by name (§10.3).
    pub fn presets(&self) -> Result<Vec<Preset>> {
        self.catalog().presets()
    }

    /// Renames a stored develop preset (§10.3).
    pub fn rename_preset(&self, id: PresetId, name: &str) -> Result<()> {
        self.catalog_mut().rename_preset(id, name)
    }

    /// Deletes a stored develop preset; revisions it already produced are
    /// untouched (§10.3, `docs/presets.md` §4).
    pub fn delete_preset(&self, id: PresetId) -> Result<()> {
        self.catalog_mut().delete_preset(id)
    }

    /// Applies a stored preset to several versions (§10.3): the synchronous
    /// core. One fresh `EditSession` per version, so one `VersionChanged`
    /// per success — prefer [`Library::apply_preset_async`] from
    /// interactive clients.
    pub fn apply_preset<F: Into<Flow>>(
        &self,
        preset: PresetId,
        versions: &[VersionId],
        mut progress: impl FnMut(u64, u64) -> F,
    ) -> Result<PresetApplyReport> {
        let stored = self.catalog().preset(preset)?;
        let fields = PresetSettings::parse(&stored.preset_json)?;
        let (free, busy) = self.split_claimed(versions);
        let mut catalog = lock(&self.inner.catalog);
        // Each revision records the preset *and the version of it* that was
        // applied (ADR 0058 §5), which is what makes "developed with an older
        // version of this preset" answerable later.
        let mut report = crate::presets::apply_batch_from(
            &mut catalog,
            &fields,
            Some((preset, stored.revision)),
            &free,
            &mut progress,
        );
        drop(catalog);
        report.failed.extend(busy.into_iter().map(busy_apply));
        for &version in &report.applied {
            self.emit(Event::VersionChanged {
                version_id: version,
            });
        }
        Ok(report)
    }

    /// Replaces a preset's settings with those of `version`, bumping its
    /// version counter (ADR 0058 §6).
    ///
    /// Revisions already produced by it are untouched: this decides what the
    /// *next* application writes (`docs/presets.md` §2).
    pub fn update_preset(
        &self,
        preset: PresetId,
        version: VersionId,
        groups: &[SettingsGroup],
    ) -> Result<u32> {
        let captured = self.capture_settings(version, groups)?;
        self.catalog_mut()
            .update_preset(preset, &captured.to_json())
    }

    /// Files a preset in a folder, or at the root with `None` (ADR 0058 §2).
    pub fn file_preset(&self, preset: PresetId, folder: Option<PresetFolderId>) -> Result<()> {
        self.catalog_mut().file_preset(preset, folder)
    }

    /// Marks a preset as a favourite, or stops.
    pub fn favourite_preset(&self, preset: PresetId, favourite: bool) -> Result<()> {
        self.catalog_mut().favourite_preset(preset, favourite)
    }

    /// The preset folders, by name (ADR 0058 §2).
    pub fn preset_folders(&self) -> Result<Vec<PresetFolder>> {
        self.catalog().preset_folders()
    }

    /// Creates a preset folder.
    pub fn create_preset_folder(&self, name: &str) -> Result<PresetFolderId> {
        self.catalog_mut().create_preset_folder(name)
    }

    /// Renames a preset folder.
    pub fn rename_preset_folder(&self, folder: PresetFolderId, name: &str) -> Result<()> {
        self.catalog_mut().rename_preset_folder(folder, name)
    }

    /// Deletes a preset folder; its presets return to the root.
    pub fn delete_preset_folder(&self, folder: PresetFolderId) -> Result<()> {
        self.catalog_mut().delete_preset_folder(folder)
    }

    /// The versions whose current revision came from `preset`, each with the
    /// version of the preset it was made with (ADR 0058 §6).
    ///
    /// The pair `(version, n)` where `n` is lower than the preset's own
    /// revision is exactly "this photo carries an older version of this
    /// preset" — what re-applying is offered on.
    pub fn versions_from_preset(&self, preset: PresetId) -> Result<Vec<(VersionId, u32)>> {
        self.catalog().versions_from_preset(preset)
    }

    /// Applies a stored preset to several versions as a job (§3.1, §10.3):
    /// returns immediately, progresses as `JobProgress` per version, then
    /// `JobFinished` with the report (per-version failures inside it, batch
    /// failures — e.g. an unknown preset — as `Failed`).
    pub fn apply_preset_async(&self, preset: PresetId, versions: Vec<VersionId>) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let applied = library.apply_preset(preset, &versions, library.job_progress(job));
            let result = match applied {
                Ok(report) => JobResult::Preset(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.finish_job(job, result);
        });
        job
    }

    /// Migrates several versions to the engine's current process version
    /// (`docs/engine-api.md` §10.4, `docs/pipeline.md` §4.5): the
    /// synchronous core. One fresh `EditSession` per version, so one
    /// `VersionChanged` per migration — prefer [`Library::reprocess_async`]
    /// from interactive clients.
    pub fn reprocess<F: Into<Flow>>(
        &self,
        versions: &[VersionId],
        mut progress: impl FnMut(u64, u64) -> F,
    ) -> Result<ReprocessReport> {
        let (free, busy) = self.split_claimed(versions);
        let mut catalog = lock(&self.inner.catalog);
        let mut report = crate::reprocess::reprocess_batch(&mut catalog, &free, &mut progress);
        drop(catalog);
        report.failed.extend(
            busy.into_iter()
                .map(|version| crate::reprocess::FailedReprocess {
                    version,
                    reason: BUSY_REASON.to_owned(),
                }),
        );
        for &version in &report.reprocessed {
            self.emit(Event::VersionChanged {
                version_id: version,
            });
        }
        Ok(report)
    }

    /// Migrates several versions to the engine's current process version as
    /// a job (§3.1, §10.4): returns immediately, progresses as
    /// `JobProgress` per version, then `JobFinished` with the report
    /// (per-version failures inside it, already-current versions counted
    /// separately, never as failures).
    pub fn reprocess_async(&self, versions: Vec<VersionId>) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let migrated = library.reprocess(&versions, library.job_progress(job));
            let result = match migrated {
                Ok(report) => JobResult::Reprocess(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.finish_job(job, result);
        });
        job
    }

    /// Writes the XMP sidecar of an asset — the On Demand synchronization
    /// of `docs/catalog.md` §29 — and returns its path.
    pub fn write_xmp(&self, asset: AssetId) -> Result<PathBuf> {
        crate::xmp::write_xmp_sidecar(&self.catalog(), &self.inner.root, asset)
    }

    /// Reads the XMP sidecar sitting next to an asset's file and seeds the
    /// catalog with it (ADR 0047), returning whether anything was written.
    ///
    /// Not the mirror image of [`Library::write_xmp`], deliberately: this
    /// **fills** what the catalog leaves empty and unions keywords, so it
    /// can never replace or remove what is already recorded (ADR 0047 §3).
    /// `Ok(false)` covers both "no usable sidecar" and "nothing left to
    /// fill" — neither is an error.
    ///
    /// The counterpart for a whole folder is an ordinary import, which reads
    /// sidecars on its own.
    pub fn read_xmp(&self, asset: AssetId) -> Result<bool> {
        let mut catalog = lock(&self.inner.catalog);
        let relative = catalog.asset_details(asset)?.relative_path;
        let file = self
            .inner
            .root
            .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        let Some(sidecar) = crate::xmp::read_xmp_sidecar(&file) else {
            return Ok(false);
        };
        crate::xmp::apply_xmp_sidecar(&mut catalog, asset, &sidecar)
    }

    /// Imports `source` as a camera profile (ADR 0035): copies it, under
    /// its own filename, to `Profiles/Camera/`, and returns the
    /// library-relative path and BLAKE3 checksum to store in a revision's
    /// `camera_profile` (`leyline_core::CameraProfile`). Unlike the single
    /// active map pack, several camera profiles coexist — one per camera —
    /// so an existing file at the destination is refused, never
    /// overwritten (the same never-overwrite posture `render_export` takes
    /// for its output files): silently replacing it would change what
    /// every revision that already references it renders to.
    /// Refused on a read-only handle, same as every catalog write.
    pub fn import_camera_profile(&self, source: &Path) -> Result<ImportedCameraProfile> {
        if self.catalog().is_read_only() {
            return Err(LeylineError::Db(
                "library opened read-only; writes are refused".to_owned(),
            ));
        }
        let file_name = source.file_name().ok_or_else(|| {
            LeylineError::InvalidSettings("camera profile source has no file name".to_owned())
        })?;
        // The relative path ends up in `settings_json`, which is UTF-8 by
        // construction. A lossily-converted name would be recorded with
        // replacement characters and never resolve back to the file that
        // was actually copied, so refuse the import instead of creating a
        // revision that cannot be rendered.
        let file_name = file_name.to_str().ok_or_else(|| {
            LeylineError::InvalidSettings(format!(
                "camera profile file name is not valid UTF-8: {}",
                source.display()
            ))
        })?;
        let dir = self.inner.root.join("Profiles").join("Camera");
        std::fs::create_dir_all(&dir)?;
        let destination = dir.join(file_name);
        if destination.exists() {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "{} already exists; camera profiles are never overwritten",
                    destination.display()
                ),
            )));
        }
        std::fs::copy(source, &destination)?;
        let bytes = std::fs::read(&destination)?;
        let checksum = format!("blake3:{}", blake3::hash(&bytes).to_hex());
        let relative_path = format!("Profiles/Camera/{file_name}");
        Ok(ImportedCameraProfile {
            relative_path,
            checksum,
        })
    }

    /// Lists the camera profiles already imported under `Profiles/Camera/`,
    /// each with the current BLAKE3 checksum of its bytes on disk, sorted by
    /// path. This is how a client turns "the user picked this profile" into
    /// the `leyline_core::CameraProfile` a revision stores, without ever
    /// hashing files itself; an empty list when nothing has been imported.
    /// Non-`.dcp` files in the directory are ignored.
    pub fn camera_profiles(&self) -> Result<Vec<ImportedCameraProfile>> {
        let dir = self.inner.root.join("Profiles").join("Camera");
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut profiles = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let is_dcp = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dcp"));
            if !path.is_file() || !is_dcp {
                continue;
            }
            let Some(file_name) = path.file_name() else {
                continue;
            };
            let bytes = std::fs::read(&path)?;
            profiles.push(ImportedCameraProfile {
                relative_path: format!("Profiles/Camera/{}", file_name.to_string_lossy()),
                checksum: format!("blake3:{}", blake3::hash(&bytes).to_hex()),
            });
        }
        profiles.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        Ok(profiles)
    }

    /// Imports a `.cube` LUT into `Profiles/LUT/` and hands back the
    /// reference a revision stores (ADR 0053 §1).
    ///
    /// The same contract as [`Library::import_camera_profile`], deliberately:
    /// the file is copied into the library so it stays portable, the returned
    /// checksum is of the bytes that were copied, and an existing name is
    /// refused rather than overwritten. The table is *not* parsed here — a
    /// malformed LUT surfaces when it is rendered through, named by
    /// [`LeylineError::LutFailed`], which is also what happens to a file that
    /// changes later.
    pub fn import_lut(&self, source: &Path) -> Result<ImportedLut> {
        if self.catalog().is_read_only() {
            return Err(LeylineError::Db(
                "library opened read-only; writes are refused".to_owned(),
            ));
        }
        let file_name = source.file_name().ok_or_else(|| {
            LeylineError::InvalidSettings("LUT source has no file name".to_owned())
        })?;
        // The relative path ends up in `settings_json`, which is UTF-8 by
        // construction (same reasoning as `import_camera_profile`).
        let file_name = file_name.to_str().ok_or_else(|| {
            LeylineError::InvalidSettings(format!(
                "LUT file name is not valid UTF-8: {}",
                source.display()
            ))
        })?;
        let dir = self.inner.root.join("Profiles").join("LUT");
        std::fs::create_dir_all(&dir)?;
        let destination = dir.join(file_name);
        if destination.exists() {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "{} already exists; LUTs are never overwritten",
                    destination.display()
                ),
            )));
        }
        std::fs::copy(source, &destination)?;
        let bytes = std::fs::read(&destination)?;
        Ok(ImportedLut {
            relative_path: format!("Profiles/LUT/{file_name}"),
            checksum: format!("blake3:{}", blake3::hash(&bytes).to_hex()),
        })
    }

    /// Lists the LUTs already imported under `Profiles/LUT/`, each with the
    /// current checksum of its bytes on disk, sorted by path — the counterpart
    /// of [`Library::camera_profiles`], and how a client turns "the user picked
    /// this look" into a stored reference without hashing anything itself.
    /// Files that are not `.cube` are ignored.
    pub fn luts(&self) -> Result<Vec<ImportedLut>> {
        let dir = self.inner.root.join("Profiles").join("LUT");
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut luts = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let is_cube = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("cube"));
            if !path.is_file() || !is_cube {
                continue;
            }
            let Some(file_name) = path.file_name() else {
                continue;
            };
            let bytes = std::fs::read(&path)?;
            luts.push(ImportedLut {
                relative_path: format!("Profiles/LUT/{}", file_name.to_string_lossy()),
                checksum: format!("blake3:{}", blake3::hash(&bytes).to_hex()),
            });
        }
        luts.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        Ok(luts)
    }

    /// The world basemap compiled into the binary (ADR 0059), when this
    /// build carries one. Natural Earth I, zoom 0–5, JPEG tiles: enough to
    /// place GPS pins on continents and coastlines from the first launch,
    /// never a substitute for a pack the user imports.
    #[cfg(feature = "bundled-basemap")]
    const BUNDLED_BASEMAP: &'static [u8] =
        include_bytes!("../../../assets/basemap/world-z0-5.mbtiles");
}
