//! What leaves the library: export (with `verify` and `reproduce`), print and
//! contact sheets, with their presets.

use super::*;

impl Library {
    /// Exports every version of `request` through its recipe (§12).
    /// `progress` receives `(done, total)` per version; one failure does
    /// not stop the rest — it is reported in [`ExportReport::failed`].
    ///
    /// Like [`Library::preview`], this deliberately does *not* hold the
    /// catalog lock across a render: decode + `process1`–`5` + encode can
    /// take seconds, and this runs once per version, so holding the lock
    /// for the whole request would freeze all of Studio's navigation,
    /// search and metadata edits for its full duration instead of just the
    /// version at hand (ADR 0024). Unlike a preview, an exported file is a
    /// one-shot artifact the caller asked for — not a revision-keyed cache
    /// another call might later serve as "current" — so no amendment guard
    /// is needed: each file is simply journaled once rendered. A
    /// [`ExportRecipe::Preset`] is resolved once, up front, under its own
    /// short lock — a preset edited mid-request does not retroactively
    /// change versions already exported.
    pub fn export<F: Into<Flow>>(
        &self,
        request: &ExportRequest,
        progress: impl FnMut(u64, u64) -> F,
    ) -> Result<ExportReport> {
        let (settings, preset) = self.resolve_export_recipe(&request.recipe)?;
        settings.validate().map_err(crate::export::export_err)?;
        let planned = self.plan_batch(&request.versions, &settings, &request.destination_dir)?;
        let in_flight = request
            .concurrency
            .unwrap_or_else(default_export_concurrency)
            .clamp(1, request.versions.len().max(1));
        self.run_export_batch(planned, &settings, preset, in_flight, progress)
    }

    /// Plans every version of a batch in one catalog lock, in request order,
    /// and gives each its output path (ADR 0068 §2).
    ///
    /// Reserving the names here — rather than letting each render check the
    /// filesystem when it gets there — is what keeps two versions of one
    /// asset colliding *deterministically*: the later one in request order
    /// fails, as it always has, instead of the two racing for the same path
    /// and one silently overwriting the other.
    #[allow(clippy::type_complexity)]
    pub(super) fn plan_batch(
        &self,
        versions: &[VersionId],
        settings: &ExportSettings,
        destination_dir: &Path,
    ) -> Result<Vec<(VersionId, PlannedExport)>> {
        let catalog = lock(&self.inner.catalog);
        let mut claimed: std::collections::HashSet<String> = std::collections::HashSet::new();
        Ok(versions
            .iter()
            .map(|&version| {
                let planned = crate::export::plan_export(&catalog, &self.inner.root, version)
                    .and_then(|plan| {
                        let name = crate::export::output_name(&plan, settings);
                        let destination = destination_dir.join(&name);
                        if !claimed.insert(name) {
                            // An earlier version of this batch already owns
                            // the name. Same outcome as meeting the file on
                            // disk, decided before anything is decoded.
                            crate::export::refuse_existing(&destination)?;
                            return Err(LeylineError::Io(std::io::Error::new(
                                std::io::ErrorKind::AlreadyExists,
                                format!(
                                    "{} is already written by an earlier version of this \
                                     batch; exports never overwrite",
                                    destination.display()
                                ),
                            )));
                        }
                        crate::export::refuse_existing(&destination)?;
                        Ok((plan, destination))
                    });
                (version, planned.map_err(|error| error.to_string()))
            })
            .collect())
    }

