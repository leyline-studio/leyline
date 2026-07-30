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

use leyline_catalog::{
    Catalog, CollectionNode, ExportPreset, KeywordNode, Preset, PrintPreset, SmartRules,
};
use leyline_core::{
    AssetId, CollectionId, ColorLabel, ExportPresetId, JobId, KeywordId, LeylineError, PickState,
    PresetId, PresetSettings, PreviewKind, PrintPresetId, Result, Settings, SettingsGroup,
    VersionId,
};
use leyline_export::{ExportSettings, PrintSettings};
use leyline_preview::PreviewCache;

use crate::decode_cache::DecodeCache;
use crate::events::{Event, JobResult};
use crate::export::{ExportReport, ExportRequest};
use crate::import::{ImportOptions, ImportReport, ImportedFile};
use crate::presets::PresetApplyReport;
use crate::preview::{Preview, PreviewFile};
use crate::print::{PrintRecipe, PrintReport, PrintRequest};
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
    /// The running tether session (`docs/adr/0038`), if any. One camera at
    /// a time per library — connecting while this is `Some` is refused.
    #[cfg(feature = "tether")]
    tether: Mutex<Option<leyline_tether::TetherSession>>,
    /// The running watched-folder session (`docs/adr/0039`), if any. One
    /// watched folder at a time per library — starting while this is
    /// `Some` is refused.
    watch: Mutex<Option<crate::watch::WatchSession>>,
    /// The active map pack (`docs/adr/0040`), opened lazily on first
    /// access and cached — `None` covers both "never opened yet" and "no
    /// pack file exists", cheap to tell apart with one `is_file` check.
    /// `import_map_pack` resets this to force a reopen against the new
    /// file.
    map_pack: Mutex<Option<leyline_map::TilePack>>,
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

/// Where [`Library::import_camera_profile`] copied a `.dcp` file to, and
/// its checksum — everything a caller needs to build the
/// `leyline_core::CameraProfile` it stores in a revision's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedCameraProfile {
    /// Library-relative path, e.g. `Profiles/Camera/Canon EOS 5D Mark III.dcp`.
    pub relative_path: String,
    /// `"blake3:"` followed by 64 hex digits — ready to store as
    /// [`leyline_core::CameraProfile::checksum`].
    pub checksum: String,
}

/// Where [`Library::import_lut`] copied a `.cube` file to, and its checksum —
/// everything a caller needs to build the `leyline_core::Lut` it stores in a
/// revision's settings (ADR 0053 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedLut {
    /// Library-relative path, e.g. `Profiles/LUT/Kodachrome.cube`.
    pub relative_path: String,
    /// `"blake3:"` followed by 64 hex digits.
    pub checksum: String,
}

/// A screen soft-proof request (ADR 0034): the destination to simulate, how
/// to get there, and whether to flag what it cannot reproduce.
///
/// An argument of [`Library::preview_soft_proofed`], never a stored value: a
/// proof is a way of looking at a photo. Nothing here reaches a revision, a
/// preset or the preview cache (ADR 0051 §4).
#[derive(Debug, Clone, PartialEq)]
pub struct SoftProof {
    /// The destination ICC profile to simulate — a printer profile, a press
    /// profile, another display. Read at each call; the user picks the file,
    /// so no library-relative rule applies (nothing is stored).
    pub profile: PathBuf,
    /// How colors are mapped on the way to that destination.
    pub intent: leyline_color::RenderingIntent,
    /// Paint what the destination cannot reproduce in LittleCMS's alarm color
    /// instead of clipping it silently.
    pub gamut_warning: bool,
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
                #[cfg(feature = "tether")]
                tether: Mutex::new(None),
                watch: Mutex::new(None),
                map_pack: Mutex::new(None),
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

