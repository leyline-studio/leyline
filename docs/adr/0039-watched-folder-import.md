# ADR 0039 — Automatic import from a watched folder

**Status:** Accepted — 2026-07

## Context

ADR 0038 (tethered capture) already noted the generic watch folder as a
possible extension, set aside at the time in favour of libgphoto2 for the
"camera plugged in over USB" case. The need reappears for a different case: a
folder where third-party software, a networked card reader or any external
process drops files — not necessarily a camera — and that the user wants
imported automatically, without launching a manual import each time. That is
Lightroom's "Auto Import" feature.

The survey of Studio/Lightroom-Darktable gaps of 2026-07-24 (see
[[studio-workflow-gaps-progress]]) had identified it as the last real
"workflow" gap, deliberately deferred: a new dependency (filesystem watching)
and a new background-thread life cycle — heavier than the seven other gaps
already shipped that day.

## Decision

Implemented directly in `leyline-engine` (not a new separate crate, unlike
`leyline-tether`): the dependency added, `notify` (a pure Rust crate, portable
across Linux, macOS and Windows), wraps no system C library needing per-platform
packaging — it does not justify the same separation as
LibRaw/Lensfun/LittleCMS/libgphoto2.

A new `watch.rs` module, which deliberately takes the shape of
`leyline_tether::TetherSession` (the same "background thread plus a
non-blocking callback" contract):

* `WatchSession::watch(folder, on_event)` — starts a recursive
  `notify::Watcher` on `folder` and a dedicated thread that debounces the raw
  filesystem events into "settled" files: a create/modify event (re)starts
  tracking a path, and on every tick (500 ms) any path whose size has not
  changed for `STABILITY_WINDOW` (2 s) is promoted to
  `WatchSessionEvent::Ready`. Necessary because a real file drop (a copy from
  a card, a network write) is not atomic: without that debounce an import
  would start on a half-written file.
* Only the extensions `leyline_engine::import::media_type` recognizes enter
  tracking — an XMP sidecar or a temporary file never fires an event.
* `WatchSession::stop()` (and `Drop`) — stops watching.

As for tethering, that module never touches the catalog:
`Library::watch_start(folder)` starts a session and, for every ready file,
calls the existing import core (`Library::import`, `copy_files: true`) — **an
import from a watched folder is an import like any other**, the same BLAKE3
checksum, the same thumbnail, the same `Event::AssetsAdded`. Two new events,
only for the session's life cycle, on the same pattern as
`TetherConnected`/`TetherDisconnected`:

```rust
pub enum Event {
    // ...
    WatchStarted { folder: PathBuf },
    WatchStopped { reason: Option<String> },
}
```

One session per `Library` (one watched folder at a time), refused if a session
is already open — the same contract as `tether_connect`.

**Every settled file is imported individually, never in a batch.**
`Library::import` holds the catalog mutex for its whole call (see its own
documentation); that mutex is also on the path of every interactive operation
in develop mode (commit, rating, preview — ADR 0023, ADR 0024). A folder
receiving many files at once (a bulk import from a card) imported one by one
therefore keeps each locking window short — never longer than a single file —
instead of blocking the catalog for the duration of the whole batch.
`handle_watch_event` takes exactly the same construction as
`handle_tether_event` here. A dedicated benchmark
(`leyline-engine/benches/import.rs`, group `import`) measures that per-file
cost — checksum + catalog write + thumbnail render — so as to catch any
regression that would lengthen that window.

## A deadlock bug found while writing the tests

`Library::watch_start`/`watch_stop` were first written by literally reusing
`tether_connect`/`tether_disconnect`'s code, including
`if let Some(session) = lock(&self.inner.watch).take() { session.stop(); }`.
Since `leyline-tether` cannot be exercised without real USB hardware, that
line had never run outside a physical test bench. `notify`, by contrast, is
entirely testable locally (no hardware required) — and the very first
end-to-end test (`tests/watch.rs`) hung indefinitely on `watch_stop()`.

The cause: in Rust, the temporary mutex guard produced by `lock(...)` in the
scrutinee of an `if let Some(x) = EXPR { BODY }` lives for **the whole
block**, not only for the evaluation of `EXPR` (temporary lifetime extension).
The `watch` mutex therefore stayed locked for the whole of `session.stop()`,
which blocks waiting for the background thread to finish — except that the
thread, on leaving its loop, must itself lock `watch` to publish its own
`WatchSessionEvent::Stopped` (`handle_watch_event`). A classic deadlock, the
main thread against the session thread, on the same mutex.

The fix — in `watch_stop` **and** in `tether_disconnect`, which carried the
same latent bug, never detected for want of a test able to exercise it:

```rust
// Before (a deadlock if a background thread must retake that same mutex
// before finishing):
if let Some(session) = lock(&self.inner.watch).take() {
    session.stop();
}

// After: the `take()` is a statement of its own, so the guard is released
// before the blocking call.
let session = lock(&self.inner.watch).take();
if let Some(session) = session {
    session.stop();
}
```

Both methods now carry a comment explaining why the single-statement form is
incorrect, not merely a style to avoid.

## Consequences

* A new dependency: `notify` (pure Rust, `default-features = false` plus
  `macos_fsevent` — the crate's only default). No per-platform packaging to
  plan for, unlike `libgphoto2` (ADR 0038).
* `docs/specification.md` §Included gains "Automatic import from a watched
  folder".
* `leyline-cli` gains `leyline watch <library> <folder>` — the same pattern as
  `leyline tether`.
* Studio's interface: File ▸ Auto Import… opens a modal panel (a folder
  picker, Start/Stop, a count of files imported this session) — one more
  client of the API above.
* `tether_disconnect` was fixed of the same deadlock bug as `watch_stop`,
  though this ticket's subject was auto-import: a fix prompted by the real
  test, not a deliberate scope extension.

## Alternatives rejected

* **A new `leyline-watch` crate** (like `leyline-tether`): rejected — `notify`
  is pure Rust, with no system C library to isolate; `leyline-tether`'s
  separation into a crate serves precisely to isolate the FFI binding to
  libgphoto2, absent here.
* **Importing the whole folder as a single batch per polling cycle**: simpler,
  but it would hold the catalog mutex for the duration of the whole batch on
  every wave of files — see the dedicated section above.
* **Tracking by `mtime` alone rather than by stable size**: simpler, but an
  `mtime` does not necessarily change on every write depending on the
  filesystem and the copying tool used, whereas a size stable across two
  consecutive polls is a direct guarantee that no write is in progress.
