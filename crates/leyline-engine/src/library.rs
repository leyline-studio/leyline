//! The library facade: one handle over a library on disk
//! (`docs/engine-api.md` §5, §3, `docs/catalog.md` §3).
//!
//! A `Library` owns the physical layout — `catalog.db`, `Photos/`,
//! `Cache/`, `Exports/`, `Backups/` — and orchestrates the engine's
//! synchronous cores (import, preview, export, edit sessions) over it.
//!
//! The handle is the §3.3 execution model: `Send + Sync`, cloned at the
//! cost of an `Arc`, every clone sharing the same catalog. Queries stay
//! synchronous (SQLite answers in microseconds); works — import, preview
//! rendering, export — also exist as `*_async` jobs that return a `JobId`
//! immediately and progress through the [`Event`] stream (§3.1). Catalog
//! writes are serialized by an internal lock; clients have no ordering
//! constraint to respect.

use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use leyline_catalog::{Catalog, CollectionNode, ExportPreset, KeywordNode, Preset, SmartRules};
use leyline_core::{
    AssetId, CollectionId, ColorLabel, ExportPresetId, JobId, KeywordId, PickState, PresetId,
    PresetSettings, PreviewKind, Result, SettingsGroup, VersionId,
};
use leyline_export::ExportSettings;
use leyline_preview::PreviewCache;

use crate::decode_cache::DecodeCache;
use crate::events::{Event, JobResult};
use crate::export::ExportReport;
use crate::import::{ImportOptions, ImportReport, ImportedFile};
use crate::presets::PresetApplyReport;
use crate::preview::{Preview, PreviewFile};
use crate::reprocess::ReprocessReport;
use crate::session::EditSession;

/// Decoded images kept in memory for preview renders. Two covers the
/// develop loop (the edited asset, at worst in two size classes) while
/// bounding memory: a full-size 24 MP decode is ~72 MB.
const DECODE_CACHE_CAPACITY: usize = 2;

/// Upper bound on the job pool (§3.3) regardless of core count: past a few
/// dozen threads the catalog mutex and disk I/O dominate anyway, so a very
/// high core count desktop gains nothing from an even wider pool.
const JOB_POOL_MAX_THREADS: usize = 16;

/// Sizes the shared job pool (§3.3): one worker per core, clamped so an
/// unusual host (one logical core, or an exotic many-core workstation)
/// still gets a sane bound. This is the engine's only concurrency ceiling
/// for `*_async` jobs — a client that fires many of them in quick
/// succession queues behind it instead of spawning unbounded OS threads.
fn job_pool_size() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4)
        .clamp(1, JOB_POOL_MAX_THREADS)
}

/// One unit of work submitted to the job pool: a `*_async` job's closure.
type Job = Box<dyn FnOnce() + Send + 'static>;

/// An open Leyline library. Clones share the same underlying handle.
#[derive(Debug, Clone)]
pub struct Library {
    inner: Arc<Inner>,
}

/// State shared by every clone of the handle.
#[derive(Debug)]
struct Inner {
    root: PathBuf,
    cache: PreviewCache,
    catalog: Mutex<Catalog>,
    decodes: Mutex<DecodeCache>,
    /// One sender per subscriber; pruned when a receiver is dropped.
    subscribers: Mutex<Vec<Sender<Event>>>,
    /// Next job id, unique within this process.
    next_job: AtomicU64,
    /// Feeds the bounded job pool every `*_async` job runs on (§3.3),
    /// instead of each call spawning its own unmanaged OS thread. The
    /// pool's worker threads are plain `std::thread`s reading from this
    /// channel's receiving end, deliberately *not* rayon workers: pixel-
    /// level parallelism inside a job (`rayon::prelude` in
    /// `process1..5.rs`, `pixels.rs`) still runs on rayon's own separate
    /// global pool exactly as before this change. Sharing one rayon
    /// registry between job dispatch and nested `par_iter`/`join` work
    /// would let work-stealing recruit a job's thread to run *another*
    /// job's closure while the first is still holding the non-reentrant
    /// catalog mutex — a real deadlock hit during development of this
    /// pool, which is why the two stay on separate pools.
    jobs: Sender<Job>,
}