    /// Per-channel 8-bit histogram (`[R, G, B]`, 256 bins each) of the
    /// asset's current develop preview at `kind` — reads back whatever
    /// [`Library::preview`] already rendered/cached rather than re-running
    /// the develop pipeline a second time.
    pub fn histogram(&self, asset: AssetId, kind: PreviewKind) -> Result<[[u32; 256]; 3]> {
        let file = self.preview(asset, kind)?;
        let image = leyline_preview::Rgb8::load_png(&file.path).map_err(|e| {
            LeylineError::DecodeFailed {
                asset,
                reason: e.to_string(),
            }
        })?;
        Ok(image.histogram())
    }

    /// Renders the asset's image at neutral (as-shot) settings, scaled like
    /// `kind` — the "before" half of develop's before/after comparison. A
    /// one-off render: unlike [`Library::preview`], this never touches the
    /// preview cache and is never recorded as any revision's valid preview,
    /// since it deliberately isn't the head's settings.
    pub fn preview_before(
        &self,
        asset: AssetId,
        kind: PreviewKind,
    ) -> Result<leyline_preview::Rgb8> {
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::preview::plan_settings_render(&catalog, &self.inner.root, asset, kind)?
        };
        let image = {
            let mut decodes = lock(&self.inner.decodes);
            crate::preview::render_with_settings(&mut decodes, asset, &plan, &Settings::default())?
        };
        Ok(match leyline_preview::max_edge(kind) {
            Some(edge) => image.scaled_to_fit(edge),
            None => image,
        })
    }

    /// Renders the asset's current preview as it would appear once it had been
    /// through `proof`'s destination profile — screen soft-proofing (ADR 0034,
    /// ADR 0051 §4).
    ///
    /// In memory, like [`Library::preview_before`] and for the same reason: a
    /// proof is a way of *looking* at a photo, not a version of it. Nothing is
    /// written — no preview cache entry, no revision, no preset — so a proofed
    /// look can never be mistaken later for what the photo is.
    ///
    /// The image is the cached preview when there is one, so proofing costs a
    /// color transform rather than a develop pass.
    pub fn preview_soft_proofed(
        &self,
        asset: AssetId,
        kind: PreviewKind,
        proof: &SoftProof,
    ) -> Result<leyline_preview::Rgb8> {
        let transform = leyline_color::SoftProofTransform::load(
            &proof.profile,
            proof.intent,
            proof.gamut_warning,
        )
        .map_err(|e| LeylineError::InvalidSettings(e.to_string()))?;
        let file = self.preview(asset, kind)?;
        let mut image = leyline_preview::Rgb8::load_png(&file.path).map_err(|e| {
            LeylineError::DecodeFailed {
                asset,
                reason: e.to_string(),
            }
        })?;
        transform.apply(image.data_mut());
        Ok(image)
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
    pub fn export(
        &self,
        request: &ExportRequest,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<ExportReport> {
        let (settings, preset) = self.resolve_export_recipe(&request.recipe)?;
        settings.validate().map_err(crate::export::export_err)?;
        let total = request.versions.len() as u64;
        let mut report = ExportReport::default();
        for (done, &version) in request.versions.iter().enumerate() {
            match self.export_one(version, &settings, preset, &request.destination_dir) {
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

    /// Exports `request` as a job (§3.1, §12): returns immediately,
    /// progresses as `JobProgress` per version, then `JobFinished` with the
    /// report (per-version failures inside it, request-level failures as
    /// `Failed`).
    pub fn export_async(&self, request: ExportRequest) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let exported = library.export(&request, {
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

    /// Resolves a recipe to the settings that drive the render, and the
    /// preset id (if any) each export journals against.
    fn resolve_export_recipe(
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

    /// Exports one version at its head revision and returns the written
    /// file — the per-version core [`Library::export`] loops over, with the
    /// catalog lock narrowed to the plan and the journal (ADR 0024).
    fn export_one(
        &self,
        version: VersionId,
        settings: &ExportSettings,
        preset: Option<ExportPresetId>,
        destination_dir: &Path,
    ) -> Result<PathBuf> {
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::export::plan_export(&catalog, &self.inner.root, version)?
        };
        let destination = crate::export::render_export(&plan, settings, destination_dir)?;
        let mut catalog = lock(&self.inner.catalog);
        crate::export::journal_export(&mut catalog, &plan, preset, settings, &destination)?;
        Ok(destination)
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

    /// Prints every version of `request` through its recipe (ADR 0036) —
    /// same shape and same catalog-lock discipline as [`Library::export`]:
    /// decode+render+scale+encode never holds the catalog lock, one failing
    /// version doesn't stop the batch, and a [`PrintRecipe::Preset`] is
    /// resolved once up front. Unlike export there is no journal step: a
    /// print has no history table (ADR 0036).
    pub fn print(
        &self,
        request: &PrintRequest,
        mut progress: impl FnMut(u64, u64),
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
            progress(done as u64 + 1, total);
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
            let printed = library.print(&request, {
                let library = library.clone();
                move |done, total| {
                    library.emit(Event::JobProgress {
                        job_id: job,
                        done,
                        total,
                    });
                }
            });
            let result = match printed {
                Ok(report) => JobResult::Print(report),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.emit(Event::JobFinished {
                job_id: job,
                result,
            });
        });
        job
    }

    /// Resolves a recipe to the settings that drive the render.
    fn resolve_print_recipe(&self, recipe: &PrintRecipe) -> Result<PrintSettings> {
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
    fn print_one(
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
    pub fn apply_settings(
        &self,
        settings: &PresetSettings,
        versions: &[VersionId],
        mut progress: impl FnMut(u64, u64),
    ) -> Result<PresetApplyReport> {
        let mut catalog = lock(&self.inner.catalog);
        let report = crate::presets::apply_batch(&mut catalog, settings, versions, &mut progress);
        drop(catalog);
        for &version in &report.applied {
            self.emit(Event::VersionChanged {
                version_id: version,
            });
        }
        Ok(report)
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

    /// Connects to the first USB camera libgphoto2 finds and starts a
    /// tether session (`docs/adr/0038-tethered-capture.md`): every shot the
    /// camera reports from here on is downloaded and imported automatically
    /// — a tethered shot is not a distinct kind of asset, just a different
    /// import source, so it lands in the catalog exactly like a file
    /// dropped into a watched folder. Emits `Event::TetherConnected` on
    /// success, then one `Event::AssetsAdded` per captured shot (the same
    /// event a normal import fires), then `Event::TetherDisconnected` once
    /// the session ends (`tether_disconnect`, an unplug, or a transport
    /// error).
    ///
    /// Refuses a second session while one is already open: one camera at a
    /// time per library in V1 (`docs/adr/0038`).
    #[cfg(feature = "tether")]
    pub fn tether_connect(&self) -> Result<()> {
        let mut slot = lock(&self.inner.tether);
        if slot.is_some() {
            return Err(LeylineError::Tether(
                "a tether session is already open on this library".to_owned(),
            ));
        }
        let staging = self.inner.root.join("Cache").join("Tether");
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
    pub fn tether_connect(&self) -> Result<()> {
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

    /// Turns one `leyline_tether::TetherEvent` into catalog state and an
    /// engine event (`docs/adr/0038`). Runs on the tether session's own
    /// background thread.
    #[cfg(feature = "tether")]
    fn handle_tether_event(&self, event: leyline_tether::TetherEvent) {
        match event {
            leyline_tether::TetherEvent::Captured(file) => {
                let options = ImportOptions {
                    copy_files: true,
                    recursive: false,
                };
                // A failed import of one captured shot (e.g. an undecodable
                // file) is dropped rather than surfaced as a disconnect —
                // same best-effort stance the import core already takes
                // for a thumbnail render failing after a successful import.
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
                // The import core already copied the bytes into `Photos/`
                // (`copy_files: true` above); leaving the staged copy
                // behind would grow `Cache/Tether/` without bound for the
                // life of the session.
                let _ = std::fs::remove_file(&file.path);
            }
            leyline_tether::TetherEvent::Disconnected(reason) => {
                lock(&self.inner.tether).take();
                self.emit(Event::TetherDisconnected { reason });
            }
        }
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
    fn handle_watch_event(&self, event: crate::watch::WatchSessionEvent) {
        match event {
            crate::watch::WatchSessionEvent::Ready(file) => {
                let options = ImportOptions {
                    copy_files: true,
                    recursive: false,
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

    /// The library-relative path of the active map pack file (ADR 0040) —
    /// a convention, not a catalog row: presence of the file is what makes
    /// a pack "active", one at a time in V1.
    fn map_pack_path(&self) -> PathBuf {
        self.inner.root.join("Map").join("pack.mbtiles")
    }

    /// Imports `source` as the library's active map pack (ADR 0040): copies
    /// it to the conventional `Map/pack.mbtiles` location, overwriting
    /// whatever pack (if any) was active before. Refused on a read-only
    /// handle (`Library::open_read_only`) — same contract as every catalog
    /// write, even though a map pack is filesystem state, not a catalog
    /// row.
    pub fn import_map_pack(&self, source: &Path) -> Result<()> {
        if self.catalog().is_read_only() {
            return Err(LeylineError::Db(
                "library opened read-only; writes are refused".to_owned(),
            ));
        }
        let destination = self.map_pack_path();
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Invalidate the cached handle *before* touching the file: with the
        // old order, a pack already opened once (any prior `map_tile`/
        // `map_pack_info` call) still had a live SQLite connection to
        // `destination` while it got overwritten out from under it — risky
        // on every platform if the copy is interrupted partway, outright
        // liable to fail on Windows where replacing an open file needs
        // share-delete cooperation SQLite doesn't request.
        lock(&self.inner.map_pack).take();
        // Copy to a sibling temp file first, then move it into place: a
        // reader that opens the pack mid-copy must never see a half-written
        // file. The temp name carries the process id so two Leyline
        // instances importing at once cannot collide on it, and so a
        // crashed import leaves an obviously-stale file rather than one the
        // next import might mistake for its own.
        let temp = destination.with_extension(format!("mbtiles.{}.tmp", std::process::id()));
        std::fs::copy(source, &temp)?;
        // `std::fs::rename` replaces the destination on POSIX but *fails*
        // on Windows when it already exists, so a re-import has to unlink
        // first. That leaves a brief window with no active pack; it is the
        // narrowest one available without platform-specific APIs, and the
        // cached handle is already dropped by this point either way.
        if destination.exists() {
            std::fs::remove_file(&destination)?;
        }
        if let Err(error) = std::fs::rename(&temp, &destination) {
            // Don't leave the copy behind to be mistaken for a pack.
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
        Ok(())
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

    /// Opens (or returns the cached handle to) the active map pack, or
    /// `None` when none has been imported yet.
    fn with_map_pack<T>(&self, f: impl FnOnce(&leyline_map::TilePack) -> T) -> Result<Option<T>> {
        let mut slot = lock(&self.inner.map_pack);
        if slot.is_none() {
            let path = self.map_pack_path();
            if !path.is_file() {
                return Ok(None);
            }
            let pack = leyline_map::TilePack::open(&path).map_err(map_err)?;
            *slot = Some(pack);
        }
        Ok(slot.as_ref().map(f))
    }

    /// The active map pack's declared coverage (ADR 0040), or `None` if
    /// none has been imported yet.
    pub fn map_pack_info(&self) -> Result<Option<leyline_map::TilePackInfo>> {
        self.with_map_pack(|pack| pack.info().map_err(map_err))?
            .transpose()
    }

    /// One tile's raw bytes (whatever image format the pack stores), or
    /// `None` when no pack is imported or the tile is outside its
    /// coverage.
    pub fn map_tile(&self, zoom: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>> {
        Ok(self
            .with_map_pack(|pack| pack.tile(zoom, x, y).map_err(map_err))?
            .transpose()?
            .flatten())
    }

    /// Every current version with recorded GPS coordinates (ADR 0040).
    pub fn map_pins(&self) -> Result<Vec<leyline_catalog::MapPin>> {
        self.catalog().map_pins()
    }
}

/// Maps a `leyline_map` error onto the platform error type.
fn map_err(error: leyline_map::MapError) -> LeylineError {
    LeylineError::Db(error.to_string())
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
