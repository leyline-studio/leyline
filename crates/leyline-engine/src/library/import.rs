//! Import and scan, and the thumbnails an import warms.

use super::*;

impl Library {
    /// Imports files (`docs/engine-api.md` §6). `progress` receives
    /// `(done, total)` per candidate file.
    ///
    /// This is the synchronous core; it holds the catalog for the whole
    /// batch. Prefer [`Library::import_async`] from interactive clients.
    ///
    /// Every asset that lands in the catalog also gets its thumbnail
    /// rendered before this returns (§11), through the same `preview` core
    /// `preview_async` uses — the client never has to schedule that step
    /// itself. A thumbnail failure never turns a successful import into a
    /// skip: it just leaves that one asset without a cached thumbnail, and
    /// the existing lazy path (`cached_preview` + `preview_async`) picks it
    /// up the first time it needs to be displayed, exactly as before this
    /// step existed.
    pub fn import<F: Into<Flow>>(
        &self,
        source: &Path,
        options: &ImportOptions,
        progress: impl FnMut(u64, u64) -> F,
    ) -> Result<ImportReport> {
        let report = {
            let mut catalog = lock(&self.inner.catalog);
            crate::import::import(&mut catalog, &self.inner.root, source, options, progress)
        }?;
        if options.thumbnails {
            self.spawn_import_thumbnails(&report.imported);
        }
        Ok(report)
    }

    /// Imports exactly the files given, which must live under `source`
    /// (ADR 0065 §4) — the counterpart of [`Library::import`] for a client
    /// that has let the user choose from a [`Library::scan_import`].
    ///
    /// Same pipeline, same report, same thumbnail pass; a file outside
    /// `source` is skipped, never filed at random.
    pub fn import_files<F: Into<Flow>>(
        &self,
        source: &Path,
        files: &[PathBuf],
        options: &ImportOptions,
        progress: impl FnMut(u64, u64) -> F,
    ) -> Result<ImportReport> {
        let report = {
            let mut catalog = lock(&self.inner.catalog);
            crate::import::import_files(
                &mut catalog,
                &self.inner.root,
                source,
                files,
                options,
                progress,
            )
        }?;
        if options.thumbnails {
            self.spawn_import_thumbnails(&report.imported);
        }
        Ok(report)
    }

    /// Lists what an import of `source` would take, **writing nothing**
    /// (ADR 0065 §1): names, capture facts, likely duplicates, and — when
    /// asked — the preview each file carries inside itself.
    ///
    /// Reads headers only. Prefer [`Library::scan_import_async`] from an
    /// interactive client: a full card is hundreds of files.
    pub fn scan_import<F: Into<Flow>>(
        &self,
        source: &Path,
        options: &ScanOptions,
        progress: impl FnMut(u64, u64) -> F,
    ) -> Result<ScanReport> {
        let catalog = lock(&self.inner.catalog);
        crate::scan::scan(&catalog, source, options, progress)
    }

