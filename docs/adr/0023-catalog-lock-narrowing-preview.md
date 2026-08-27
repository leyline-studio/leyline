# ADR 0023 — Not holding the catalog lock during a preview render

**Status:** Accepted — 2026-07

## Context

`Library::preview()` holds the single `Mutex<Catalog>` — the one every read
(`catalog()`) and every write (`catalog_mut()`, `edit()`) goes through — for
its whole duration, including the complete render (RAW decode + the
`process1`–`5` pipeline + PNG encoding), which never touches the catalog. A
render costs from a few tens of milliseconds to ~100 ms (the `process1.rs`
benchmarks); the bounded render pool admits up to 16 concurrent jobs. Every
render — and every `preview_async` job in the pool — therefore blocks all of
Studio's navigation, search and metadata editing for the duration of the
render, when the render itself needs no catalog access whatsoever.

`rusqlite::Connection` is `Send` but `!Sync`: `Catalog` wraps a single
connection, so an `RwLock<Catalog>` would not compile for concurrent reads —
it is not only logical consistency that the mutex protects, it is the single
connection itself. A connection pool (many readers plus one writer, which WAL
permits) would address contention more broadly, but that is an invasive
change to all of `leyline-catalog`'s API (borrows, transactions,
opening/migration) for a gain wider than the bug targeted here — excluded
from this decision, to be revisited separately if read-versus-write
contention ever proves measurable.

Simply narrowing the lock's window (taking it to read the settings, releasing
it during the render, taking it again to write the preview) looked risk-free
at first glance: between the two acquisitions the head revision could advance
(`commit_revision`) or move (`undo`/`redo`), but those operations create a
new revision or move the head onto an existing immutable revision — the
preview being recorded stays a *correct* preview of the revision R that was
rendered, merely no longer the head, and therefore ignored by `valid_preview`
until one comes back to it. Nothing incorrect.

There is however one exception: `try_amend_head` (§17, the 2 s amendment
window) **rewrites a revision's `settings_json` in place, keeping the same
identifier R**, and deletes R's previews (a rule already documented,
`catalog.md` §17). Without the lock held end to end, the following sequence
becomes possible:

1. The render thread reads head = R, settings = S1.
2. It renders S1 with no lock held.
3. A concurrent amendment rewrites R: R now means S2, and R's previews are
   deleted.
4. The render thread records its preview for R with the S1 pixels.
5. `valid_preview` for head = R finds that row and serves it as R's valid
   preview — while the pixels are S1 and R means S2: a stale preview served
   as fresh, with no freshness signal by which to detect it.

Narrowing the window and nothing else therefore reintroduces genuine silent
corruption, not merely a wasted render.

## Decision

`preview::preview` splits into three phases sequenced by the caller, with the
catalog lock held only for the first two and the last — never during the
render:

1. **`plan_preview`** (read-only, a short lock): serves the cache if a valid
   preview already exists, otherwise reads the head, the settings, the source
   path and the metadata, and captures the revision's raw `settings_json`
   string.
2. **Decode + render**: no catalog lock held; the decode cache
   (`Mutex<DecodeCache>`) is held only for the duration of
   `get_or_insert_with`, which returns an owned `Arc<RawImage>`.
3. **`record_render`** (a write, a short lock): records the preview through a
   new catalog method, `record_preview_if_current`, which compares — in the
   same transaction as the write — the `settings_json` string captured in
   phase 1 against the one currently stored for that revision. Equal ⇒ the
   write happens; different (or the revision gone) ⇒ nothing is written, the
   rendered file stays displayable for that call but is not marked valid, and
   a later `preview` call regenerates.

The guaranteed invariant: a concurrent amendment can never pass off a render
obtained with old settings as valid. `commit_revision`, `undo` and `redo`
never modify an existing revision's `settings_json`, so they never trip this
guard — only `try_amend_head` can, which is exactly the case in view.

## Consequences

* All of the catalog's navigation, search and metadata editing stays
  available during a render, including under load from the bounded render
  pool (up to 16 concurrent jobs) — the global block disappears.
* A new additive catalog method (`Catalog::record_preview_if_current`), no
  schema migration, and no change to the engine's public API
  (`Library::preview` keeps its signature).
* The worst case of a race with an amendment is a wasted render, never a
  corrupted preview — the next call regenerates normally. This reinforces the
  rule already documented in `catalog.md` §17 (an amendment invalidates the
  revision's previews) instead of changing it.
* RAW decoding stays serialized by `Mutex<DecodeCache>` for its own duration
  (unchanged, out of scope) — a future tightening of that particular section
  would remain an independent and more modest fix, if ever measured
  necessary.
* No change to the `process1`–`5` formulas: only when the catalog lock is
  held, never the order or the value of the pixel computations.

## Alternatives rejected

* **`RwLock<Catalog>`**: does not compile for concurrent reads,
  `rusqlite::Connection` being `!Sync` — no benefit.
* **A reader/writer connection pool (WAL)**: a relevant long-term direction
  for read-versus-write contention in general, but an invasive change across
  all of `leyline-catalog` for a gain wider than the bug targeted here;
  deferred to a separate decision should the contention become measurable.
* **A per-asset lock**: adds a map of locks and complexity without answering
  global catalog contention (navigation touches every asset); the global
  catalog mutex would still be held for the SQL operations themselves.
* **Narrowing the window with no atomicity guard**: rejected — it
  reintroduces the silent corruption described above, for the benefit of the
  amendment window alone, a real case and one already in use (dragging a
  slider).