/// Read access to the catalog, released when dropped.
///
/// Holding it blocks writers: keep it for one query, not across calls
/// into the same [`Library`].
pub struct CatalogRead<'a>(MutexGuard<'a, Catalog>);

impl Deref for CatalogRead<'_> {
    type Target = Catalog;
    fn deref(&self) -> &Catalog {
        &self.0
    }
}

/// Write access to the catalog, released when dropped.
///
/// Same locking rule as [`CatalogRead`]: one operation, then drop.
pub struct CatalogWrite<'a>(MutexGuard<'a, Catalog>);

impl Deref for CatalogWrite<'_> {
    type Target = Catalog;
    fn deref(&self) -> &Catalog {
        &self.0
    }
}

impl DerefMut for CatalogWrite<'_> {
    fn deref_mut(&mut self) -> &mut Catalog {
        &mut self.0
    }
}

impl Library {
    /// Creates a new library: the §3 directory skeleton and its catalog.
    /// The root may exist (empty or not); the catalog must not.
    pub fn create(root: &Path, name: &str) -> Result<Library> {
        for dir in ["Photos", "Cache", "Exports", "Backups"] {
            std::fs::create_dir_all(root.join(dir))?;
        }
        let catalog = Catalog::create(&root.join("catalog.db"), name)?;
        Ok(Library::assemble(root, catalog))
    }

    /// Opens an existing library, applying pending catalog migrations.
    pub fn open(root: &Path) -> Result<Library> {
        let catalog = Catalog::open(&root.join("catalog.db"))?;
        Ok(Library::assemble(root, catalog))
    }

    /// Opens an existing library without write access — the §5 fallback for
    /// catalogs newer than this engine.
    pub fn open_read_only(root: &Path) -> Result<Library> {
        let catalog = Catalog::open_read_only(&root.join("catalog.db"))?;
        Ok(Library::assemble(root, catalog))
    }

    fn assemble(root: &Path, catalog: Catalog) -> Library {
        let (job_tx, job_rx) = channel::<Job>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        for i in 0..job_pool_size() {
            let job_rx = Arc::clone(&job_rx);
            std::thread::Builder::new()
                .name(format!("leyline-job-{i}"))
                .spawn(move || {
                    // A separate `let` before the `match`, deliberately not
                    // `while let Ok(job) = lock(&job_rx).recv() { job() }`:
                    // that form keeps the receiver's `MutexGuard` alive for
                    // the whole loop body, so one worker would hold the
                    // lock while *running* its job — every other worker
                    // then blocks trying to `recv()` its own job and never
                    // gets there. `recv()` errors once every `Sender` (i.e.
                    // every `Library` clone's `Inner`) has dropped: the
                    // pool shuts itself down at that point, no explicit
                    // signal needed.
                    loop {
                        let job = lock(&job_rx).recv();
                        match job {
                            Ok(job) => job(),
                            Err(_) => break,
                        }
                    }
                })
                .expect("spawning a job pool worker thread");
        }
        Library {
            inner: Arc::new(Inner {
                root: root.to_owned(),
                cache: PreviewCache::new(root.join("Cache")),
                catalog: Mutex::new(catalog),
                decodes: Mutex::new(DecodeCache::new(DECODE_CACHE_CAPACITY)),
                subscribers: Mutex::new(Vec::new()),
                next_job: AtomicU64::new(1),
                jobs: job_tx,
            }),
        }
    }

    /// Submits a `*_async` job's closure to the bounded job pool (§3.3).
    fn spawn_job(&self, job: impl FnOnce() + Send + 'static) {
        // The pool only shuts down once every `Sender` (every `Inner`) has
        // dropped, which can't happen while `self` is alive to call this.
        self.inner
            .jobs
            .send(Box::new(job))
            .expect("the job pool outlives every live Library handle");
    }