    /// Renders, encodes and journals a planned batch, `in_flight` photos at
    /// a time (ADR 0068).
    ///
    /// The workers are plain threads, never rayon tasks: rayon's work
    /// stealing may run *another* task on a thread that is already inside
    /// one, and the catalog mutex is not reentrant (`docs/engine-api.md`
    /// §3.1). Pixel-level `par_iter` inside each render keeps using rayon's
    /// global pool — that nesting is precisely why four photos are enough to
    /// fill sixteen cores.
    pub(super) fn run_export_batch<F: Into<Flow>>(
        &self,
        planned: Vec<(VersionId, PlannedExport)>,
        settings: &ExportSettings,
        preset: Option<ExportPresetId>,
        in_flight: usize,
        mut progress: impl FnMut(u64, u64) -> F,
    ) -> Result<ExportReport> {
        let total = planned.len() as u64;
        // One slot per version, filled in place, so the report keeps request
        // order however the renders finish.
        let outcomes: Vec<std::sync::Mutex<Option<std::result::Result<PathBuf, String>>>> = (0
            ..planned.len())
            .map(|_| std::sync::Mutex::new(None))
            .collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        // Set by the progress loop below when the caller asks to stop, read
        // by every worker before it picks up its next photograph
        // (ADR 0139 §2): a batch of four in flight therefore ends after at
        // most four more files, each of them whole.
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();

        std::thread::scope(|scope| {
            for _ in 0..in_flight {
                let (next, outcomes, planned, cancelled) = (&next, &outcomes, &planned, &cancelled);
                let done_tx = done_tx.clone();
                scope.spawn(move || {
                    loop {
                        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                            return;
                        }
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((_, planned)) = planned.get(index) else {
                            return;
                        };
                        let outcome = match planned {
                            // A panic inside one render is that version's
                            // failure, not the batch's (ADR 0152 §5).
                            Ok((plan, destination)) => crate::panic_net::isolate(|| {
                                crate::export::render_export_to(plan, settings, destination)
                                    .and_then(|()| {
                                        // Hashed before the lock is taken
                                        // (ADR 0024, ADR 0153 §2).
                                        let output_checksum =
                                            crate::provenance::file_checksum(destination)?;
                                        let mut catalog = lock(&self.inner.catalog);
                                        crate::export::journal_export(
                                            &mut catalog,
                                            plan,
                                            preset,
                                            settings,
                                            destination,
                                            output_checksum,
                                        )?;
                                        Ok(destination.clone())
                                    })
                                    .map_err(|error| error.to_string())
                            })
                            .and_then(|outcome| outcome),
                            Err(reason) => Err(reason.clone()),
                        };
                        *lock(&outcomes[index]) = Some(outcome);
                        // A closed receiver only means the batch is being
                        // torn down; the work itself is already done.
                        let _ = done_tx.send(());
                    }
                });
            }
            drop(done_tx);
            let mut done = 0;
            while done_rx.recv().is_ok() {
                done += 1;
                if progress(done, total).into().stops() {
                    cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        });

        let mut report = ExportReport {
            cancelled: cancelled.load(std::sync::atomic::Ordering::Relaxed),
            ..ExportReport::default()
        };
        for ((version, _), outcome) in planned.iter().zip(outcomes) {
            match outcome.into_inner().unwrap_or_else(|e| e.into_inner()) {
                Some(Ok(path)) => report.exported.push(crate::export::ExportedVersion {
                    version: *version,
                    path,
                }),
                Some(Err(reason)) => report.failed.push(crate::export::FailedExport {
                    version: *version,
                    reason,
                }),
                // Empty only once the batch has been cancelled: a worker
                // that stops leaves every photograph after it untouched,
                // and an untouched photograph is neither an export nor a
                // failure — it is simply not in the report.
                None => debug_assert!(report.cancelled, "an unfilled slot outside a cancellation"),
            }
        }
        Ok(report)
    }

    /// Exports `request` as a job (§3.1, §12): returns immediately,
    /// progresses as `JobProgress` per version, then `JobFinished` with the
    /// report (per-version failures inside it, request-level failures as
    /// `Failed`).
    pub fn export_async(&self, request: ExportRequest) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let exported = library.export(&request, library.job_progress(job));
            let result = match exported {
                Ok(report) => JobResult::Export(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.finish_job(job, result);
        });
        job
    }

    /// Resolves a recipe to the settings that drive the render, and the
    /// preset id (if any) each export journals against.
    pub(super) fn resolve_export_recipe(
        &self,
        recipe: &crate::export::ExportRecipe,
    ) -> Result<(ExportSettings, Option<ExportPresetId>)> {
        match recipe {
            crate::export::ExportRecipe::Adhoc(settings) => Ok((settings.clone(), None)),
            crate::export::ExportRecipe::Preset(preset) => {
                let catalog = lock(&self.inner.catalog);
                let stored = catalog.export_preset(*preset)?;
                let settings = ExportSettings::parse(&stored.settings_json)
                    .map_err(crate::export::export_err)?;
                Ok((settings, Some(*preset)))
            }
        }
    }

    /// Stores a named export preset (§12), validating the recipe first.
    pub fn create_export_preset(
        &self,
        name: &str,
        settings: &ExportSettings,
    ) -> Result<ExportPresetId> {
        settings.validate().map_err(crate::export::export_err)?;
        self.catalog_mut()
            .create_export_preset(name, &settings.to_json())
    }

    /// Lists every stored export preset, ordered by name (§12).
    pub fn export_presets(&self) -> Result<Vec<ExportPreset>> {
        self.catalog().export_presets()
    }

    /// Says whether the file at `path` is one this library exported
    /// (ADR 0153 §4): *intact*, *modified* since, or *unknown* here.
    ///
    /// The file is read and hashed before the catalog lock is taken
    /// (ADR 0024); only the two journal lookups hold it.
    pub fn verify_export(&self, path: &Path) -> Result<crate::ExportVerdict> {
        let file = std::fs::read(path)?;
        let checksum = *blake3::hash(&file).as_bytes();
        crate::verify::verdict(&self.catalog(), &file, &checksum)
    }

