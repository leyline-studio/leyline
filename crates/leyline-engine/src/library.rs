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
    Catalog, CollectionNode, ExportPreset, FolderNode, KeywordNode, LibraryInfo, Preset,
    PresetFolder, PrintPreset, Root, SmartRules,
};
use leyline_core::{
    AssetId, CameraSettings, CollectionId, ColorLabel, ExportPresetId, JobId, KeywordId,
    LeylineError, PickState, PresetFolderId, PresetId, PresetSettings, PreviewKind, PrintPresetId,
    Result, Settings, SettingsGroup, TetherSetting, VersionId, WhiteBalance,
};
use leyline_export::{ExportSettings, PrintSettings};
use leyline_preview::PreviewCache;

use crate::auto_tone::AutoTone;
use crate::decode_cache::DecodeCache;
use crate::events::{Event, JobResult};
use crate::export::{ExportReport, ExportRequest};
use crate::import::{ImportOptions, ImportReport, ImportedFile};
use crate::presets::PresetApplyReport;
use crate::preview::{Preview, PreviewFile};
use crate::print::{PrintRecipe, PrintReport, PrintRequest};
use crate::reprocess::ReprocessReport;
use crate::scan::{ImportCandidate, ScanOptions};
use crate::session::EditSession;
use crate::wb::RangeSample;

/// Decoded images kept in memory for preview renders. Two covers the
/// develop loop (the edited asset, at worst in two size classes) while
/// bounding memory: a full-size 24 MP decode is ~72 MB.
const DECODE_CACHE_CAPACITY: usize = 2;

/// Upper bound on the job pool (§3.3) regardless of core count: past a few
/// dozen threads the catalog mutex and disk I/O dominate anyway, so a very
/// high core count desktop gains nothing from an even wider pool.
const JOB_POOL_MAX_THREADS: usize = 16;

/// One version of a planned batch: where to write it, or why it was refused
/// before anything was decoded (ADR 0068 §2). The reason is carried as text
/// because a slot is read once per version, long after the catalog lock that
/// produced it is gone.
type PlannedExport = std::result::Result<(crate::export::ExportPlan, PathBuf), String>;

/// Ceiling on the photos an export batch keeps in flight by default
/// (ADR 0068 §1). The default itself is [`default_export_concurrency`],
/// which lowers this on a machine with fewer cores.
///
/// One photo's pipeline cannot fill a modern machine — a 12-file batch of
/// 30 Mpx RAWs measured 280 % of 1600 % on sixteen threads — so the batch
/// runs several. Four takes 2.57× of the 3.67× on offer for ~2.7 GB of peak
/// memory at 30 Mpx; six would take 3.42× for 4 GB. The cap is deliberately
/// low because the cost of being wrong is paging, which loses far more than
/// the concurrency wins. `ExportRequest::concurrency` overrides it.
const DEFAULT_EXPORT_CONCURRENCY: usize = 4;

/// How many photos a batch keeps in flight when the request names no number:
/// `min(4, available_parallelism())` (ADR 0068 §1).
///
/// The core count matters because peak memory grows linearly with the degree
/// — ~700 MB per photo in flight at 30 Mpx, past a gigabyte at 45 Mpx
/// (`docs/system-requirements.md` §2). A two-core machine is also, typically,
/// the machine with 4 GB of RAM: giving it the full degree of four would buy
/// no speed its cores can use, and would buy it at the one price that makes a
/// batch slower rather than faster.
fn default_export_concurrency() -> usize {
    DEFAULT_EXPORT_CONCURRENCY.min(job_pool_size())
}

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
    /// Intermediate buffers of the preview pipeline (ADR 0041 §3). Lives
    /// here, beside the decode cache, rather than in an `EditSession`:
    /// Studio's develop view renders through `Library::preview`, so a
    /// session-held cache would never be hit by the very interaction it
    /// exists for. Purely derived — dropping it changes no pixel.
    stage_cache: Mutex<crate::stages::StageCache>,
    /// One sender per subscriber; pruned when a receiver is dropped.
    subscribers: Mutex<Vec<Sender<Event>>>,
    /// Next job id, unique within this process.
    next_job: AtomicU64,
    /// The running tether session (`docs/adr/0038`), if any. One camera at
    /// a time per library — connecting while this is `Some` is refused.
    #[cfg(feature = "tether")]
    tether: Mutex<Option<leyline_tether::TetherSession>>,
    /// What the running tether session was opened with (ADR 0087 §4): the
    /// folder its shots are filed under and the preset each one is
    /// developed with. Read by `handle_tether_event`, on the session's own
    /// thread, for every shot that arrives.
    #[cfg(feature = "tether")]
    tether_options: Mutex<crate::tether::TetherOptions>,
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

