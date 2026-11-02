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

use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use leyline_catalog::{
    Catalog, CollectionNode, ContactSheetPreset, ExportPreset, FolderNode, KeywordNode,
    LibraryInfo, Preset, PresetFolder, PrintPreset, Root, SmartRules,
};
use leyline_core::{
    AssetId, CameraSettings, CollectionId, ColorLabel, ContactSheetPresetId, ExportPresetId, JobId,
    KeywordId, LeylineError, PickState, PresetFolderId, PresetId, PresetSettings, PreviewKind,
    PrintPresetId, Result, RevisionOrigin, Settings, SettingsGroup, TetherSetting, VersionId,
    WhiteBalance,
};
use leyline_export::{ContactSheetSettings, ExportSettings, PrintSettings};
use leyline_preview::{PreviewCache, Rgb8};

use crate::auto_tca::TcaEstimate;
use crate::auto_tone::AutoTone;
use crate::contact_sheet::{ContactSheetRecipe, ContactSheetReport, ContactSheetRequest};
use crate::cull::{CullOptions, CullProposal};
use crate::decode_cache::DecodeCache;
use crate::events::{Event, JobResult};
use crate::export::{ExportReport, ExportRequest};
use crate::flow::Flow;
use crate::import::{ImportOptions, ImportReport, ImportedFile};
use crate::keystone::{GuideLine, KeystoneSolution};
use crate::presets::PresetApplyReport;
use crate::preview::{Preview, PreviewFile};
use crate::print::{PrintRecipe, PrintReport, PrintRequest};
use crate::rename::{RenameReport, move_sidecars};
use crate::reprocess::ReprocessReport;
use crate::scan::{ScanOptions, ScanReport};
use crate::session::{CatalogAccess, EditSession};
use crate::wb::RangeSample;

