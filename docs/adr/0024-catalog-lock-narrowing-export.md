# ADR 0024 — Not holding the catalog lock during an export render

**Status:** Accepted — 2026-07

## Context

`ADR 0023` narrowed the catalog lock's window around `Library::preview`: the
render (RAW decode + the `process1`–`5` pipeline + encoding) never touches
the catalog, so holding it locked for that time blocked all of Studio's
navigation, search and metadata editing for nothing.

`Library::export`, and the batches it underpins (`export_batch`,
`export_with_preset`, and their `export_async` / `export_with_preset_async`
jobs), had the same flaw, and worse: a preview render costs a few tens of
milliseconds to ~100 ms, but an export adds scaling and a full-size encode
(JPEG/TIFF/WebP/AVIF), and above all `export_batch` chained every version of
the batch **under a single lock taken once at the start** — a batch of
several dozen photos could therefore freeze all catalog access for minutes,
while Studio lets the user go on culling and editing as an export runs in the
background.

Unlike a preview, an export is not a cache indexed by revision that
`valid_preview` could later serve as "up to date": it is a one-shot file the
caller asked for once, written to disk and journaled in `export_history`
(§28) for the record — nothing reads it back afterwards to decide whether it
is still "valid". ADR 0023's atomicity guard
(`record_preview_if_current`), necessary because a concurrent amendment could
pass a stale render off as the valid preview of the revision it had just
rewritten, therefore has no equivalent here: nothing can pass an export off
as anything other than what it is. The worst case of a race with an amendment
stays identical to what it was before this decision — the exported file
reflects the settings read at planning time, not necessarily the very latest
— behaviour already inherent to "export at the head revision", not something
this decision changes.

## Decision

`export::export_version` splits into three phases sequenced by the caller, on
the same model as ADR 0023, with the catalog lock held only for the first and
the last — never during the render:

1. **`plan_export`** (read-only, a short lock): reads the asset, the head,
   the revision's develop settings, the source path and the lens metadata,
   and computes the stem of the output file name.
2. **`render_export`**: no catalog lock held — decode, `process1`–`5`
   rendering, any scaling, refusal if the destination file already exists
   (the "never overwrite" rule), and encoding to disk.
3. **`journal_export`** (a write, a short lock): records the export in
   `export_history`.

`Library::export` sequences those three phases, releasing the lock between
the first and the second. `Library::export_batch` and
`Library::export_with_preset` no longer take the lock once for the whole
batch: they call `Library::export` version by version, so the lock is never
held longer than the plan plus the journal of **one** version at a time — the
same constraint as if the client called `export` in a loop itself.

> **Correction, 2026-08-05.** This paragraph no longer describes the engine,
> on both its halves. The three methods it names have merged into `export` /
> `export_async` around an `ExportRequest`
> ([ADR 0025](0025-unified-export-request.md)). And the batch's split has
> **inverted**: [ADR 0068](0068-concurrent-export-batch.md) §2 now plans every
> version of a batch **under a single lock**, in request order, so that two
> versions of the same asset collide deterministically rather than racing for
> the same output path.
>
> What stays true, and is this ADR's decision: the split into three phases,
> and **no lock held during a render**. ADR 0068 §3 notes that this discipline
> is not merely preserved but has become necessary — a batch's photos now
> render in parallel, and a worker holding the catalog would serialize them
> all.

The free functions `export::export_version` and `export::export_batch` (used
directly by this crate's integration tests) keep their `&mut Catalog`
signature held end to end — they drive the three phases under a single lock,
as `preview::preview` does for ADR 0023.

## Consequences

* All of the catalog's navigation, search and metadata editing stays
  available during an export or a batch of exports, including a batch of
  several dozen versions — the global freeze disappears.
* No catalog schema change, and no new catalog method (unlike ADR 0023, no
  atomicity guard is needed here); the public signatures of
  `Library::export`, `export_batch`, `export_with_preset` and their jobs are
  unchanged.
* No change to the `process1`–`5` formulas nor to the format of the journaled
  file: only when the catalog lock is held, and how a batch is split per
  version.

## Alternatives rejected

* **Keeping the lock for the whole batch but releasing it between versions**
  (instead of routing every version through `Library::export`): equivalent in
  practice, but it duplicates the narrowing logic already written for the
  single call — routing through `Library::export` reuses the same code and
  guarantees that a future change to the narrowing (a future guard, say)
  applies to both paths without double maintenance.
* **An atomicity guard in the style of `record_preview_if_current`**:
  rejected, cf. Context — nothing reads an export back to decide whether it
  is "up to date", unlike a preview.