/// What a removal actually did (ADR 0060 §4).
///
/// The three fields answer three different questions, and a caller that
/// conflates them will mislead its user: what left the catalog, what left
/// the disk, and what refused to move.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemovalReport {
    /// Assets that existed and are no longer in the catalog. Ids that
    /// matched nothing are silently absent — asking to remove what is
    /// already gone is not an error.
    pub removed: Vec<AssetId>,
    /// Files sent to the system trash, sidecars included. Always empty for
    /// [`Library::remove_assets`], which touches no file.
    pub trashed: Vec<PathBuf>,
    /// Files that exist but could not be trashed, each with the reason.
    /// A locked or permission-denied file lands here; a file already gone
    /// does not.
    pub failed: Vec<(PathBuf, String)>,
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

/// A root, plus where it is on this machine right now (ADR 0085 §5).
///
/// `location` absent means **offline, not missing**: the photographs are
/// still catalogued, still browsable from the preview cache, still
/// searchable — what cannot happen is anything needing their pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootStatus {
    /// Identity and name, as the catalog holds them.
    pub root: Root,
    /// Where it verified, or `None` when it is offline.
    pub location: Option<PathBuf>,
}

impl RootStatus {
    /// Whether the root can be read right now.
    pub fn is_online(&self) -> bool {
        self.location.is_some()
    }
}

