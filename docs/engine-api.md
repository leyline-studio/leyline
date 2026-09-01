# Engine API Specification

**Document:** `docs/engine-api.md`
**Version:** 1.0
**Status:** Draft

---

# 1. Purpose

To define the contract between the Leyline engine and its clients.

The graphical interface is only one client among others:

```text
Leyline Studio ─┐
Leyline CLI    ─┼──→ leyline-sdk ──→ leyline-engine ──→ leyline-core
Scripts / apps ─┘
```

The project's founding principle: **API before graphical interface**.

Everything Studio can do, the CLI and the SDK can do — because they call exactly the same API.

---

# 2. Principles

* The API is a **Rust library** (`leyline-sdk`), not a server and not a network protocol.
* No heavy operation blocks the caller: fast reads are synchronous, heavy work is asynchronous.
* The engine owns its threads; the client owns its event loop.
* No async runtime is imposed (no `tokio` dependency): native threads plus channels.
* Every error is a value (`Result`), never a panic across the API boundary.
* Identifiers are dedicated types, never bare integers.

---

# 3. Execution model

## 3.1 Two categories of call

| Category | Examples | Behaviour |
|---|---|---|
| **Queries** | grid, an asset's details, current settings, collections | Synchronous — SQLite answers in microseconds, a thread round trip would cost more |
| **Jobs** | import, preview rendering, export, reprocessing | Asynchronous — return a `JobId` immediately, progress through events |

## 3.2 Events

The client subscribes to a stream of events:

```rust
pub enum Event {
    AssetsAdded { asset_ids: Vec<AssetId> },
    AssetsChanged { asset_ids: Vec<AssetId> },
    AssetsRemoved { asset_ids: Vec<AssetId> },
    VersionChanged { version_id: VersionId },
    PreviewReady { asset_id: AssetId, kind: PreviewKind },
    JobProgress { job_id: JobId, done: u64, total: u64 },
    JobFinished { job_id: JobId, result: JobResult },
    LibraryClosed,
    TetherConnected,
    TetherDisconnected { reason: Option<String> },
    TetherSettingsChanged,
    TetherLiveFrame,
    TetherCommandFailed { message: String },
    WatchStarted { folder: PathBuf },
    WatchStopped { reason: Option<String> },
}

pub enum JobResult {
    Import(ImportReport),   // per-file failures included in the report
    Export(ExportReport),   // per-version failures included in the report
    Preset(PresetApplyReport), // per-version failures included in the report
    Preview(PreviewFile),
    Failed(String),         // the job failed before producing anything at all
}
```

* Subscribing returns a `Receiver<Event>` (a standard channel): `library.subscribe()`.
* Studio wires that channel into the Slint loop; the CLI reads it in sequence; a script may ignore it. A dropped receiver unsubscribes silently.
* Events are **notifications**, never complete data: the client re-queries what it needs. That avoids any coherence problem between the stream and the database.
* `PreviewReady` carries the asset (not the version): the preview surface is asset-based (§11), and the rendered preview is always that of the asset's current version.
* `TetherConnected`/`TetherDisconnected` bound the life cycle of a `tether_connect`/`tether_disconnect` session (§6bis) — each photo captured during the session notifies through `AssetsAdded`, exactly like an import: it is not a distinct event, only a different source for the same import.
* `TetherSettingsChanged`/`TetherLiveFrame` say that the session's own state moved: the camera's settings, or the live view's newest frame (ADR 0087 §2). Notifications like every other event — the values are read back through `tether_settings()`/`tether_live_frame()`, never carried here. A client that misses ten `TetherLiveFrame` and then reads the frame once is right, which is what a viewfinder wants.
* `TetherCommandFailed` reports a command the body refused — a shutter speed its current mode does not allow — **without** ending the session. It is not a disconnect: the next frame still has to be firable.
* `WatchStarted`/`WatchStopped` follow the same principle for `watch_start`/`watch_stop` (§6ter, `docs/adr/0039-watched-folder-import.md`): each file that stabilises in the watched folder notifies through `AssetsAdded`.
* **Delivered state**: `subscribe` and the `import_async`, `preview_async`, `export_async` jobs emit `JobProgress`, `AssetsAdded`, `PreviewReady` and `JobFinished`. The façade's writes notify: classification (§8) → one `VersionChanged` per version in the batch; keywords (§8) → `AssetsChanged` with the batch; removing assets (ADR 0060, `remove_assets`/`delete_assets`) → `AssetsRemoved` with the ones that actually existed; every history write of an edit session (§10.1 — commit, amendment, undo, redo) → `VersionChanged`. Applying a preset (§10.3) notifies nothing more: it is one session commit per targeted version, hence the same `VersionChanged` as §10.1, carried by the `apply_preset_async` job. A client that writes through `catalog_mut()` directly bypasses the notifications: go through the façade. `close()` (§5) emits `LibraryClosed` to every subscriber of the shared stream; the other clones of the `Library` stay usable — only the catalog connection closes, and only when the last clone is dropped.

## 3.3 Threading

* `Library` is `Send + Sync` and clones at no cost (an internal `Arc`). Every clone shares the same catalog and the same event stream.
* Catalog accesses go through guards (`catalog()` / `catalog_mut()`) that hold the internal lock: one operation, then release — never hold a guard across another call into the `Library` (an edit session holds the guard for its lifetime, and that is deliberate: nothing else mutates during editing).
* Every `*_async` job (`import_async`, `preview_async`, `export_async`, `export_with_preset_async`, `apply_preset_async`, `reprocess_async`) runs on a shared, bounded job pool (a fixed set of dedicated threads, sized to `available_parallelism()` capped at 16, one pool per `Library`) rather than on a dedicated thread per call: job concurrency has a ceiling imposed by the engine, whatever the client's behaviour — beyond the ceiling, further jobs queue. That pool is deliberately distinct from the global rayon pool used for pixel rendering (§10, `pixels.rs`, `process1..5.rs`): sharing one rayon pool between job dispatch and the `par_iter`/`join` work internal to rendering would expose work stealing to recruiting a job's thread to run *another* job while it still holds the catalog mutex — a non-reentrant lock, hence a deadlock. The job pool belongs to the same internal `Arc` as the catalog: its threads run as long as a clone of the `Library` exists and stop by themselves when the last one disappears; jobs in progress or queued are neither cancelled nor interrupted by `close()`.
* Catalog writes are serialised internally; the client has no ordering constraint to respect.