    /// Renders again the export the file at `path` came from, into
    /// `destination_dir`, and compares (ADR 0153 §5): *bit for bit*, *same
    /// pixels* (the metadata moved since), or *different pixels* with the
    /// condition that can be named.
    ///
    /// The journal row is found as [`Library::verify_export`] finds it; a
    /// file unknown to this library is [`LeylineError::NotReproducible`].
    /// The render holds no catalog lock (ADR 0024) and journals nothing: a
    /// reproduction checks an export, it is not one. Like every export, it
    /// refuses to overwrite a file already at its destination.
    pub fn reproduce_export(
        &self,
        path: &Path,
        destination_dir: &Path,
    ) -> Result<crate::ReproduceVerdict> {
        let file = std::fs::read(path)?;
        let checksum = *blake3::hash(&file).as_bytes();
        let plan = {
            let catalog = self.catalog();
            let verdict = crate::verify::verdict(&catalog, &file, &checksum)?;
            crate::reproduce::plan(&catalog, &self.inner.root, path, verdict)?
        };
        crate::reproduce::reproduce(plan, path, destination_dir)
    }

    /// Prints every version of `request` through its recipe (ADR 0036) —
    /// same shape and same catalog-lock discipline as [`Library::export`]:
    /// decode+render+scale+encode never holds the catalog lock, one failing
    /// version doesn't stop the batch, and a [`PrintRecipe::Preset`] is
    /// resolved once up front. Unlike export there is no journal step: a
    /// print has no history table (ADR 0036).
    pub fn print<F: Into<Flow>>(
        &self,
        request: &PrintRequest,
        mut progress: impl FnMut(u64, u64) -> F,
    ) -> Result<PrintReport> {
        let settings = self.resolve_print_recipe(&request.recipe)?;
        settings.validate().map_err(crate::print::print_err)?;
        let total = request.versions.len() as u64;
        let mut report = PrintReport::default();
        for (done, &version) in request.versions.iter().enumerate() {
            match self.print_one(version, &settings, &request.destination_dir) {
                Ok(path) => report
                    .printed
                    .push(crate::print::PrintedVersion { version, path }),
                Err(error) => report.failed.push(crate::print::FailedPrint {
                    version,
                    reason: error.to_string(),
                }),
            }
            if progress(done as u64 + 1, total).into().stops() {
                report.cancelled = true;
                break;
            }
        }
        Ok(report)
    }