// The methods of `Library` by family, each module adding its own
// `impl Library` block; this file keeps the struct, opening and closing,
// roots, jobs and events.
mod assist;
mod capture;
mod classify;
mod develop;
mod import;
mod map;
mod output;
mod preview;

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
    /// Versions an edit session currently claims (ADR 0120 §1). A session
    /// no longer holds the catalog's lock: it registers here, and takes the
    /// lock one operation at a time like every other method of the façade.
    claims: Mutex<HashSet<VersionId>>,
    /// Jobs a client has asked to stop (ADR 0139). A job id enters this set
    /// when [`Library::cancel_job`] is called and leaves it when the job
    /// ends, so the set holds at most the handful of batches in flight.
    cancelled_jobs: Mutex<HashSet<JobId>>,
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
        Library::assemble(root, catalog)
    }

    /// Opens an existing library, applying pending catalog migrations.
    pub fn open(root: &Path) -> Result<Library> {
        let catalog = Catalog::open(&root.join("catalog.db"))?;
        ensure_library_marker(root, &catalog)?;
        Library::assemble(root, catalog)
    }

    /// Opens an existing library without write access — the §5 fallback for
    /// catalogs newer than this engine.
    pub fn open_read_only(root: &Path) -> Result<Library> {
        let catalog = Catalog::open_read_only(&root.join("catalog.db"))?;
        Library::assemble(root, catalog)
    }

    /// Fails only when the OS refuses a worker thread (out of threads or
    /// memory): the workers already started stop once `job_tx` drops.
    fn assemble(root: &Path, catalog: Catalog) -> Result<Library> {
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
                })?;
        }
        Ok(Library {
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
                claims: Mutex::new(HashSet::new()),
                cancelled_jobs: Mutex::new(HashSet::new()),
                jobs: job_tx,
            }),
        })
    }

    /// Submits a `*_async` job's closure to the bounded job pool (§3.3).
    #[allow(
        clippy::expect_used,
        reason = "the pool's receiver lives until every Library handle, this one included, has dropped"
    )]
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

    /// Declares which program holds this library (ADR 0154 §6): every
    /// revision written through it from now on — edits, imports, presets —
    /// records `origin`. It belongs to the handle rather than to each write,
    /// because a per-write parameter can be forgotten and a forgotten origin
    /// is a lie by omission. A client declares it right after opening; an
    /// MCP server, once the client has named itself at `initialize`. A
    /// handle that never declares writes revisions of unknown origin.
    pub fn declare_origin(&self, origin: RevisionOrigin) {
        lock(&self.inner.catalog).declare_origin(origin);
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

    /// Asks a running job to stop at its next checkpoint (ADR 0139).
    ///
    /// Takes effect **between two units of work**, never inside one: the
    /// photograph being imported finishes being imported, the file being
    /// written finishes being written, and the batch then ends with the
    /// ordinary `JobFinished` whose report carries `cancelled: true` and
    /// names everything that was done. Nothing is undone.
    ///
    /// Cancelling a job that has already ended, or one that never existed,
    /// does nothing — a client racing a finishing batch is the normal case,
    /// not an error. Not every job listens: a contact sheet and a culling
    /// proposal are single answers rather than batches of work kept, and
    /// [`docs/adr/0139`] records why they are left out.
    pub fn cancel_job(&self, job: JobId) {
        lock(&self.inner.cancelled_jobs).insert(job);
    }

    /// The progress callback a cancellable `*_async` wrapper hands to its
    /// synchronous core: it reports, then answers whether to carry on.
    fn job_progress(&self, job: JobId) -> impl FnMut(u64, u64) -> Flow + use<> {
        let library = self.clone();
        move |done, total| {
            library.emit(Event::JobProgress {
                job_id: job,
                done,
                total,
            });
            if lock(&library.inner.cancelled_jobs).contains(&job) {
                Flow::Stop
            } else {
                Flow::Continue
            }
        }
    }

    /// Ends a job: forgets any cancellation asked of it, then announces the
    /// outcome. The order matters — a client that hears `JobFinished` and
    /// immediately starts another job must not inherit this one's flag.
    fn finish_job(&self, job: JobId, result: JobResult) {
        lock(&self.inner.cancelled_jobs).remove(&job);
        self.emit(Event::JobFinished {
            job_id: job,
            result,
        });
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
/// What a batch reports for a version an open session is holding
/// (ADR 0120 §2). One sentence, in the family of the engine's other
/// per-version reasons: the client shows it as-is.
const BUSY_REASON: &str = "another edit session is open on this version";

/// The per-version failure a claimed version produces in a preset batch.
fn busy_apply(version: VersionId) -> crate::presets::FailedApply {
    crate::presets::FailedApply {
        version,
        reason: BUSY_REASON.to_owned(),
    }
}

/// A session's claim on one version (ADR 0120 §1).
///
/// Two things at once, which is why it is one type: the **registration** that
/// refuses a second session on the same version, and the **handle** through
/// which that session reaches the catalog — taking the lock for one operation
/// and giving it back, the rule every other method of the façade follows.
///
/// Released by `Drop`, which runs after `EditSession`'s own `Drop` has
/// committed whatever was pending: the work is saved first, the claim goes
/// second.
#[derive(Debug)]
pub struct VersionClaim {
    library: Library,
    version: VersionId,
}

impl VersionClaim {
    /// Registers a claim, or refuses because one is already held.
    fn take(library: &Library, version: VersionId) -> Result<VersionClaim> {
        if !lock(&library.inner.claims).insert(version) {
            return Err(LeylineError::VersionBusy {
                version: version.get(),
            });
        }
        Ok(VersionClaim {
            library: library.clone(),
            version,
        })
    }
}

impl CatalogAccess for VersionClaim {
    fn with<R>(&mut self, operation: impl FnOnce(&mut Catalog) -> R) -> R {
        operation(&mut lock(&self.library.inner.catalog))
    }
}

impl Drop for VersionClaim {
    fn drop(&mut self) {
        lock(&self.library.inner.claims).remove(&self.version);
    }
}

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