---

# 4. Fundamental types

```rust
pub struct AssetId(i64);
pub struct VersionId(i64);
pub struct RevisionId(i64);
pub struct CollectionId(i64);
pub struct KeywordId(i64);
pub struct PresetId(i64);
pub struct JobId(u64);

pub enum LeylineError {
    LibraryNotFound(PathBuf),
    LibraryLocked,
    NewerCatalog { found: u32, supported: u32 },
    AssetMissing(AssetId),
    VersionMissing(VersionId),
    RevisionMissing(RevisionId),
    KeywordMissing(KeywordId),
    CollectionMissing(CollectionId),
    PresetMissing(PresetId),
    DecodeFailed { asset: AssetId, reason: String },
    InvalidSettings(String),
    NewerSettings { schema: u32 },
    UnknownStage { stage: String, version: u16 },
    MixedWorkingSpaces {
        stage: String, version: u16, space: String,
        other_stage: String, other_version: u16, other_space: String,
    },
    InvalidImage(String),
    Io(std::io::Error),
    Db(String),
}

pub type Result<T> = std::result::Result<T, LeylineError>;
```

`NewerCatalog` and `NewerSettings` embody the forward-compatibility rule (`pipeline.md` §3.4): an old engine opens read-only or refuses — at the catalog level as at the revision level — but never modifies. Faced with `NewerSettings`, the client shows the best cached preview with a warning.

---

# 5. Library

```rust
impl Library {
    /// Creates a new library (folder + catalog.db).
    pub fn create(root: &Path, name: &str) -> Result<Library>;

    /// Opens an existing library. Applies migrations if needed.
    pub fn open(root: &Path) -> Result<Library>;

    /// Opens without write rights (catalog newer than the engine).
    pub fn open_read_only(root: &Path) -> Result<Library>;

    pub fn subscribe(&self) -> Receiver<Event>;

    pub fn close(self) -> Result<()>;
}
```

One writing instance per library (a file lock); several readers are free (WAL).