    /// Prints `request` as a job (§3.1): returns immediately, progresses as
    /// `JobProgress` per version, then `JobFinished` with the report
    /// (per-version failures inside it, request-level failures as `Failed`).
    pub fn print_async(&self, request: PrintRequest) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let printed = library.print(&request, library.job_progress(job));
            let result = match printed {
                Ok(report) => JobResult::Print(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.finish_job(job, result);
        });
        job
    }

    /// Resolves a recipe to the settings that drive the render.
    pub(super) fn resolve_print_recipe(&self, recipe: &PrintRecipe) -> Result<PrintSettings> {
        match recipe {
            PrintRecipe::Adhoc(settings) => Ok(settings.clone()),
            PrintRecipe::Preset(preset) => {
                let catalog = lock(&self.inner.catalog);
                let stored = catalog.print_preset(*preset)?;
                PrintSettings::parse(&stored.settings_json).map_err(crate::print::print_err)
            }
        }
    }

    /// Prints one version at its head revision and returns the written PDF
    /// — the per-version core [`Library::print`] loops over, with the
    /// catalog lock narrowed to the plan (ADR 0024). No journal write: a
    /// print has no history table (ADR 0036).
    pub(super) fn print_one(
        &self,
        version: VersionId,
        settings: &PrintSettings,
        destination_dir: &Path,
    ) -> Result<PathBuf> {
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::print::plan_print(&catalog, &self.inner.root, version)?
        };
        crate::print::render_print(&plan, settings, destination_dir)
    }

    /// Stores a named print preset (ADR 0036), validating the recipe first.
    pub fn create_print_preset(
        &self,
        name: &str,
        settings: &PrintSettings,
    ) -> Result<PrintPresetId> {
        settings.validate().map_err(crate::print::print_err)?;
        self.catalog_mut()
            .create_print_preset(name, &settings.to_json())
    }

    /// Lists every stored print preset, ordered by name (ADR 0036).
    pub fn print_presets(&self) -> Result<Vec<PrintPreset>> {
        self.catalog().print_presets()
    }

    /// Lays `request`'s versions out on a grid and writes **one** multi-page
    /// PDF (ADR 0110) — the difference from [`Library::print`], which writes
    /// one file per version.
    ///
    /// Pages are rendered and composed one at a time, so the memory a sheet
    /// costs is one page's worth whatever the number of photographs. A
    /// version that fails to render leaves its cell empty and lands in
    /// [`ContactSheetReport::failed`]; the sheet is still written, because
    /// grid position is how a person points at a frame (ADR 0110 §6).
    pub fn contact_sheet(
        &self,
        request: &ContactSheetRequest,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<ContactSheetReport> {
        let settings = self.resolve_contact_sheet_recipe(&request.recipe)?;
        settings.validate().map_err(crate::print::print_err)?;
        if request.versions.is_empty() {
            return Err(LeylineError::InvalidSettings(
                "a contact sheet needs at least one version".to_owned(),
            ));
        }
        // Refused before a single photograph is decoded: a sheet of forty
        // frames must not be rendered to discover the file was already there.
        if request.destination.exists() {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "{} already exists; contact sheets never overwrite",
                    request.destination.display()
                ),
            )));
        }

        let (box_w, box_h) = settings
            .image_box_pixels()
            .map_err(crate::print::print_err)?;
        let transform = crate::contact_sheet::output_transform(&settings)?;
        let total = request.versions.len() as u64;
        let mut report = ContactSheetReport {
            path: request.destination.clone(),
            ..ContactSheetReport::default()
        };
        let mut pages = Vec::new();
        let mut done = 0u64;

        for chunk in request
            .versions
            .chunks(settings.cells_per_page().max(1) as usize)
        {
            let mut cells = Vec::with_capacity(chunk.len());
            for &version in chunk {
                match self.render_sheet_cell(version, &settings, box_w, box_h) {
                    Ok(cell) => {
                        report.placed += 1;
                        cells.push(cell);
                    }
                    Err(error) => {
                        report.failed.push(crate::print::FailedPrint {
                            version,
                            reason: error.to_string(),
                        });
                        cells.push((None, String::new()));
                    }
                }
                done += 1;
                progress(done, total);
            }
            pages.push(crate::contact_sheet::compose_page(
                &settings,
                &cells,
                transform.as_ref(),
            )?);
        }

        report.pages = pages.len();
        leyline_export::encode_contact_sheet(&request.destination, &pages, &settings)
            .map_err(crate::print::print_err)?;
        Ok(report)
    }

    /// Renders `request` as a job (§3.1): returns immediately, progresses
    /// per photograph, then `JobFinished` with the report.
    pub fn contact_sheet_async(&self, request: ContactSheetRequest) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let sheet = library.contact_sheet(&request, {
                let library = library.clone();
                move |done, total| {
                    library.emit(Event::JobProgress {
                        job_id: job,
                        done,
                        total,
                    });
                }
            });
            let result = match sheet {
                Ok(report) => JobResult::ContactSheet(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.finish_job(job, result);
        });
        job
    }

    /// Resolves a sheet recipe to the settings that drive the render.
    pub(super) fn resolve_contact_sheet_recipe(
        &self,
        recipe: &ContactSheetRecipe,
    ) -> Result<ContactSheetSettings> {
        match recipe {
            ContactSheetRecipe::Adhoc(settings) => Ok(settings.clone()),
            ContactSheetRecipe::Preset(preset) => {
                let catalog = lock(&self.inner.catalog);
                let stored = catalog.contact_sheet_preset(*preset)?;
                ContactSheetSettings::parse(&stored.settings_json).map_err(crate::print::print_err)
            }
        }
    }

    /// Plans and renders one cell, with the catalog lock narrowed to the
    /// plan (ADR 0024) exactly as [`Library::print_one`] does.
    pub(super) fn render_sheet_cell(
        &self,
        version: VersionId,
        settings: &ContactSheetSettings,
        box_w: u32,
        box_h: u32,
    ) -> Result<(Option<Rgb8>, String)> {
        let cell = {
            let catalog = lock(&self.inner.catalog);
            crate::contact_sheet::plan_cell(
                &catalog,
                &self.inner.root,
                version,
                settings,
                box_w,
                box_h,
            )?
        };
        let image = crate::contact_sheet::render_cell(&cell, box_w, box_h)?;
        Ok((Some(image), cell.caption))
    }

    /// Stores a named contact-sheet preset (ADR 0110 §8), validating the
    /// recipe first.
    pub fn create_contact_sheet_preset(
        &self,
        name: &str,
        settings: &ContactSheetSettings,
    ) -> Result<ContactSheetPresetId> {
        settings.validate().map_err(crate::print::print_err)?;
        self.catalog_mut()
            .create_contact_sheet_preset(name, &settings.to_json())
    }

    /// Lists every stored contact-sheet preset, ordered by name (ADR 0110).
    pub fn contact_sheet_presets(&self) -> Result<Vec<ContactSheetPreset>> {
        self.catalog().contact_sheet_presets()
    }
}