    /// The library root directory.
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// Subscribes to the event stream (`docs/engine-api.md` §3.2).
    ///
    /// Every clone of the handle feeds the same stream. Events are
    /// notifications, never complete data: re-query what you need.
    pub fn subscribe(&self) -> Receiver<Event> {
        let (sender, receiver) = channel();
        lock(&self.inner.subscribers).push(sender);
        receiver
    }

    /// Closes this handle (`docs/engine-api.md` §5): notifies every
    /// subscriber with [`Event::LibraryClosed`] so clients can tear down
    /// their UI before the handle disappears.
    ///
    /// `Library` clones share one `Arc`; other clones — and any job still
    /// running against them on the job pool (§3.3) — stay usable. The job
    /// sender lives on that same `Arc`, so the pool's worker threads keep
    /// running until the last clone drops, then exit on their own once
    /// `recv()` starts erroring — no explicit shutdown signal needed.
    /// `close` is the owning
    /// client's courtesy notice that its session is ending, not a hard
    /// resource release: the catalog connection closes only once the last
    /// clone drops, same as any `Arc`-backed handle.
    pub fn close(self) -> Result<()> {
        self.emit(Event::LibraryClosed);
        Ok(())
    }

    /// Sends an event to every live subscriber, dropping dead ones.
    fn emit(&self, event: Event) {
        lock(&self.inner.subscribers).retain(|s| s.send(event.clone()).is_ok());
    }

    /// A process-unique id for a new job.
    fn new_job(&self) -> JobId {
        JobId::new(self.inner.next_job.fetch_add(1, Ordering::Relaxed))
    }