/// Writes the library's own `.leyline-root` marker if it is not already
/// there (ADR 0085 §3).
///
/// Root 1 carries the library's own UUID, so the marker is **derivable from
/// the catalog** and can be rewritten at any time: a library restored from a
/// backup that lost the dotfile, or migrated from before ADR 0085, gets it
/// back on the next open rather than becoming unidentifiable.
///
/// A read-only library is left alone. Root 1 is the one root that resolves
/// without a marker — it is the folder the caller already opened — so a
/// catalog that cannot be written to loses nothing by not having one.
fn ensure_library_marker(root: &Path, catalog: &Catalog) -> Result<()> {
    let Some(library) = catalog
        .roots()?
        .into_iter()
        .find(|r| r.id == leyline_catalog::LIBRARY_ROOT)
    else {
        return Ok(());
    };
    if crate::roots::verifies(root, &library.uuid) {
        return Ok(());
    }
    match crate::roots::write_marker(root, &library.uuid) {
        Ok(()) => Ok(()),
        // A library on read-only media still opens; it simply cannot be
        // referenced as an external root from elsewhere until it can be
        // written to.
        Err(LeylineError::Io(e)) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        Err(e) => Err(e),
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
        ensure_library_marker(root, &catalog)?;
        Ok(Library::assemble(root, catalog))
    }

    /// Opens an existing library, applying pending catalog migrations.
    pub fn open(root: &Path) -> Result<Library> {
        let catalog = Catalog::open(&root.join("catalog.db"))?;
        ensure_library_marker(root, &catalog)?;
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
                stage_cache: Mutex::new(crate::stages::StageCache::default()),
                subscribers: Mutex::new(Vec::new()),
                #[cfg(feature = "tether")]
                tether: Mutex::new(None),
                #[cfg(feature = "tether")]
                tether_options: Mutex::new(crate::tether::TetherOptions::default()),
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

    /// Every root this library references, and whether it is reachable now
    /// (ADR 0085 §8).
    pub fn roots(&self) -> Result<Vec<RootStatus>> {
        let catalog = lock(&self.inner.catalog);
        let hints = crate::roots::read_hints(&self.inner.root);
        Ok(catalog
            .roots()?
            .into_iter()
            .map(|root| {
                let location = if root.id == leyline_catalog::LIBRARY_ROOT {
                    Some(self.inner.root.clone())
                } else {
                    hints
                        .get(&root.uuid)
                        .filter(|path| crate::roots::verifies(path, &root.uuid))
                        .cloned()
                };
                RootStatus { root, location }
            })
            .collect())
    }

    /// Adds a folder as a root this library may reference photos inside
    /// (ADR 0085 §6).
    ///
    /// An **explicit gesture**, never implicit: an import whose source sits
    /// outside every known root keeps today's behaviour and is skipped, and
    /// nothing about browsing or importing creates a root behind the user's
    /// back.
    ///
    /// Writes the marker before the catalog row. If the process dies between
    /// the two, what is left on disk is a folder claiming an identity no
    /// catalog knows — inert, and overwritten by the next attempt — rather
    /// than a catalog row pointing at a folder that can never be verified.
    ///
    /// A folder that already carries a marker keeps its identity: adding the
    /// same archive to a second library must reference the same root, which
    /// is exactly what makes a root belong to the folder rather than to a
    /// library.
    pub fn add_root(&self, folder: &Path, name: &str) -> Result<Root> {
        let folder = folder.canonicalize()?;
        if !folder.is_dir() {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                format!("{} is not a folder", folder.display()),
            )));
        }

        let uuid = match crate::roots::read_marker(&folder) {
            Some(existing) => existing,
            None => {
                let fresh = uuid::Uuid::new_v4().to_string();
                crate::roots::write_marker(&folder, &fresh)?;
                fresh
            }
        };

        let mut catalog = lock(&self.inner.catalog);
        let root = match catalog.root_by_uuid(&uuid)? {
            Some(known) => known,
            None => catalog.insert_root(&uuid, name)?,
        };
        drop(catalog);

        crate::roots::remember(&self.inner.root, &uuid, &folder)?;
        Ok(root)
    }

    /// Tells the library where a root went (ADR 0085 §2, step 3).
    ///
    /// The hint is written only if the folder's marker agrees: a wrong
    /// answer is refused rather than remembered, because a remembered wrong
    /// answer resolves silently to somebody else's photographs.
    pub fn locate_root(&self, uuid: &str, folder: &Path) -> Result<()> {
        let folder = folder.canonicalize()?;
        if !crate::roots::verifies(&folder, uuid) {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{} is not that root: its marker says {}",
                    folder.display(),
                    crate::roots::read_marker(&folder).unwrap_or_else(|| "nothing".into())
                ),
            )));
        }
        crate::roots::remember(&self.inner.root, uuid, &folder)
    }

    /// Stops referencing a root (ADR 0085 §8).
    ///
    /// Refused while any folder still sits in it. The marker on disk is left
    /// alone: it belongs to the folder, and another library may be using it.
    pub fn forget_root(&self, uuid: &str) -> Result<()> {
        let mut catalog = lock(&self.inner.catalog);
        let root = catalog
            .root_by_uuid(uuid)?
            .ok_or_else(|| LeylineError::Db(format!("no root {uuid} in this library")))?;
        catalog.delete_root(root.id)?;
        drop(catalog);
        crate::roots::forget(&self.inner.root, uuid)
    }

    /// Where an asset's file is, right now, on this machine (ADR 0085 §4).
    ///
    /// **The one place a stored path becomes a real one.** Every caller that
    /// opens a photograph goes through here, and that is not tidiness: it is
    /// what lets the offline case of ADR 0085 §5 be handled once instead of
    /// at each of the dozen sites that used to join the library root
    /// themselves.
    ///
    /// `Cache/`, `Masks/`, `Profiles/`, `Exports/` and `Backups/` keep
    /// joining the library root directly — they are library-local by
    /// definition, and no root but the library's own ever holds them.
    ///
    /// Fails with [`LeylineError::RootOffline`] when the root holding the
    /// asset cannot be found, never with a file-not-found: an unplugged disk
    /// and a deleted photograph are different accidents and deserve
    /// different words.
    pub fn locate(&self, asset: AssetId) -> Result<PathBuf> {
        crate::roots::locate(&lock(&self.inner.catalog), &self.inner.root, asset)
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

    /// Removes assets from the catalog, leaving their files untouched
    /// (ADR 0060 §1). Emits `AssetsRemoved`.
    ///
    /// This is the operation that makes ADR 0043 §5 applicable again: with
    /// the asset gone its checksum is unknown, so the file re-imports
    /// instead of being skipped as a duplicate.
    pub fn remove_assets(&self, assets: &[AssetId]) -> Result<RemovalReport> {
        self.remove_inner(assets, false)
    }

    /// Removes assets from the catalog *and* sends their files to the
    /// system trash (ADR 0060 §2). Emits `AssetsRemoved`.
    ///
    /// The XMP sidecar goes with the file. Nothing outside the library root
    /// is ever touched: even a referenced import lives under it, since the
    /// catalog stores root-relative paths only (ADR 0010).
    ///
    /// A file already missing is not an error — the catalog must be able to
    /// clean up after a photo the user moved away behind Leyline's back.
    /// A file that exists and resists is reported in
    /// [`RemovalReport::failed`], never swallowed.
    pub fn delete_assets(&self, assets: &[AssetId]) -> Result<RemovalReport> {
        self.remove_inner(assets, true)
    }

    /// Shared body of [`Library::remove_assets`] and
    /// [`Library::delete_assets`] — the catalog side is identical, only the
    /// fate of the source files differs.
    fn remove_inner(&self, assets: &[AssetId], trash_files: bool) -> Result<RemovalReport> {
        // A companion goes with its master (ADR 0079 §6). The `ON DELETE
        // CASCADE` would drop its row either way — expanding the list first
        // is what makes its *file* reach the trash and its id appear in the
        // report, instead of a JPEG left on disk that nothing points to.
        let assets = self.catalog().with_companions(assets)?;
        let deleted = self.catalog_mut().delete_assets(&assets)?;
        if deleted.assets.is_empty() {
            return Ok(RemovalReport::default());
        }

        // Preview files are outside SQLite: the cascade dropped their rows
        // and would leave their bytes behind. A cache file that refuses to
        // go is not worth failing a removal over — the cache is derived and
        // can be cleared wholesale.
        for path in &deleted.preview_paths {
            let _ = self.inner.cache.remove(path);
        }

        let mut report = RemovalReport {
            removed: deleted.assets,
            trashed: Vec::new(),
            failed: Vec::new(),
        };

        if trash_files {
            for relative in &deleted.file_paths {
                let file = self.inner.root.join(relative);
                // Both naming conventions: leaving behind the sidecar of a
                // photo that no longer exists is how a later import of the
                // same folder resurrects metadata nothing points to.
                let sidecars = crate::xmp::sidecar_candidates(&file);
                for target in std::iter::once(file).chain(sidecars) {
                    if !target.exists() {
                        continue;
                    }
                    match trash::delete(&target) {
                        Ok(()) => report.trashed.push(target),
                        Err(error) => report.failed.push((target, error.to_string())),
                    }
                }
            }
        }

        self.emit(Event::AssetsRemoved {
            asset_ids: report.removed.clone(),
        });
        Ok(report)
    }

    /// Pairs every RAW+JPEG couple the library already holds, and reports
    /// each as `(master, companion)` (ADR 0079 §7).
    ///
    /// The v3 migration adds the column without pairing anything, so this is
    /// how a library imported before ADR 0079 stops listing every shot
    /// twice. Idempotent: a second run finds nothing left to pair.
    pub fn pair_assets(&self) -> Result<Vec<(AssetId, AssetId)>> {
        let paired = self.catalog_mut().pair_all()?;
        if !paired.is_empty() {
            // Both sides changed for a view: the companion left the grid,
            // and the master now carries one.
            let mut touched = Vec::with_capacity(paired.len() * 2);
            for (master, companion) in &paired {
                touched.push(*master);
                touched.push(*companion);
            }
            self.emit(Event::AssetsChanged { asset_ids: touched });
        }
        Ok(paired)
    }

    /// Detaches `assets`, master or companion, and returns how many rows
    /// stopped being companions (ADR 0079 §6).
    ///
    /// Nothing is lost or moved: a detached companion returns to the grid
    /// with the rating, keywords and revisions it always had.
    pub fn unpair_assets(&self, assets: &[AssetId]) -> Result<u32> {
        let detached = self.catalog_mut().unpair_assets(assets)?;
        if detached > 0 {
            self.emit(Event::AssetsChanged {
                asset_ids: assets.to_vec(),
            });
        }
        Ok(detached)
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

    /// Renames a collection (§9, `docs/catalog.md` §24).
    pub fn rename_collection(&self, id: CollectionId, name: &str) -> Result<()> {
        self.catalog_mut().rename_collection(id, name)
    }

    /// Moves a collection under `parent`, or to the root with `None` (§9).
    ///
    /// A move that would make the collection its own descendant is refused.
    pub fn move_collection(&self, id: CollectionId, parent: Option<CollectionId>) -> Result<()> {
        self.catalog_mut().move_collection(id, parent)
    }

    /// Deletes a collection and everything under it, returning how many
    /// collections went (§9).
    ///
    /// A grouping disappears; no version, no revision and no file is touched.
    pub fn delete_collection(&self, id: CollectionId) -> Result<u32> {
        self.catalog_mut().delete_collection(id)
    }

    /// The library's own record: name, uuid and dates (catalogue §7).
    ///
    /// What a client displays as the library's identity — the title bar, the
    /// About dialog, and the name of the root row in a folder tree, that row
    /// being the library folder itself (§8).
    pub fn info(&self) -> Result<LibraryInfo> {
        self.catalog().library()
    }

    /// Every folder of the library with its photo count, in display order
    /// (§8) — what a sidebar folder tree lists (ADR 0055 §2). Read-only:
    /// nothing here renames, moves or deletes a folder.
    ///
    /// A photograph sitting directly under the library root gives the tree a
    /// row whose `relative_path` is empty: the root itself.
    pub fn folders(&self) -> Result<Vec<FolderNode>> {
        self.catalog().folders()
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
    pub fn import_files(
        &self,
        source: &Path,
        files: &[PathBuf],
        options: &ImportOptions,
        progress: impl FnMut(u64, u64),
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
    pub fn scan_import(
        &self,
        source: &Path,
        options: &ScanOptions,
        progress: impl FnMut(u64, u64),
    ) -> Result<Vec<ImportCandidate>> {
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
            let scanned = library.scan_import(&source, &options, |done, total| {
                library.emit(Event::JobProgress {
                    job_id: job,
                    done,
                    total,
                });
            });
            let result = match scanned {
                Ok(candidates) => JobResult::Scan(candidates),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.emit(Event::JobFinished {
                job_id: job,
                result,
            });
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
    fn spawn_import_thumbnails(&self, imported: &[ImportedFile]) {
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
    fn spawn_import(
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
            let progress = |done, total| {
                library.emit(Event::JobProgress {
                    job_id: job,
                    done,
                    total,
                });
            };
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
            // ADR 0082 §1. Taken with no lock held at all: this decodes no
            // sensor and runs no stage, so it needs neither the decode cache
            // nor the stage cache — the two the render path holds, and the
            // two that made the import pass unparallelizable.
            crate::preview::PreviewPlan::FromFile {
                source_path,
                media_type,
                head,
                fallback,
            } => match crate::preview::file_thumbnail(&source_path, media_type) {
                Some(image) => {
                    let mut catalog = lock(&self.inner.catalog);
                    return crate::preview::record_embedded(
                        &mut catalog,
                        &self.inner.cache,
                        asset,
                        head,
                        &image,
                    );
                }
                // No embedded preview, unreadable, or smaller than the class
                // asked for: develop it after all.
                None => fallback,
            },
        };
        let image = {
            let mut decodes = lock(&self.inner.decodes);
            let mut stage_cache = lock(&self.inner.stage_cache);
            crate::preview::render_preview(&mut decodes, &mut stage_cache, asset, &plan)?
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
            crate::preview::render_with_settings(
                &mut decodes,
                None,
                asset,
                &plan,
                &Settings::default(),
            )?
        };
        Ok(match leyline_preview::max_edge(kind) {
            Some(edge) => image.scaled_to_fit(edge),
            None => image,
        })
    }

    /// Renders `settings` at `kind`, live: what the photo would look like if
    /// these values were committed (ADR 0074).
    ///
    /// The third *view* of the engine, next to [`Library::preview_before`]
    /// and the mask overlay, and it follows the same rule as both: **nothing
    /// is written**. No preview file, no cache row, no revision — so a value
    /// a user was merely trying out can never be mistaken later for what the
    /// photo is, and a drag across a slider costs no disk at all.
    ///
    /// Unlike those two it renders through the *stage cache* (ADR 0041 §3):
    /// this is the path that cache exists for, and it is what keeps moving a
    /// late-pipeline slider in the tens of milliseconds rather than replaying
    /// the whole pipeline per frame.
    ///
    /// The caller owns the throttling: this renders every time it is asked.
    pub fn preview_live(
        &self,
        asset: AssetId,
        kind: PreviewKind,
        settings: &Settings,
    ) -> Result<leyline_preview::Rgb8> {
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::preview::plan_settings_render(&catalog, &self.inner.root, asset, kind)?
        };
        let image = {
            let mut decodes = lock(&self.inner.decodes);
            let mut stage_cache = lock(&self.inner.stage_cache);
            crate::preview::render_with_settings(
                &mut decodes,
                Some((&mut stage_cache, asset)),
                asset,
                &plan,
                settings,
            )?
        };
        Ok(match leyline_preview::max_edge(kind) {
            Some(edge) => image.scaled_to_fit(edge),
            None => image,
        })
    }

    /// The tone Auto proposes for `asset` (ADR 0088 §1–§2).
    ///
    /// **Writes nothing.** It returns five numbers; feeding them through an
    /// `EditSession` is the caller's business, which is what makes an
    /// automatic tone one ordinary, undoable revision rather than a
    /// second way of developing a photo. No stage, no stage version,
    /// nothing new in `settings_json`: `docs/pipeline.md` §5.1 is untouched
    /// by construction.
    ///
    /// Measured on the photo at its **current** settings, not at neutral:
    /// pressing Auto after moving the white balance should answer for the
    /// photo as it now is. Costs a handful of proxy renders — the exposure
    /// is a logarithm, the recovery a pair of percentiles, and the two ends
    /// a short search against the real pipeline rather than a transfer
    /// function nobody wrote down.
    pub fn auto_tone(&self, asset: AssetId) -> Result<AutoTone> {
        let base = {
            let catalog = lock(&self.inner.catalog);
            let version = catalog.current_version(asset)?;
            let head = catalog.version_head(version)?;
            Settings::parse(&catalog.revision(head)?.settings_json)?
        };
        let mut tone = AutoTone {
            exposure: base.exposure,
            highlights: base.highlights,
            shadows: base.shadows,
            whites: base.whites,
            blacks: base.blacks,
        };

        // 1. Exposure, from the photo as it stands.
        let bins = self.tone_histogram(asset, &base)?;
        tone.exposure = crate::auto_tone::exposure_for(&bins, base.exposure);

        // 2. Recovery, measured *after* that correction: whether the
        //    highlights are crowded depends on where the exposure put them.
        let bins = self.tone_histogram(asset, &crate::auto_tone::with(&base, &tone))?;
        let (highlights, shadows) = crate::auto_tone::recovery_for(&bins);
        tone.highlights = highlights;
        tone.shadows = shadows;

        // 3. The two ends, searched. They are the two sliders whose effect
        //    is exactly "where does this end land", so they are measured
        //    against the real pipeline instead of guessed.
        for _ in 0..crate::auto_tone::SEARCH_STEPS {
            let bins = self.tone_histogram(asset, &crate::auto_tone::with(&base, &tone))?;
            let (white, black) = crate::auto_tone::ends_error(&bins);
            if white.abs() < crate::auto_tone::SEARCH_TOLERANCE
                && black.abs() < crate::auto_tone::SEARCH_TOLERANCE
            {
                break;
            }
            let stepped_whites = crate::auto_tone::stepped(tone.whites, white);
            let stepped_blacks = crate::auto_tone::stepped(tone.blacks, black);
            // Both ends already at the edge of their range: the photo asks
            // for more than a slider can give, and another render would
            // measure the same thing again.
            if stepped_whites == tone.whites && stepped_blacks == tone.blacks {
                break;
            }
            tone.whites = stepped_whites;
            tone.blacks = stepped_blacks;
        }
        Ok(tone)
    }

    /// The luma histogram of one proxy render of `asset` under `settings`.
    fn tone_histogram(&self, asset: AssetId, settings: &Settings) -> Result<[u64; 256]> {
        let image = self.preview_live(asset, PreviewKind::Small, settings)?;
        Ok(crate::auto_tone::histogram_of(&image))
    }

    /// Records what someone wrote about a photograph (ADR 0099).
    ///
    /// Passes through to the catalog undecorated, like the other authored
    /// state (classification, keywords): the facade adds orchestration only
    /// where several stores cooperate. Emits `AssetsChanged` so a client
    /// showing the description refreshes.
    pub fn set_description(
        &self,
        asset: AssetId,
        description: &leyline_catalog::AssetDescription,
    ) -> Result<()> {
        self.catalog_mut().set_description(asset, description)?;
        self.emit(Event::AssetsChanged {
            asset_ids: vec![asset],
        });
        Ok(())
    }

    /// Overlays one description onto many assets (ADR 0099 §4): a template
    /// typed once and stamped on a whole import.
    ///
    /// *Overlays*, never replaces: a template that sets only a copyright
    /// line leaves a title someone already wrote. Emits one
    /// `AssetsChanged` for the batch rather than one per asset — a client
    /// refreshing ten thousand times is a client that stops responding.
    pub fn describe_batch(
        &self,
        assets: &[AssetId],
        template: &leyline_catalog::AssetDescription,
    ) -> Result<()> {
        if assets.is_empty() || template.is_empty() {
            return Ok(());
        }
        {
            let mut catalog = self.catalog_mut();
            for &asset in assets {
                let existing = catalog.description(asset)?.unwrap_or_default();
                catalog.set_description(asset, &existing.overlaid_with(template))?;
            }
        }
        self.emit(Event::AssetsChanged {
            asset_ids: assets.to_vec(),
        });
        Ok(())
    }

    /// What one click reads for a range mask (ADR 0093): the display-axis
    /// luminance and the hue of the 5×5 mean around `(x, y)` — unit
    /// coordinates of the rendered image.
    ///
    /// A measurement, never a write: the client decides what band to make
    /// of it. Read from the same proxy render the white-balance picker
    /// reads, which is the image the user is pointing at; the frozen
    /// `local_adjustments` modules read their terms at rank 160, and the
    /// gap is accepted and stated (ADR 0093 §1) — the eyedropper proposes
    /// a starting point, the sliders own the truth.
    pub fn sample_range(&self, asset: AssetId, x: f64, y: f64) -> Result<RangeSample> {
        let base = {
            let catalog = lock(&self.inner.catalog);
            let version = catalog.current_version(asset)?;
            let head = catalog.version_head(version)?;
            Settings::parse(&catalog.revision(head)?.settings_json)?
        };
        let image = self.preview_live(asset, PreviewKind::Small, &base)?;
        let rgb = crate::wb::sample_mean_display(&image, x, y);
        let luminance = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        #[allow(clippy::cast_possible_truncation)]
        let (hue, _, _) =
            crate::stages::kernel::v1::rgb_to_hsl(&[rgb[0] as f32, rgb[1] as f32, rgb[2] as f32]);
        Ok(RangeSample {
            luminance,
            hue: f64::from(hue),
        })
    }

    /// The white balance that makes the 5×5 neighbourhood around `(x, y)`
    /// — unit coordinates of the rendered image — neutral (ADR 0091 §1).
    ///
    /// A proposal, exactly like [`Library::auto_tone`]: it writes nothing,
    /// the client feeds the answer through an ordinary session, and
    /// `docs/pipeline.md` §5.1 is untouched by construction. Solved against
    /// the real pipeline (ADR 0091 §2): render at the candidate, sample,
    /// divide out the candidate's own gains, re-solve, until neutral. A
    /// clipped or near-black sample is refused, never guessed.
    pub fn neutralize_wb(&self, asset: AssetId, x: f64, y: f64) -> Result<WhiteBalance> {
        self.solve_wb(asset, |image| crate::wb::sample_mean(image, x, y), true)
    }

    /// The white balance that makes the frame's mean color neutral —
    /// grey-world, clipped pixels excluded (ADR 0091 §3). The same solver
    /// as [`Library::neutralize_wb`], fed the whole frame.
    pub fn auto_wb(&self, asset: AssetId) -> Result<WhiteBalance> {
        self.solve_wb(asset, crate::wb::frame_mean, false)
    }

    /// The shared render/sample/solve loop of the two pickers. `strict`
    /// refuses a sample that cannot answer (the pointed picker); the
    /// whole-frame mean is always readable.
    fn solve_wb(
        &self,
        asset: AssetId,
        sample: impl Fn(&leyline_preview::Rgb8) -> [f64; 3],
        strict: bool,
    ) -> Result<WhiteBalance> {
        let base = {
            let catalog = lock(&self.inner.catalog);
            let version = catalog.current_version(asset)?;
            let head = catalog.version_head(version)?;
            Settings::parse(&catalog.revision(head)?.settings_json)?
        };
        let mut wb = base.white_balance.clone().unwrap_or_default();
        for step in 0..crate::wb::SEARCH_STEPS {
            let mut settings = base.clone();
            settings.white_balance = Some(wb.clone());
            let image = self.preview_live(asset, PreviewKind::Small, &settings)?;
            let sampled = sample(&image);
            if step == 0 && strict {
                if let Some(reason) = crate::wb::refuse(sampled) {
                    return Err(LeylineError::InvalidImage(reason.to_owned()));
                }
            }
            if crate::wb::neutral_enough(sampled) {
                break;
            }
            let gains = crate::wb::wb_gains(&wb);
            wb = crate::wb::solve([
                sampled[0] / gains[0],
                sampled[1] / gains[1],
                sampled[2] / gains[2],
            ]);
        }
        Ok(wb)
    }

    /// Renders `asset` as it would look **with** `preset` applied, without
    /// applying it (ADR 0058 §4): the trial a client shows while a preset is
    /// merely hovered.
    ///
    /// A *view*, like [`Library::preview_before`] and the soft proof: no
    /// revision, no preset provenance, no cache entry — nothing that could
    /// later be mistaken for a development the photographer asked for.
    /// Leaving the hover shows the photo as it really is because nothing
    /// ever changed.
    ///
    /// It goes through the same `param_values` mapping the real application
    /// uses (`presets::overlay`), so what is shown is what would be written.
    pub fn preset_preview(
        &self,
        asset: AssetId,
        kind: PreviewKind,
        preset: PresetId,
    ) -> Result<leyline_preview::Rgb8> {
        let settings = {
            let catalog = lock(&self.inner.catalog);
            let stored = catalog.preset(preset)?;
            let fields = PresetSettings::parse(&stored.preset_json)?;
            let version = catalog.current_version(asset)?;
            let head = catalog.version_head(version)?;
            let base = Settings::parse(&catalog.revision(head)?.settings_json)?;
            crate::presets::overlay(&base, &fields)?
        };
        self.preview_live(asset, kind, &settings)
    }

    /// Renders the coverage of one of the head revision's local adjustments,
    /// scaled like `kind` — the mask overlay of ADR 0071.
    ///
    /// Grey, never coloured: 0 where the adjustment does not apply, 255 where
    /// it applies fully, already through the revision's rotation, perspective
    /// and crop so it lands on the preview it will be drawn over. The tint is
    /// the interface's decision, not the engine's.
    ///
    /// A *view*, like [`Library::preview_before`] and the soft proof: nothing
    /// is cached, nothing is recorded, and no stage version exists for it —
    /// `docs/pipeline.md` §5.1 is not in play.
    pub fn mask_coverage_preview(
        &self,
        asset: AssetId,
        kind: PreviewKind,
        index: usize,
    ) -> Result<leyline_preview::Rgb8> {
        let (plan, settings) = {
            let catalog = lock(&self.inner.catalog);
            let plan =
                crate::preview::plan_settings_render(&catalog, &self.inner.root, asset, kind)?;
            let version = catalog.current_version(asset)?;
            let head = catalog.version_head(version)?;
            let settings = Settings::parse(&catalog.revision(head)?.settings_json)?;
            (plan, settings)
        };
        let image = {
            let mut decodes = lock(&self.inner.decodes);
            crate::preview::render_mask_coverage(&mut decodes, asset, &plan, &settings, index)?
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

    /// Returns what the cache can show for the asset's current version,
    /// without ever rendering (§11). Lets a client fill what is already on
    /// disk instantly and schedule the rest.
    ///
    /// "What it can show" and not "the head's render": the thumbnail of a
    /// photo nobody has developed is the picture the file carries, and it is
    /// the finished answer for that state, not a placeholder (ADR 0082 §1).
    /// A client therefore needs no notion of provenance — a cell either has
    /// an image or does not, exactly as before.
    pub fn cached_preview(&self, asset: AssetId, kind: PreviewKind) -> Result<Option<PreviewFile>> {
        Ok(self
            .catalog()
            .displayable_preview(asset, kind)?
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
        progress: impl FnMut(u64, u64),
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
    fn plan_batch(
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
    fn run_export_batch(
        &self,
        planned: Vec<(VersionId, PlannedExport)>,
        settings: &ExportSettings,
        preset: Option<ExportPresetId>,
        in_flight: usize,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<ExportReport> {
        let total = planned.len() as u64;
        // One slot per version, filled in place, so the report keeps request
        // order however the renders finish.
        let outcomes: Vec<std::sync::Mutex<Option<std::result::Result<PathBuf, String>>>> = (0
            ..planned.len())
            .map(|_| std::sync::Mutex::new(None))
            .collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();

        std::thread::scope(|scope| {
            for _ in 0..in_flight {
                let (next, outcomes, planned) = (&next, &outcomes, &planned);
                let done_tx = done_tx.clone();
                scope.spawn(move || {
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((_, planned)) = planned.get(index) else {
                            return;
                        };
                        let outcome = match planned {
                            Ok((plan, destination)) => {
                                crate::export::render_export_to(plan, settings, destination)
                                    .and_then(|()| {
                                        let mut catalog = lock(&self.inner.catalog);
                                        crate::export::journal_export(
                                            &mut catalog,
                                            plan,
                                            preset,
                                            settings,
                                            destination,
                                        )?;
                                        Ok(destination.clone())
                                    })
                                    .map_err(|error| error.to_string())
                            }
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
                progress(done, total);
            }
        });

        let mut report = ExportReport::default();
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
                None => unreachable!("every slot is filled before the scope ends"),
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
    pub fn apply_preset(
        &self,
        preset: PresetId,
        versions: &[VersionId],
        mut progress: impl FnMut(u64, u64),
    ) -> Result<PresetApplyReport> {
        let stored = self.catalog().preset(preset)?;
        let fields = PresetSettings::parse(&stored.preset_json)?;
        let mut catalog = lock(&self.inner.catalog);
        // Each revision records the preset *and the version of it* that was
        // applied (ADR 0058 §5), which is what makes "developed with an older
        // version of this preset" answerable later.
        let report = crate::presets::apply_batch_from(
            &mut catalog,
            &fields,
            Some((preset, stored.revision)),
            versions,
            &mut progress,
        );
        drop(catalog);
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
    fn handle_tether_event(&self, event: leyline_tether::TetherEvent) {
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
    fn import_tethered_shot(&self, staged: &Path) {
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
    fn handle_watch_event(&self, event: crate::watch::WatchSessionEvent) {
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

    /// Stores a mask coverage in the library and returns the
    /// [`leyline_core::Mask`] that references it (ADR 0070 §5).
    ///
    /// `coverage` is `width * height` samples in row-major order, `0` = the
    /// adjustment does not apply here, `u16::MAX` = it applies fully. The
    /// resolution is the producer's own: it is sampled over the normalized
    /// canvas, so it need not match the photo, and storing a segmentation
    /// model's native output beats upsampling it to the sensor's size.
    ///
    /// The only way to create a stored mask, and the surface a closed
    /// extension uses through the SDK ([ADR 0069](../../../docs/adr/0069-closed-extension-boundary.md)).
    /// Content-addressed, so storing the same coverage twice writes one
    /// file; the caller never learns the layout.
    pub fn store_mask_coverage(
        &self,
        width: u32,
        height: u32,
        coverage: &[u16],
    ) -> Result<leyline_core::Mask> {
        if self.catalog().is_read_only() {
            return Err(LeylineError::Db(
                "library opened read-only; writes are refused".to_owned(),
            ));
        }
        crate::mask_coverage::store_coverage(&self.inner.root, width, height, coverage)
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

    /// Opens (or returns the cached handle to) the active map pack: the one
    /// the user imported if there is one, the embedded world basemap
    /// otherwise (ADR 0059), and `None` when this build embeds none.
    ///
    /// An imported pack always wins. The two are never composed into one
    /// view — Leyline serves one pack, it does not blend a fine regional
    /// pack over a coarse world one.
    fn with_map_pack<T>(&self, f: impl FnOnce(&leyline_map::TilePack) -> T) -> Result<Option<T>> {
        let mut slot = lock(&self.inner.map_pack);
        if slot.is_none() {
            let path = self.map_pack_path();
            let pack = if path.is_file() {
                leyline_map::TilePack::open(&path).map_err(map_err)?
            } else {
                #[cfg(feature = "bundled-basemap")]
                {
                    leyline_map::TilePack::from_static(Self::BUNDLED_BASEMAP).map_err(map_err)?
                }
                #[cfg(not(feature = "bundled-basemap"))]
                {
                    return Ok(None);
                }
            };
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

    /// The default export degree is bounded by the machine, not only by the
    /// constant (ADR 0068 §1). Peak memory grows linearly with the degree,
    /// so a host with fewer cores than the ceiling must get fewer photos in
    /// flight — the constant alone would hand a two-core, 4 GB machine four
    /// of them, which is the one way to make a batch slower.
    #[test]
    fn default_export_concurrency_never_exceeds_the_machine() {
        let degree = default_export_concurrency();

        assert!(degree >= 1, "a batch must always run at least one photo");
        assert!(
            degree <= DEFAULT_EXPORT_CONCURRENCY,
            "the constant is the ceiling: {degree} > {DEFAULT_EXPORT_CONCURRENCY}"
        );
        assert!(
            degree <= job_pool_size(),
            "the machine is the other ceiling: {degree} > {}",
            job_pool_size()
        );
    }
}