**Studio, launched with no argument (ADR 0022).** `leyline-studio` takes an optional library path as its argument (`leyline-studio <library-dir>`), exactly as a CLI client would call `Library::open`. Launched with no argument — which is the normal case from a graphical shortcut (the Windows installer's Start menu, the Linux AppImage, a double-click on the macOS `.app`, none of which attaches a console) — Studio no longer raises a usage error: it opens or creates, through `Library::create`/`Library::open` depending on whether a `catalog.db` already exists, a default library under `<the user's Documents>/Leyline Library` (falling back to `<home>/Leyline Library` if the system has no Documents folder). That location stays visible in the app (Help ▸ About Leyline). An explicit argument keeps the historical behaviour identically: `Library::open` alone, hence a clean error if the given path does not exist.

---

# 6. Import

```rust
pub struct ImportOptions {
    pub copy_files: bool,      // copy into Photos/ or reference in place (ADR 0010)
    pub recursive: bool,
    pub pair_companions: bool, // attach the camera JPEG to the RAW (ADR 0079)
    pub thumbnails: bool,      // warm the thumbnail cache afterwards (ADR 0082)
}
```

**`copy_files: false` does not mean "photos anywhere".** It skips the copy into `Photos/`; the file must already sit under the library root, the catalog storing nothing but root-relative paths ([ADR 0010](adr/0010-relative-paths.md)). A file located elsewhere is not imported — it joins `ImportReport::skipped` with the reason `file is outside the library root`. Cataloguing a collection where it already lives therefore means creating the library **above** it.

```rust
/// What a scan looks at (ADR 0065 §1).
pub struct ScanOptions {
    pub recursive: bool,
    pub thumbnails: bool,      // extract each file's embedded thumbnail
}

/// A file an import would take, described without being taken.
pub struct ImportCandidate {
    pub path: PathBuf,
    pub filename: String,
    pub media_type: MediaType,
    pub file_size: u64,
    pub capture_date: Option<i64>,
    pub camera: Option<String>,
    pub already_imported: bool,        // a name+size hint, not a verdict
    pub thumbnail: Option<Vec<u8>>,    // JPEG, edge ≤ 256 px, oriented
}

impl Library {
    /// The synchronous core: holds the catalog for the whole batch.
    pub fn import(&self, source: &Path, options: &ImportOptions,
                  progress: impl FnMut(u64, u64)) -> Result<ImportReport>;
    /// The job: returns immediately, progresses through `JobProgress`,
    /// announces `AssetsAdded` then `JobFinished` with the report.
    pub fn import_async(&self, source: &Path, options: &ImportOptions) -> JobId;

    /// Selective import (ADR 0065 §4): exactly those files, which must
    /// live under `source`. Same pipeline, same report, same events;
    /// a file outside `source` is set aside, not filed at random.
    pub fn import_files(&self, source: &Path, files: &[PathBuf],
                        options: &ImportOptions,
                        progress: impl FnMut(u64, u64)) -> Result<ImportReport>;
    pub fn import_files_async(&self, source: &Path, files: &[PathBuf],
                              options: &ImportOptions) -> JobId;

    /// RAW+JPEG pairing (ADR 0079). An import pairs as it goes;
    /// these two are for what was imported earlier, and for undoing.
    /// The catalog's v3 migration adds the column **without pairing**,
    /// so an existing library is never reorganised on its own (§7).
    /// `pair_assets` is idempotent and returns every pair made,
    /// `(master, companion)`; `unpair_assets` accepts a master as well as
    /// a companion and returns the number of photos back in the grid.
    /// Both notify through `AssetsChanged`.
    pub fn pair_assets(&self) -> Result<Vec<(AssetId, AssetId)>>;
    pub fn unpair_assets(&self, assets: &[AssetId]) -> Result<u32>;

    /// Files a mask coverage into the library and returns the
    /// `Mask::Coverage` that references it (ADR 0070 §5) — the single
    /// entry point, and the one a closed extension calls through the SDK
    /// (ADR 0069 §2). `coverage` holds `width * height` samples,
    /// 0 = the setting does not apply here, `u16::MAX` = it applies fully;
    /// the resolution is the producer's, it does not have to follow the
    /// photo's. Content-addressed: the same coverage twice makes one file.
    pub fn store_mask_coverage(&self, width: u32, height: u32,
                               coverage: &[u16]) -> Result<Mask>;

    /// What an import would take, **without writing anything** (ADR 0065 §1).
    pub fn scan_import(&self, source: &Path, options: &ScanOptions,
                       progress: impl FnMut(u64, u64)) -> Result<Vec<ImportCandidate>>;
    /// The corresponding job: `JobProgress` per file, then
    /// `JobFinished` with `JobResult::Scan`. Nothing being written, nothing
    /// else is announced — neither `AssetsAdded` nor `AssetsChanged`.
    pub fn scan_import_async(&self, source: &Path, options: &ScanOptions) -> JobId;
}
```

**Look before taking** (ADR 0065). A scan enumerates exactly what the import
would keep — same traversal, same extension filter, same ordering, the
enumeration code being shared — and reads nothing but headers. A candidate's
thumbnail is the one the camera wrote into the file (`leyline_raw::thumbnail`),
reduced and reoriented: never a render from the pipeline, since a candidate has
no revision to render. The `already_imported` marking compares name and size
(catalog §43); the import's BLAKE3 checksum remains the only exact answer,
and it is what refuses.

Import is a job: EXIF extraction, BLAKE3 checksum, creation of the initial revision and of the `Default` version (catalog §18) — streamed, with `JobProgress` per candidate file.

Filling the catalog and filling the thumbnail cache are **two jobs**, and only the first is the import ([ADR 0082](adr/0082-embedded-preview-at-import.md) §4). `import`/`import_async` return as soon as the assets exist; when `options.thumbnails` is set, a warming pass starts behind them and emits one `PreviewReady` per thumbnail, like any other render. A client that never waits for it still has a usable grid — §11 is what guarantees that, not the pass.

The pass runs **in the order the grid will show them** (newest capture first), which is also what drops companions: no grid draws one (ADR 0079 §5), so no pass should warm one. It runs **in parallel**: a thumbnail taken from the file's own picture decodes no sensor and runs no stage, so it holds neither the decode cache nor the stage cache — the two mutexes that used to serialise it. Photos that fall back to a real render still serialise there, correctly.

`Library::warm_thumbnails(&[AssetId])` is that pass, exposed and synchronous, for a caller that has no event loop and a process that exits — the command-line tool runs it directly rather than spawning a job that would be killed first.

A thumbnail that cannot be produced never cancels the asset's import: it stays imported with no cached thumbnail, and falls back on the lazy path (`cached_preview` then `preview_async`) the first time it must be displayed.

An **XMP sidecar** placed next to the source file seeds the asset just created — rating, label, hierarchical keywords, artist, copyright ([ADR 0047](adr/0047-xmp-sidecar-read.md), catalog §29): this is the migration path from other software, and it asks for no option. Like the thumbnail, it is best-effort: an unreadable sidecar never sets the photo aside, it sets itself aside. For an already imported asset, `Library::read_xmp(asset) -> Result<bool>` does the same on demand, filling without ever overwriting.

---

# 6bis. Tethered capture (`docs/adr/0038-tethered-capture.md`, `docs/adr/0087-tethered-capture-bar.md`)

```rust
pub struct TetherOptions {
    /// Folder under `Photos/` the session files its shots in.
    /// Blank means `DEFAULT_SESSION` ("Tethered").
    pub session: String,
    /// Develop preset applied to every shot as it arrives.
    pub preset: Option<PresetId>,
}

/// Validates a session name and returns the folder name to use —
/// what a client calls as the photographer types it.
pub fn session_folder(name: &str) -> Result<String>;

impl Library {
    /// Connects to the first USB camera detected (libgphoto2) and
    /// starts a session: every photo taken from then on is
    /// downloaded and imported automatically, like an ordinary import.
    /// Refuses a second session while one is already open.
    pub fn tether_connect(&self, options: &TetherOptions) -> Result<()>;

    /// Ends the current session; does nothing if none is open.
    pub fn tether_disconnect(&self);

    /// What the body last reported: model, capabilities, and the four
    /// exposure settings with the values it will accept. A read of the
    /// session's own slot, never a trip over USB.
    pub fn tether_settings(&self) -> CameraSettings;

    /// The newest live-view frame, as the JPEG bytes the camera
    /// produced. `None` when live view is off.
    pub fn tether_live_frame(&self) -> Option<Arc<Vec<u8>>>;

    /// Fires the shutter. Enqueues and returns.
    pub fn tether_capture(&self);

    /// Sets one exposure setting to one of the values the body offers.
    /// Enqueues and returns.
    pub fn tether_set(&self, setting: TetherSetting, value: &str);

    /// Starts or stops the live view. Enqueues and returns.
    pub fn tether_live_view(&self, on: bool);

    /// Re-aims the preset arriving shots are developed with.
    pub fn tether_set_preset(&self, preset: Option<PresetId>);
}
```

A tethered capture is not a separate data path: the file received from
the camera goes through the same import core as `Library::import`
(checksum, EXIF, initial revision, thumbnail), and therefore emits the same
`Event::AssetsAdded` (§3.2). It is filed at `Photos/<session>/<name>`, and
the session's preset is applied *before* that event fires, so a client never
shows the neutral render of a shot it is about to develop.

Every call above is non-blocking except `tether_connect`/`tether_disconnect`:
the camera is owned by one thread, and the rest of the API posts orders to it
(ADR 0087 §1). One camera at a time per `Library` in V1.

The `tether` feature removes the libgphoto2 **backend**, never this API: in a
build without it, `tether_connect` reports that this build has no backend and
every other call answers as if nothing were connected.

---

# 6ter. Automatic import from a watched folder (`docs/adr/0039-watched-folder-import.md`)

```rust
impl Library {
    /// Starts watching `folder`: every file that stabilises there
    /// from then on is imported automatically, like an
    /// ordinary import. Refuses a second session while one is already
    /// active.
    pub fn watch_start(&self, folder: &Path) -> Result<()>;

    /// Ends the current session; does nothing if none is active.
    pub fn watch_stop(&self);
}
```

The same principle as tethering (§6bis): a file that stabilises in the
watched folder goes through the same import core as `Library::import`
(checksum, EXIF, initial revision, thumbnail), and therefore emits the same
`Event::AssetsAdded` (§3.2). Only `WatchStarted`/`WatchStopped` are
new, to signal the session itself — one watched folder at a
time per `Library` in V1. Unlike tethering, each file is
imported individually as it comes (never in a batch), so that
the catalog lock `Library::import` holds for the whole of its call
(§3.3) never stays taken longer than a single file, even if the
folder receives many at once — otherwise every interactive operation in
develop mode sharing that same lock would wait behind the entire batch.

---

# 7. Navigation and search

The grid enumerates **versions** (catalog §16).

```rust
pub struct GridQuery {
    pub folder: Option<FolderId>,
    pub collection: Option<CollectionId>,
    pub rating_at_least: Option<u8>,
    pub color_label: Option<ColorLabel>,
    pub pick: Option<PickState>,
    pub keywords: Vec<KeywordId>,        // hierarchical: includes descendants
    pub text: Option<String>,            // FTS5
    pub capture_range: Option<(i64, i64)>,
    pub camera: Option<String>,          // shot filters (ADR 0064): model, or "manufacturer model"
    pub lens: Option<String>,
    pub iso: ShotRange,
    pub aperture: ShotRange,
    pub focal_length: ShotRange,
    pub shutter_speed: ShotRange,
    pub sort: Sort,
    pub range: Range<u32>,               // windowed pagination
}

pub struct GridItem {
    pub version_id: VersionId,
    pub asset_id: AssetId,
    pub filename: String,
    pub capture_date: Option<i64>,
    pub rating: Option<u8>,
    pub color_label: Option<ColorLabel>,
    pub pick: PickState,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub edited: bool,                    // more than its initial revision (ADR 0055 §5)
}

/// An inclusive interval, both bounds optional (ADR 0064 §1).
pub struct ShotRange {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl ShotRange {
    pub fn at_least(min: f64) -> ShotRange;
    pub fn at_most(max: f64) -> ShotRange;
    pub fn between(min: f64, max: f64) -> ShotRange;
    pub fn is_unbounded(&self) -> bool;
    /// Reads the written form `min-max`, `min-`, `-max`, or a single value;
    /// the bounds accept fractions (`1/200`). Empty = no filter.
    pub fn parse(text: &str) -> Result<ShotRange>;
}

impl Library {
    pub fn count(&self, query: &GridQuery) -> Result<u64>;
    pub fn grid(&self, query: &GridQuery) -> Result<Vec<GridItem>>;
    pub fn asset(&self, id: AssetId) -> Result<AssetDetails>;   // full EXIF, versions, paths
    pub fn shot_facets(&self) -> Result<ShotFacets>;            // observed bodies, lenses and bounds
}
```

`grid` + `range` enable virtual scrolling: the UI never loads more than the visible window, whatever the size of the catalog.

The six shot filters (ADR 0064) combine by **and** with the others and with
each other. A photo whose metadata is missing satisfies none of them: it
leaves the grid as soon as one of those filters is set. An inverted
interval is **refused** (`InvalidSettings`) rather than answered with an
empty grid. The lists from which body and lens are chosen come from
`shot_facets`, computed over the whole library (catalog §43) —
a client refreshes them on `AssetsAdded` / `AssetsRemoved`, not on every
keystroke.

---

# 8. Classification

Classification lives on the **version**; keywords on the **asset** (catalog §16).

```rust
impl Library {
    pub fn set_rating(&self, ids: &[VersionId], rating: Option<u8>) -> Result<()>;
    pub fn set_color_label(&self, ids: &[VersionId], label: Option<ColorLabel>) -> Result<()>;
    pub fn set_pick(&self, ids: &[VersionId], pick: PickState) -> Result<()>;

    pub fn add_keyword(&self, assets: &[AssetId], keyword: KeywordId) -> Result<()>;
    pub fn remove_keyword(&self, assets: &[AssetId], keyword: KeywordId) -> Result<()>;
    pub fn create_keyword(&self, parent: Option<KeywordId>, name: &str) -> Result<KeywordId>;
    pub fn keyword_tree(&self) -> Result<Vec<KeywordNode>>;
}
```

Every operation accepts batches: batch processing is a nominal case, not an option.

---

# 9. Collections

```rust
impl Library {
    pub fn create_collection(&self, parent: Option<CollectionId>, name: &str) -> Result<CollectionId>;
    pub fn create_smart_collection(&self, parent: Option<CollectionId>, name: &str, rules: SmartRules) -> Result<CollectionId>;

    pub fn add_to_collection(&self, id: CollectionId, versions: &[VersionId]) -> Result<()>;
    pub fn remove_from_collection(&self, id: CollectionId, versions: &[VersionId]) -> Result<()>;
    pub fn reorder_collection(&self, id: CollectionId, order: &[VersionId]) -> Result<()>;

    pub fn collections(&self) -> Result<Vec<CollectionNode>>;

    pub fn rename_collection(&self, id: CollectionId, name: &str) -> Result<()>;
    pub fn move_collection(&self, id: CollectionId, parent: Option<CollectionId>) -> Result<()>;
    // Takes the subtree with it; returns the number of collections deleted.
    pub fn delete_collection(&self, id: CollectionId) -> Result<u32>;
}
```

The last three touch no version, no revision and no file
(catalog §24): a circular move is refused, and a deletion
takes the descendants and the memberships alone.

**Folders** are read through the same shape, read-only (ADR 0055 §2) — nothing here renames, moves or deletes a folder, that is file management:

```rust
impl Library {
    pub fn folders(&self) -> Result<Vec<FolderNode>>;   // path, parent, photo count
}
```

Rows arrive sorted by path, that is, depth-first: a client can indent on the number of segments without walking a tree.

A smart collection is queried through `GridQuery { collection: Some(id), .. }`: the engine translates `SmartRules` into SQL (catalog §26), and the client sees no difference from a manual collection.

---

# 10. Development

## 10.1 Edit session

Editing goes through a **session**, which embodies coalescing (`pipeline.md`, catalog §17):

```rust
impl Library {
    pub fn edit(&self, version: VersionId) -> Result<EditSession>;
}

impl EditSession {
    /// An in-memory value, real-time preview — no catalog write.
    pub fn set(&mut self, param: Param, value: Value) -> Result<()>;

    /// Commit point: creates the revision, moves the head forward.
    /// The engine applies the amendment window automatically.
    pub fn commit(&mut self) -> Result<RevisionId>;

    pub fn undo(&mut self) -> Result<Option<RevisionId>>;
    pub fn redo(&mut self) -> Result<Option<RevisionId>>;

    pub fn settings(&self) -> &Settings;       // the complete current state
    pub fn history(&self) -> Result<Vec<RevisionInfo>>;
}
```

* `set` is called on every slider movement: the engine updates the preview in memory.
* `commit` is called at the commit points defined by `pipeline.md` (release, tool change…). The session alone decides whether it is a new revision or an amendment.
* Closing the session (drop) commits the pending state: nothing is ever lost.

## 10.2 Versions

```rust
impl Library {
    /// Virtual version: a new branch from the head (or from a given revision).
    pub fn create_version(&self, from: VersionId, name: &str, at: Option<RevisionId>) -> Result<VersionId>;

    pub fn versions(&self, asset: AssetId) -> Result<Vec<VersionInfo>>;
    pub fn set_current_version(&self, asset: AssetId, version: VersionId) -> Result<()>;
    pub fn rename_version(&self, version: VersionId, name: &str) -> Result<()>;
    pub fn delete_version(&self, version: VersionId) -> Result<()>;   // refuses the last version
}
```

---

## 10.3 Presets

A preset (`docs/presets.md`) captures a subset of `Param` (§10.1) — never the complete state — and is applied by writing a normal revision on each targeted version. No new rendering or writing mechanism: application reuses `EditSession::set`/`commit` as they are (`docs/adr/0014-develop-presets.md`).

```rust
pub enum SettingsGroup {
    WhiteBalance,
    Tone,
    Presence,
    LensCorrection,
    Detail,
    Geometry,
}

pub struct PresetInfo {
    pub id: PresetId,
    pub name: String,
    pub groups: Vec<SettingsGroup>,
}

/// Outcome of one preset application batch — the same shape as `ExportReport` (§12).
pub struct PresetApplyReport {
    pub applied: Vec<VersionId>,
    pub failed: Vec<(VersionId, String)>,
}

impl Library {
    /// Captures the fields of the requested `groups` from the head of `from` (§10.1).
    pub fn create_preset(&self, name: &str, from: VersionId, groups: &[SettingsGroup]) -> Result<PresetId>;
    pub fn presets(&self) -> Result<Vec<PresetInfo>>;
    pub fn rename_preset(&self, id: PresetId, name: &str) -> Result<()>;
    pub fn delete_preset(&self, id: PresetId) -> Result<()>;

    /// The job: `JobProgress` per version, then `JobFinished` with the report
    /// (per-version failures in the report, batch failure as `Failed`).
    pub fn apply_preset_async(&self, preset: PresetId, versions: Vec<VersionId>) -> JobId;

    /// Filing and provenance (ADR 0058 §2, §6).
    pub fn file_preset(&self, preset: PresetId, folder: Option<PresetFolderId>) -> Result<()>;
    pub fn favourite_preset(&self, preset: PresetId, favourite: bool) -> Result<()>;
    pub fn update_preset(&self, preset: PresetId, version: VersionId,
                         groups: &[SettingsGroup]) -> Result<u32>;
    pub fn preset_folders(&self) -> Result<Vec<PresetFolder>>;
    pub fn create_preset_folder(&self, name: &str) -> Result<PresetFolderId>;
    pub fn rename_preset_folder(&self, folder: PresetFolderId, name: &str) -> Result<()>;
    pub fn delete_preset_folder(&self, folder: PresetFolderId) -> Result<()>;

    /// `asset` rendered as it would look **with** `preset`, without
    /// applying it (ADR 0058 §4) — the trial a client shows on hover.
    pub fn preset_preview(&self, asset: AssetId, kind: PreviewKind,
                          preset: PresetId) -> Result<Rgb8>;
}
```

* `preset_preview` is a **view**, like `preview_before` and the soft proof: no revision, no provenance, no cache entry — nothing that could later be mistaken for a development anyone asked for. It goes through the same `param_values` mapping the real application uses (`presets::overlay`), so what is shown is what would be written.

* `SettingsGroup` groups the `Param` of §10.1 at the granularity of the preset's checkboxes (`docs/presets.md` §3.1): `Tone` = Exposure + Contrast + Highlights + Shadows + Whites + Blacks, `Presence` = Vibrance + Saturation, `Detail` = NoiseReduction + Sharpening, `Geometry` = Rotation + Crop; the other groups each correspond to a single `Param`.
* `apply_preset_async` is a **job** (§3.1) even for a single version: a selection can go as far as the entire library (`docs/presets.md` §5.2), and a single call category avoids making query-vs-job depend on the size of the selection at call time.
* For every version in the batch: open a fresh session through `Library::edit` (§10.1), `set` each parameter of the included groups, then `commit` once. A brand-new session has no commit history to amend (`last_commit` empty, §10.1): the commit is therefore always a new revision, never an amendment — without having to change the coalescing policy for this case.
* A failing version (typically `NewerSettings`, catalog §17/§3.4, if the preset or the head references a schema the engine no longer knows) joins `PresetApplyReport::failed` with the reason; the other versions in the batch carry on (`docs/presets.md` §5.2).
* `create_preset` knows nothing but the vocabulary of `Settings` (`leyline-core`): it reads the head of `from`, keeps the fields of the requested groups, and serialises the `preset_json` with `schema` = that of that head (`docs/presets.md` §3.2).

---

## 10.4 Reprocessing

Raising each pinned stage of a version to its current version (`pipeline.md` §4.5) — typically so that a photo edited before an operator was fixed benefits from the fix without the user touching a single slider.

```rust
impl EditSession {
    /// Raises the head to the current stage versions: a new revision, the same
    /// parameters. Nothing if there is nothing to do.
    pub fn reprocess(&mut self) -> Result<RevisionId>;
}

/// Outcome of one reprocess batch — the same shape as `PresetApplyReport` (§10.3).
pub struct ReprocessReport {
    pub reprocessed: Vec<VersionId>,
    pub already_current: Vec<VersionId>,
    pub failed: Vec<(VersionId, String)>,
}

impl Library {
    pub fn reprocess(&self, versions: &[VersionId], progress: impl FnMut(u64, u64)) -> Result<ReprocessReport>;

    /// The job: `JobProgress` per version, then `JobFinished` with the report.
    pub fn reprocess_async(&self, versions: Vec<VersionId>) -> JobId;
}
```

* Always a new revision, never an amendment: §4.5 says explicitly that reprocessing "keeps the old" runs — extending the user's last modification in place would lose the distinction between "what the user set" and "what the engine migrated".
* A version already on `CURRENT_PROCESS` produces no revision: `EditSession::reprocess` returns the head unchanged, and `ReprocessReport::already_current` counts it separately — neither a success that writes, nor a failure.
* `EditSession::reprocess` first commits any pending state (the same rule as `undo`/`redo`, §10.1) before migrating: nothing is lost.
* `reprocess_async` is a **job** (§3.1) for the same reason as `apply_preset_async`: a selection can go as far as the entire library.

---

# 10bis. External mask detectors (`docs/adr/0073-external-mask-detectors.md`)

The SDK re-exports `leyline-detect`, which **is not the engine**: a detector is a separate executable, and the SDK does nothing but find it and call it.

```rust
pub struct Detection { pub id: String, pub label: String }

pub struct DetectorSource {
    pub id: String,
    pub label: String,
    pub command: PathBuf,     // absolute, or a name to look for in PATH
    pub args: Vec<String>,
    pub detections: Vec<Detection>,
}

/// `<user config>/Leyline/detectors` — never inside a library.
pub fn manifests_dir() -> Option<PathBuf>;

/// Everything installed and usable, sorted. Never fails: nothing
/// installed, an unreadable manifest or a missing command all give the
/// caller the same thing — no detection to offer.
pub fn discover() -> Vec<DetectorSource>;
pub fn discover_in(dir: &Path) -> Vec<DetectorSource>;

/// Runs a detection: an image in, a coverage out.
pub fn detect(source: &DetectorSource, detection: &str,
              image: &Path, out: &Path) -> Result<(), DetectError>;
```

The call contract fits on one line:

```
<command> <args…> --image <in.png> --detector <id> --out <out.png>
```

`in.png` is a developed preview (8-bit RGB PNG, the format of the preview cache); `out.png` must be a **16-bit grey PNG**, `0` = the setting does not apply, `65535` = it applies fully. It is then up to the caller to make a mask of it through `Library::store_mask_coverage` (§10): **a detector never opens the library**, takes no lock and learns no identifier.

Four failures are named separately (`DetectError`), because they call for four different reactions: a detection unknown to the manifest, a command that does not start, a refusal — with the detector's `stderr`, the only thing that knows why — and a success that writes nothing. A 120 s guard terminates a stuck executable.

---

# 10ter. External pixel processors (`docs/adr/0107-derived-assets-and-the-pixel-socket.md`)

The second socket, and the one difference from the first: the SDK re-exports `leyline-derive` **as a module** (`leyline_sdk::derive`, since the two sockets use the same words for the same ideas), and the **engine** consumes it — because a processor's answer has to be rendered before and imported after, and both are the engine's work.

```rust
pub struct Operation { pub id: String, pub label: String }

pub struct ProcessorSource {
    pub id: String,
    pub label: String,
    pub command: PathBuf,     // absolute, or a name to look for in PATH
    pub args: Vec<String>,
    pub operations: Vec<Operation>,
}

/// `<user config>/Leyline/processors` — never inside a library.
pub fn manifests_dir() -> Option<PathBuf>;
pub fn discover() -> Vec<ProcessorSource>;
pub fn discover_in(dir: &Path) -> Vec<ProcessorSource>;

/// Linear Rec. 2020, white at 1.0, interleaved RGB.
pub struct Exchange { pub width: u32, pub height: u32, pub samples: Vec<f32> }

/// Writes the input, runs the processor, reads and checks the answer.
pub fn process(source: &ProcessorSource, operation: &str,
               image: &Exchange) -> Result<Exchange, DeriveError>;

impl Library {
    /// Develops up to rank 20, runs the processor, and files the answer
    /// as a NEW asset carrying this version's development.
    pub fn derive(&self, version: VersionId, processor: &ProcessorSource,
                  operation: &str) -> Result<AssetId>;
}
```

The call contract, again on one line:

```
<command> <args…> --image <in.tif> --operation <id> --out <out.tif>
```

Both files are **16-bit RGB TIFFs holding linear Rec. 2020, white at 1.0** — the develop buffer as it stood before rank 20, the first place in the pipeline where it has a single meaning whatever the revision says. The answer must have the **same dimensions**: what comes back inherits a development, and a development cannot move to another geometry.

`Library::derive` locks the catalog twice, briefly, with the slow half between (the split of ADR 0024), fires the ordinary `AssetsAdded`, and refuses a destination name already taken. A 600 s guard terminates a stuck executable — five times the detector's budget, because a denoise runs on all thirty million pixels of the original where a segmentation runs on a preview.

---

# 11. Previews

```rust
pub enum Preview {
    /// Up to date for the version's head.
    Ready(PathBuf),
    /// Stale: usable for immediate display, regeneration started.
    Stale { path: PathBuf, job: JobId },
    /// Nothing cached: generation started.
    Generating(JobId),
}
```

The client always displays something immediately (`Ready` or `Stale`), then updates on `PreviewReady`. Validity follows catalog §20 strictly (the head's `revision_id`).

**Delivered surface**: the synchronous get-or-generate, the read-only cache lookup, the render job, and the `Preview` enum (`Ready`/`Stale`/`Generating`) that merges all three into a single call.

```rust
impl Library {
    /// Synchronous get-or-generate: renders the preview if nothing valid is cached.
    pub fn preview(&self, asset: AssetId, kind: PreviewKind) -> Result<PreviewFile>;
    /// Read-only cache lookup: `None` if nothing valid, never renders.
    pub fn cached_preview(&self, asset: AssetId, kind: PreviewKind) -> Result<Option<PreviewFile>>;
    /// The job: `PreviewReady` on success, then `JobFinished`.
    pub fn preview_async(&self, asset: AssetId, kind: PreviewKind) -> JobId;
    /// Merges the three calls above behind the `Preview` enum: an
    /// up-to-date cache returns `Ready` without starting anything; a cache
    /// from a non-head revision returns `Stale` with the stale file and starts
    /// `preview_async`; nothing cached returns `Generating` and starts `preview_async`.
    pub fn preview_state(&self, asset: AssetId, kind: PreviewKind) -> Result<Preview>;
}
```

`cached_preview` answers "what can be shown for the current version", which is not "what the head's render is": the thumbnail of a photo nobody has developed is the picture the file itself carries, and that is the finished answer for that state rather than a placeholder ([ADR 0082](adr/0082-embedded-preview-at-import.md) §1). A client therefore needs no notion of provenance — a cell either has an image or does not. The pipeline takes the thumbnail back the moment there is an edit to show.

`cached_preview` lets the client use the same pattern as `Ready`/`Generating`: display immediately what exists, schedule the generation of the rest — now through `preview_async` and the `PreviewReady` event (Studio can replace its timer with that stream), or directly through `preview_state`.

The `Library` keeps the latest **source buffers** in memory (bounded MRU caches, phase 7): the develop loop re-renders the same asset after every slider commit, and without them each adjustment paid for a full LibRaw decode. Two levels, for the same reason and with the same guarantee:

* the **decodes** themselves, bounded by number of entries;
* the **proxies** derived from them — the decode reduced to the requested preview class before entering the pipeline (ADR 0041 §1) — bounded in memory, because a `Thumbnail` proxy and a `Large` proxy differ by a factor of 250 ([ADR 0076](adr/0076-proxy-cache.md)). A hit on a proxy also avoids the decode: that is what divides by four the cost of one live-render frame (§10.1).

Source files never changing (non-destructive editing), an entry stays valid for the whole life of the process; the pixels served are bit-for-bit those of a fresh decode — or a fresh reduction — (`pipeline.md` §5), and reproducibility is unaffected.

---

# 12. Export

```rust
pub enum ExportRecipe {
    /// Settings supplied by the caller, not stored.
    Adhoc(ExportSettings),
    /// A stored preset (§27), resolved when the request runs.
    Preset(ExportPresetId),
}

pub struct ExportRequest {
    pub versions: Vec<VersionId>,
    pub recipe: ExportRecipe,
    pub destination_dir: PathBuf,
    /// Photos in flight, `None` = the engine's default (4, ADR 0068).
    /// A property of the execution, not of the recipe: never in a preset.
    pub concurrency: Option<usize>,
}

impl Library {
    /// Synchronous: `progress` receives `(done, total)` per version; an
    /// individual failure does not stop the others, it lands in
    /// `ExportReport::failed`.
    pub fn export(&self, request: &ExportRequest,
                  progress: impl FnMut(u64, u64)) -> Result<ExportReport>;
    /// The job: `JobProgress` per version, then `JobFinished` with the
    /// report (per-version failures in the report, failure of the whole
    /// request as `Failed`).
    pub fn export_async(&self, request: ExportRequest) -> JobId;
    pub fn export_presets(&self) -> Result<Vec<ExportPreset>>;
}
```

**Delivered surface** — the single-`ExportRequest` shape described above, with two deliberate differences from this document's initial draft: `recipe` is a union (`Adhoc`/`Preset`) rather than a forced `preset: ExportPresetId` — an ad hoc recipe is a real case (Studio's export dialog, the CLI's `leyline export` without `--preset`), not merely stored presets; and `export` has a synchronous form in addition to the job, necessary for scripted use (CLI, SDK) that does not need to wait for an event for a one-off export. A single request now covers what previously existed as three synchronous entry points (`export`, `export_batch`, `export_with_preset`) and two jobs (`export_async`, `export_with_preset_async`).

Studio now drives its import and export dialogs as well as its grid thumbnails through that stream: `import_async`/`export_async` for the dialogs (progress displayed from `JobProgress`, result from `JobFinished`), `preview_async` for the thumbnails (up to 3 renders in flight, filled from `PreviewReady`).

Export renders each version at its head revision, with the stage versions that revision declares (`pipeline.md` §3.3), and journals into `export_history`. The rendered pixels are in sRGB (`adr/0015-color-management-srgb.md`); `leyline-export` embeds the canonical sRGB ICC profile (generated by LittleCMS, `leyline-color::srgb_icc_profile`) into JPEG, PNG and TIFF files — WebP and AVIF go without, for want of ICC support in their encoding libraries.

`Library::export` processes **several photos at a time** ([ADR 0068](adr/0068-concurrent-export-batch.md)): 4 in flight by default, adjustable through `ExportRequest::concurrency`, because one photo's pipeline does not use three cores out of sixteen. The photos are carried by ordinary threads rather than rayon tasks — the same reason as in §3.1 — while rendering goes on using the global rayon pool inside each photo. Output names are reserved in advance, in request order, in a planning pass that takes the catalog lock **once for the whole batch**: two versions of the same asset contend for the same name deterministically (the second fails, as before) instead of racing towards the same path. The report and the progress are unchanged — request order, `(done, total)` per file written — and the files produced are byte-for-byte identical whatever the degree.

Like `Library::preview` (§11, `adr/0023-catalog-lock-narrowing-preview.md`), `Library::export` holds the catalog lock only for reading the settings (resolving the preset where applicable) and writing the journal — never during the decoding/rendering/encoding of a version (`adr/0024-catalog-lock-narrowing-export.md`). A multi-version request therefore no longer blocks navigation, search or metadata editing for its whole duration, only version by version.

## 12.1 Printing (ADR 0036)

```rust
pub enum PrintRecipe {
    /// Settings supplied by the caller, not stored.
    Adhoc(PrintSettings),
    /// A stored preset (catalog.md §42), resolved when the request runs.
    Preset(PrintPresetId),
}

pub struct PrintRequest {
    pub versions: Vec<VersionId>,
    pub recipe: PrintRecipe,
    pub destination_dir: PathBuf,
    /// Job data, never in the preset — as `ExportRequest.versions`
    /// is to `ExportRecipe`.
    pub copies: u32,
}

impl Library {
    /// Synchronous, the same discipline as `Library::export`.
    pub fn print(&self, request: &PrintRequest,
                 progress: impl FnMut(u64, u64)) -> Result<PrintReport>;
    /// The job: `JobProgress` per version, then `JobFinished`.
    pub fn print_async(&self, request: PrintRequest) -> JobId;
    pub fn create_print_preset(&self, name: &str, settings: &PrintSettings) -> Result<PrintPresetId>;
    pub fn print_presets(&self) -> Result<Vec<PrintPreset>>;
}
```

Printing is neither a process version nor a pipeline stage (ADR 0036): it is "an export with a physical dimension and a destination profile". The engine renders exactly as it does for an export (decode → `render` → the revision's stages), then scales into the printable area in pixels (`PrintSettings::target_pixels`, paper × DPI, in place of an export's `max_edge`) instead of scaling towards a longest edge, optionally transforms into a destination ICC profile (`leyline_color::OutputTransform`, ADR 0027) if `PrintSettings.profile` is filled in, and encodes the result as a one-page PDF (`leyline_export::encode_print`) — the most portable hand-off to an OS printing flow (the risk explicitly named and deferred by ADR 0036): the PDF already carries its physical size and its pixels in the destination profile, ready to be handed as is to the system's print dialog. That hand-off (Studio actually invoking that dialog on the rendered file) remains to be wired in `leyline-studio`, with no new engine surface (the same pattern as the menu bar, ADR 0020).

Unlike export, there is no journalling: no `print_history` table (catalog.md §42) — a print modifies no revision and does not need to be found again later from the catalog.

---

# 13. API stability

* `leyline-engine` is **internal**: its API may change with every version.
* `leyline-sdk` is the **stable surface**: strict semver, `0.x` until V1, then a compatibility commitment on `1.x`.
* The types of `leyline-core` (ids, errors, `Settings`) are part of the SDK contract.
* A C FFI bridge (and therefore Python bindings, and so on) is a planned evolution, outside V1 — the API described here is designed to make it possible (no exposed generics, no lifetimes in public signatures).

```rust
/// The RAW decoder that produced this process's pixels, e.g. "0.21.2-Release".
pub fn decoder_version() -> &'static str;
```

Free-standing, and on the SDK surface deliberately: LibRaw is linked dynamically ([ADR 0004](adr/0004-libraw-decoding.md)), so the decoder is a property of the machine, and [`pipeline.md`](pipeline.md) §5.1 counts it among the terms that must match for two renders to agree bit for bit ([ADR 0086](adr/0086-decoder-in-the-promise.md)). A client reporting *what rendered this* — `leyline --version`, Studio's About dialog — must be able to ask without reaching past the façade into `leyline-raw`.

---

# 14. What the API does not do

* No on-screen rendering: the engine produces files and buffers, display belongs to the client.
* No window management, no shortcuts, no UI selection.
* No network access.
* No direct manipulation of SQLite by clients: the catalog is an implementation, not an interface. The schema (`catalog.md`) is documented for the durability of the data, not as a public API.