    /// Read access to the catalog: grid queries, trees, histories.
    pub fn catalog(&self) -> CatalogRead<'_> {
        CatalogRead(lock(&self.inner.catalog))
    }

    /// Write access to the catalog: classement, keywords, collections...
    ///
    /// The facade adds orchestration only where several stores cooperate;
    /// pure catalog operations pass through undecorated.
    pub fn catalog_mut(&self) -> CatalogWrite<'_> {
        CatalogWrite(lock(&self.inner.catalog))
    }

    /// Emits one `VersionChanged` per version, after a classement write.
    fn emit_versions_changed(&self, versions: &[VersionId]) {
        for &version in versions {
            self.emit(Event::VersionChanged {
                version_id: version,
            });
        }
    }

    /// Rates a batch of versions (`docs/engine-api.md` §8); `None` clears.
    /// Emits `VersionChanged` per version.
    pub fn set_rating(&self, versions: &[VersionId], rating: Option<u8>) -> Result<()> {
        self.catalog_mut().set_rating(versions, rating)?;
        self.emit_versions_changed(versions);
        Ok(())
    }

    /// Labels a batch of versions (§8); `None` clears. Emits
    /// `VersionChanged` per version.
    pub fn set_color_label(&self, versions: &[VersionId], label: Option<ColorLabel>) -> Result<()> {
        self.catalog_mut().set_color_label(versions, label)?;
        self.emit_versions_changed(versions);
        Ok(())
    }

    /// Flags a batch of versions (§8). Emits `VersionChanged` per version.
    pub fn set_pick(&self, versions: &[VersionId], pick: PickState) -> Result<()> {
        self.catalog_mut().set_pick(versions, pick)?;
        self.emit_versions_changed(versions);
        Ok(())
    }

    /// Tags a batch of assets (§8, idempotent). Emits `AssetsChanged`.
    pub fn add_keyword(&self, assets: &[AssetId], keyword: KeywordId) -> Result<()> {
        self.catalog_mut().add_keyword(assets, keyword)?;
        self.emit(Event::AssetsChanged {
            asset_ids: assets.to_vec(),
        });
        Ok(())
    }

    /// Untags a batch of assets (§8; removing an absent tag is a no-op).
    /// Emits `AssetsChanged`.
    pub fn remove_keyword(&self, assets: &[AssetId], keyword: KeywordId) -> Result<()> {
        self.catalog_mut().remove_keyword(assets, keyword)?;
        self.emit(Event::AssetsChanged {
            asset_ids: assets.to_vec(),
        });
        Ok(())
    }

    /// Creates a keyword under `parent`, or at the root (§8).
    pub fn create_keyword(&self, parent: Option<KeywordId>, name: &str) -> Result<KeywordId> {
        self.catalog_mut().create_keyword(parent, name)
    }

    /// The complete keyword tree, siblings ordered by name (§8).
    pub fn keyword_tree(&self) -> Result<Vec<KeywordNode>> {
        self.catalog().keyword_tree()
    }

    /// Creates a manual collection under `parent`, or at the root (§9).
    pub fn create_collection(
        &self,
        parent: Option<CollectionId>,
        name: &str,
    ) -> Result<CollectionId> {
        self.catalog_mut().create_collection(parent, name)
    }

    /// Creates a smart collection driven by `rules` (§9, catalogue §26).
    pub fn create_smart_collection(
        &self,
        parent: Option<CollectionId>,
        name: &str,
        rules: &SmartRules,
    ) -> Result<CollectionId> {
        self.catalog_mut()
            .create_smart_collection(parent, name, rules)
    }

    /// Adds versions to a manual collection (§9).
    pub fn add_to_collection(&self, id: CollectionId, versions: &[VersionId]) -> Result<()> {
        self.catalog_mut().add_to_collection(id, versions)
    }

    /// Removes versions from a manual collection (§9).
    pub fn remove_from_collection(&self, id: CollectionId, versions: &[VersionId]) -> Result<()> {
        self.catalog_mut().remove_from_collection(id, versions)
    }

    /// The collection tree, children under their parents (§9).
    pub fn collections(&self) -> Result<Vec<CollectionNode>> {
        self.catalog().collections()
    }

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
    pub fn import(
        &self,
        source: &Path,
        options: &ImportOptions,
        progress: impl FnMut(u64, u64),
    ) -> Result<ImportReport> {
        let report = {
            let mut catalog = lock(&self.inner.catalog);
            crate::import::import(&mut catalog, &self.inner.root, source, options, progress)
        }?;
        self.generate_import_thumbnails(&report.imported);
        Ok(report)
    }

    /// Best-effort thumbnail pass for freshly imported assets (§6, §11).
    ///
    /// Run serially: [`Library::preview`] holds the catalog lock for the
    /// whole decode+render of each asset (the decode cache is a small LRU
    /// too, not built for concurrent renders), so parallelizing here would
    /// need deeper changes to that locking, not just a `rayon` iterator
    /// over this loop — the calls would simply serialize on the catalog
    /// mutex today. Left serial; worth revisiting if import-time
    /// thumbnailing shows up in the perf benches.
    fn generate_import_thumbnails(&self, imported: &[ImportedFile]) {
        for file in imported {
            let _ = self.preview(file.registered.asset, PreviewKind::Thumbnail);
        }
    }

    /// Imports files as a job (§3.1): returns immediately, progresses as
    /// `JobProgress` per candidate file, then `AssetsAdded` and
    /// `JobFinished` with the report.
    pub fn import_async(&self, source: &Path, options: &ImportOptions) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        let source = source.to_owned();
        let options = *options;
        self.spawn_job(move || {
            let imported = library.import(&source, &options, |done, total| {
                library.emit(Event::JobProgress {
                    job_id: job,
                    done,
                    total,
                });
            });
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
            library.emit(Event::JobFinished {
                job_id: job,
                result,
            });
        });
        job
    }

    /// Returns the preview of the asset's current version, rendering it into
    /// the cache first when nothing valid exists (§11).
    ///
    /// Unlike the free `preview::preview` this crate's tests use, this
    /// method deliberately does *not* hold the catalog lock across the
    /// render: the render touches no catalog state, so holding it there
    /// would block every other client's catalog access (grid, search,
    /// metadata edits) for the render's full duration, including under the
    /// bounded render pool's concurrent `preview_async` jobs (ADR 0023).
    /// `preview::record_render`'s settings-match guard is what keeps this
    /// safe against a concurrent amendment landing while the render is in
    /// flight.
    pub fn preview(&self, asset: AssetId, kind: PreviewKind) -> Result<PreviewFile> {
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::preview::plan_preview(
                &catalog,
                &self.inner.cache,
                &self.inner.root,
                asset,
                kind,
            )?
        };
        let plan = match plan {
            crate::preview::PreviewPlan::Cached(file) => return Ok(file),
            crate::preview::PreviewPlan::Render(plan) => plan,
        };
        let image = {
            let mut decodes = lock(&self.inner.decodes);
            crate::preview::render_preview(&mut decodes, asset, &plan)?
        };
        let mut catalog = lock(&self.inner.catalog);
        crate::preview::record_render(&mut catalog, &self.inner.cache, asset, kind, &plan, &image)
    }

    /// Renders a preview as a job (§3.1, §11): returns immediately, then
    /// `PreviewReady` on success and `JobFinished` either way. A cache hit
    /// still emits both — the client logic stays uniform.
    pub fn preview_async(&self, asset: AssetId, kind: PreviewKind) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let result = match library.preview(asset, kind) {
                Ok(file) => {
                    library.emit(Event::PreviewReady {
                        asset_id: asset,
                        kind,
                    });
                    JobResult::Preview(file)
                }
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.emit(Event::JobFinished {
                job_id: job,
                result,
            });
        });
        job
    }

    /// Returns the cached preview of the asset's current version when a
    /// valid one exists, without ever rendering (§11). Lets a client fill
    /// what is already on disk instantly and schedule the rest.
    pub fn cached_preview(&self, asset: AssetId, kind: PreviewKind) -> Result<Option<PreviewFile>> {
        Ok(self
            .catalog()
            .valid_preview(asset, kind)?
            .map(|row| PreviewFile {
                path: self.inner.cache.absolute_path(&row.relative_path),
                width: row.width,
                height: row.height,
                freshly_generated: false,
            }))
    }

    /// Fuses `preview`, `cached_preview` and `preview_async` into one
    /// convenience call (§11): the client always has something to display
    /// immediately, except when nothing has ever been cached.
    ///
    /// - A valid cache hit (head revision) comes back as [`Preview::Ready`],
    ///   no job started.
    /// - A cache entry for an older revision comes back as
    ///   [`Preview::Stale`]: display it right away, a regeneration job is
    ///   already running, watch `PreviewReady`/`JobFinished` (§3.2) for the
    ///   fresh file.
    /// - Nothing cached at all comes back as [`Preview::Generating`]: a job
    ///   was started, nothing to show until it finishes.
    pub fn preview_state(&self, asset: AssetId, kind: PreviewKind) -> Result<Preview> {
        if let Some(row) = self.catalog().valid_preview(asset, kind)? {
            return Ok(Preview::Ready(
                self.inner.cache.absolute_path(&row.relative_path),
            ));
        }
        if let Some(row) = self.catalog().latest_preview(asset, kind)? {
            let path = self.inner.cache.absolute_path(&row.relative_path);
            let job = self.preview_async(asset, kind);
            return Ok(Preview::Stale { path, job });
        }
        Ok(Preview::Generating(self.preview_async(asset, kind)))
    }

    /// Exports a version at its head revision (§12) and returns the file.
    ///
    /// Like [`Library::preview`], this deliberately does *not* hold the
    /// catalog lock across the render: decode + `process1`–`5` + encode can
    /// take seconds, and a batch export runs this per version, so holding
    /// the lock there would freeze all of Studio's navigation, search and
    /// metadata edits for the whole export instead of just the version at
    /// hand (ADR 0024). Unlike a preview, an exported file is a one-shot
    /// artifact the caller asked for — not a revision-keyed cache another
    /// call might later serve as "current" — so no amendment guard is
    /// needed: the file is simply journaled once rendered.
    pub fn export(
        &self,
        version: VersionId,
        settings: &ExportSettings,
        preset: Option<ExportPresetId>,
        destination_dir: &Path,
    ) -> Result<PathBuf> {
        settings.validate().map_err(crate::export::export_err)?;
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::export::plan_export(&catalog, &self.inner.root, version)?
        };
        let destination = crate::export::render_export(&plan, settings, destination_dir)?;
        let mut catalog = lock(&self.inner.catalog);
        crate::export::journal_export(&mut catalog, &plan, preset, settings, &destination)?;
        Ok(destination)
    }

    /// Exports several versions with one ad-hoc recipe (§12). `progress`
    /// receives `(done, total)` per version; one failure does not stop the
    /// batch. The catalog lock is narrowed per version, not held for the
    /// whole batch (ADR 0024) — see [`Library::export`].
    pub fn export_batch(
        &self,
        versions: &[VersionId],
        settings: &ExportSettings,
        destination_dir: &Path,
        progress: impl FnMut(u64, u64),
    ) -> Result<ExportReport> {
        self.export_batch_with_preset(versions, settings, None, destination_dir, progress)
    }

    /// Shared core of [`Library::export_batch`] and
    /// [`Library::export_with_preset`]: exports each version through
    /// [`Library::export`], so the catalog lock never spans more than one
    /// version's render (ADR 0024).
    fn export_batch_with_preset(
        &self,
        versions: &[VersionId],
        settings: &ExportSettings,
        preset: Option<ExportPresetId>,
        destination_dir: &Path,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<ExportReport> {
        settings.validate().map_err(crate::export::export_err)?;
        let total = versions.len() as u64;
        let mut report = ExportReport::default();
        for (done, &version) in versions.iter().enumerate() {
            match self.export(version, settings, preset, destination_dir) {
                Ok(path) => report
                    .exported
                    .push(crate::export::ExportedVersion { version, path }),
                Err(error) => report.failed.push(crate::export::FailedExport {
                    version,
                    reason: error.to_string(),
                }),
            }
            progress(done as u64 + 1, total);
        }
        Ok(report)
    }

    /// Exports several versions as a job (§3.1, §12): returns immediately,
    /// progresses as `JobProgress` per version, then `JobFinished` with the
    /// report (per-version failures inside it, batch failures as `Failed`).
    pub fn export_async(
        &self,
        versions: Vec<VersionId>,
        settings: ExportSettings,
        destination_dir: PathBuf,
    ) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let exported = library.export_batch(&versions, &settings, &destination_dir, {
                let library = library.clone();
                move |done, total| {
                    library.emit(Event::JobProgress {
                        job_id: job,
                        done,
                        total,
                    });
                }
            });
            let result = match exported {
                Ok(report) => JobResult::Export(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.emit(Event::JobFinished {
                job_id: job,
                result,
            });
        });
        job
    }

    /// Exports several versions with a stored preset, as a job (§3.1,
    /// §12): same event contract as [`Library::export_async`].
    pub fn export_with_preset_async(
        &self,
        versions: Vec<VersionId>,
        preset: ExportPresetId,
        destination_dir: PathBuf,
    ) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let exported = library.export_with_preset(&versions, preset, &destination_dir, {
                let library = library.clone();
                move |done, total| {
                    library.emit(Event::JobProgress {
                        job_id: job,
                        done,
                        total,
                    });
                }
            });
            let result = match exported {
                Ok(report) => JobResult::Export(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.emit(Event::JobFinished {
                job_id: job,
                result,
            });
        });
        job
    }

    /// Exports several versions with a stored preset (§12): the preset's
    /// recipe drives the batch and each success is journaled against it.
    /// Only the preset lookup holds the catalog lock up front; each
    /// version's render is narrowed the same way as
    /// [`Library::export_batch`] (ADR 0024).
    pub fn export_with_preset(
        &self,
        versions: &[VersionId],
        preset: ExportPresetId,
        destination_dir: &Path,
        progress: impl FnMut(u64, u64),
    ) -> Result<ExportReport> {
        let settings = {
            let catalog = lock(&self.inner.catalog);
            let stored = catalog.export_preset(preset)?;
            ExportSettings::parse(&stored.settings_json).map_err(crate::export::export_err)?
        };
        self.export_batch_with_preset(versions, &settings, Some(preset), destination_dir, progress)
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

    /// Opens an edit session on a version (§10.1). The session holds the
    /// catalog lock: nothing else mutates while editing, so keep sessions
    /// short — Studio opens one per commit point. Every history write of
    /// the session emits `VersionChanged` (§3.2).
    pub fn edit(&self, version: VersionId) -> Result<EditSession<CatalogWrite<'_>>> {
        let library = self.clone();
        Ok(EditSession::open(self.catalog_mut(), version)?
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

    /// Lists every stored develop preset, ordered by name (§10.3).
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
    pub fn apply_preset(
        &self,
        preset: PresetId,
        versions: &[VersionId],
        mut progress: impl FnMut(u64, u64),
    ) -> Result<PresetApplyReport> {
        let stored = self.catalog().preset(preset)?;
        let fields = PresetSettings::parse(&stored.preset_json)?;
        let mut catalog = lock(&self.inner.catalog);
        let report = crate::presets::apply_batch(&mut catalog, &fields, versions, &mut progress);
        drop(catalog);
        for &version in &report.applied {
            self.emit(Event::VersionChanged {
                version_id: version,
            });
        }
        Ok(report)
    }

    /// Applies a stored preset to several versions as a job (§3.1, §10.3):
    /// returns immediately, progresses as `JobProgress` per version, then
    /// `JobFinished` with the report (per-version failures inside it, batch
    /// failures — e.g. an unknown preset — as `Failed`).
    pub fn apply_preset_async(&self, preset: PresetId, versions: Vec<VersionId>) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let applied = library.apply_preset(preset, &versions, {
                let library = library.clone();
                move |done, total| {
                    library.emit(Event::JobProgress {
                        job_id: job,
                        done,
                        total,
                    });
                }
            });
            let result = match applied {
                Ok(report) => JobResult::Preset(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.emit(Event::JobFinished {
                job_id: job,
                result,
            });
        });
        job
    }

    /// Migrates several versions to the engine's current process version
    /// (`docs/engine-api.md` §10.4, `docs/pipeline.md` §4.5): the
    /// synchronous core. One fresh `EditSession` per version, so one
    /// `VersionChanged` per migration — prefer [`Library::reprocess_async`]
    /// from interactive clients.
    pub fn reprocess(
        &self,
        versions: &[VersionId],
        mut progress: impl FnMut(u64, u64),
    ) -> Result<ReprocessReport> {
        let mut catalog = lock(&self.inner.catalog);
        let report = crate::reprocess::reprocess_batch(&mut catalog, versions, &mut progress);
        drop(catalog);
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
            let migrated = library.reprocess(&versions, {
                let library = library.clone();
                move |done, total| {
                    library.emit(Event::JobProgress {
                        job_id: job,
                        done,
                        total,
                    });
                }
            });
            let result = match migrated {
                Ok(report) => JobResult::Reprocess(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.emit(Event::JobFinished {
                job_id: job,
                result,
            });
        });
        job
    }

    /// Writes the XMP sidecar of an asset — the On Demand synchronization
    /// of `docs/catalog.md` §29 — and returns its path.
    pub fn write_xmp(&self, asset: AssetId) -> Result<PathBuf> {
        crate::xmp::write_xmp_sidecar(&self.catalog(), &self.inner.root, asset)
    }
}