    /// Scans as a job (§3.1): returns immediately, progresses as
    /// `JobProgress` per file, then `JobFinished` with `JobResult::Scan`.
    ///
    /// Nothing is written, so nothing is announced beyond the job itself —
    /// no `AssetsAdded`, no `AssetsChanged`.
    pub fn scan_import_async(&self, source: &Path, options: &ScanOptions) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        let source = source.to_owned();
        let options = options.clone();
        self.spawn_job(move || {
            let scanned = library.scan_import(&source, &options, library.job_progress(job));
            let result = match scanned {
                Ok(report) => JobResult::Scan(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.finish_job(job, result);
        });
        job
    }

    /// Best-effort thumbnail pass for freshly imported assets (§6, §11),
    /// started as a background job so the import can return (ADR 0082 §4).
    ///
    /// Filling the catalog and filling the cache are two jobs, and only the
    /// first is the import: this one emits its `PreviewReady` events like any
    /// other render, and a client that never waits for it still gets a usable
    /// grid — ADR 0082 §1 is what guarantees that, not this pass.
    ///
    /// **Ordered as the grid will show them**, because the first second must
    /// go to the photos seen first, and the order of an import has no reason
    /// to be that one — a card of old photos sorts to the bottom of a grid
    /// ordered by capture date. The same query drops companions, which no
    /// grid draws (ADR 0082 §5).
    ///
    /// **Parallel.** This used to be impossible for an exact reason:
    /// `preview()` serialised on the decode cache and the stage cache, two
    /// mutexes held across a whole render (the catalog lock is already
    /// released between, ADR 0023). A thumbnail taken from the file's own
    /// picture touches neither — no sensor decode, no stage — so the
    /// objection went with its cause. The photos that do fall back to a
    /// render still serialise there, correctly.
    pub(super) fn spawn_import_thumbnails(&self, imported: &[ImportedFile]) {
        let assets: Vec<AssetId> = imported.iter().map(|f| f.registered.asset).collect();
        let library = self.clone();
        self.spawn_job(move || library.warm_thumbnails(&assets));
    }

    /// Fills the thumbnail cache for `assets` and returns when it is done.
    ///
    /// The body of the pass above, exposed because scheduling it and running
    /// it are two different questions. A client with a window wants it in the
    /// background; the command-line tool has no event loop and a process that
    /// exits — a job spawned there would be killed before it drew anything,
    /// so it calls this instead and waits.
    ///
    /// Best-effort throughout: a thumbnail that cannot be produced is skipped,
    /// never raised. Ordering, companion-skipping and parallelism are
    /// described on [`Library::spawn_import_thumbnails`].
    pub fn warm_thumbnails(&self, assets: &[AssetId]) {
        let Ok(ordered) = lock(&self.inner.catalog).grid_order(assets) else {
            return;
        };
        use rayon::prelude::*;
        ordered.par_iter().for_each(|&asset| {
            // One `PreviewReady` per thumbnail, like any other render (§3.2):
            // without them a client would sit on a grid of empty cells until
            // something else happened to make it reload.
            if self.preview(asset, PreviewKind::Thumbnail).is_ok() {
                self.emit(Event::PreviewReady {
                    asset_id: asset,
                    kind: PreviewKind::Thumbnail,
                });
            }
        });
    }

    /// Imports files as a job (§3.1): returns immediately, progresses as
    /// `JobProgress` per candidate file, then `AssetsAdded` and
    /// `JobFinished` with the report.
    pub fn import_async(&self, source: &Path, options: &ImportOptions) -> JobId {
        self.spawn_import(source, None, options)
    }

    /// Imports a chosen list as a job — [`Library::import_files`] with the
    /// event contract of [`Library::import_async`] (ADR 0065 §5).
    pub fn import_files_async(
        &self,
        source: &Path,
        files: &[PathBuf],
        options: &ImportOptions,
    ) -> JobId {
        self.spawn_import(source, Some(files.to_vec()), options)
    }

    /// The body both import jobs share: whole folder when `files` is `None`,
    /// exactly that list otherwise. One place emits the events, so the two
    /// cannot drift on what an import announces.
    pub(super) fn spawn_import(
        &self,
        source: &Path,
        files: Option<Vec<PathBuf>>,
        options: &ImportOptions,
    ) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        let source = source.to_owned();
        let options = *options;
        self.spawn_job(move || {
            let progress = library.job_progress(job);
            let imported = match &files {
                None => library.import(&source, &options, progress),
                Some(files) => library.import_files(&source, files, &options, progress),
            };
            let result = match imported {
                Ok(report) => {
                    let asset_ids: Vec<AssetId> = report
                        .imported
                        .iter()
                        .map(|file| file.registered.asset)
                        .collect();
                    if !asset_ids.is_empty() {
                        library.emit(Event::AssetsAdded { asset_ids });
                    }
                    JobResult::Import(report)
                }
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.finish_job(job, result);
        });
        job
    }
}