/// Locks a mutex, recovering the data if a previous holder panicked: the
/// catalog is transactional, a poisoned lock carries no torn state.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Unit tests for the job pool itself (§3.3): they use `spawn_job`
/// directly to submit synthetic work, unlike `tests/jobs.rs` which only
/// exercises the public `*_async` facade and its event contract.
#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    fn open_test_library(name: &str) -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::create(&dir.path().join("Library"), name).unwrap();
        (dir, library)
    }

    /// Submits `count` synthetic jobs straight to `library`'s job pool and
    /// blocks until every one of them has run `body` — bypassing the
    /// `*_async` facade entirely, since what's under test here is the
    /// pool's own concurrency bound, not any one job kind's behavior.
    fn run_on_job_pool(library: &Library, count: usize, body: impl Fn() + Send + Sync + 'static) {
        let body = Arc::new(body);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        for _ in 0..count {
            let body = Arc::clone(&body);
            let done_tx = done_tx.clone();
            library.spawn_job(move || {
                body();
                done_tx.send(()).unwrap();
            });
        }
        for _ in 0..count {
            // Generous: this test and its sibling both saturate the pool's
            // full width at once, and `cargo test` runs them (and every
            // other test in the crate) concurrently — on a machine with
            // exactly `width` cores that's real, if temporary, contention,
            // not a hang.
            done_rx
                .recv_timeout(Duration::from_secs(30))
                .expect("every job submitted to the job pool must finish");
        }
    }

    /// Submitting exactly as many jobs as the pool has workers must let
    /// them all run at once — the bound is a ceiling, not an accidental
    /// serialization down to one thread.
    #[test]
    fn job_pool_reaches_its_configured_width() {
        let (_dir, library) = open_test_library("PoolWidth");
        let width = job_pool_size();

        let running = Arc::new(AtomicUsize::new(0));
        let max_running = Arc::new(AtomicUsize::new(0));
        // Every job must reach the barrier before any of them is released:
        // that only happens if `width` of them are genuinely in flight
        // together.
        let barrier = Arc::new(Barrier::new(width));

        let running2 = Arc::clone(&running);
        let max2 = Arc::clone(&max_running);
        let barrier2 = Arc::clone(&barrier);
        run_on_job_pool(&library, width, move || {
            let now = running2.fetch_add(1, Ordering::SeqCst) + 1;
            max2.fetch_max(now, Ordering::SeqCst);
            barrier2.wait();
            running2.fetch_sub(1, Ordering::SeqCst);
        });

        assert_eq!(max_running.load(Ordering::SeqCst), width);
    }

    /// Submitting far more jobs than the pool has workers must still
    /// complete every one of them (queued, not dropped), and the observed
    /// concurrency must never exceed the pool's configured width — the
    /// ceiling this whole change exists to enforce.
    #[test]
    fn job_pool_never_exceeds_its_configured_width() {
        let (_dir, library) = open_test_library("PoolBound");
        let width = job_pool_size();
        let rounds = 3;

        let running = Arc::new(AtomicUsize::new(0));
        let max_running = Arc::new(AtomicUsize::new(0));
        // Reused across `rounds` batches of `width`: std's Barrier is
        // cyclic, so each full batch synchronizes and releases in turn.
        let barrier = Arc::new(Barrier::new(width));

        let running2 = Arc::clone(&running);
        let max2 = Arc::clone(&max_running);
        let barrier2 = Arc::clone(&barrier);
        run_on_job_pool(&library, width * rounds, move || {
            let now = running2.fetch_add(1, Ordering::SeqCst) + 1;
            max2.fetch_max(now, Ordering::SeqCst);
            barrier2.wait();
            running2.fetch_sub(1, Ordering::SeqCst);
        });

        assert_eq!(running.load(Ordering::SeqCst), 0, "every job must finish");
        assert_eq!(
            max_running.load(Ordering::SeqCst),
            width,
            "concurrency must reach the pool width but never exceed it"
        );
    }
}
