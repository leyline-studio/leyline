# ADR 0075 — The preview cache keeps a window, not the whole history

**Status:** Accepted — 2026-08

## Context

A preview is indexed by `(asset, revision, kind)` and **an ordinary commit
leaves the previous revision's in place**. That is deliberate:
[`catalog.md`](../catalog.md) §20 makes it a property — "an undo that brings
the head back onto an already-previewed revision automatically revalidates the
old previews: no regeneration is necessary".

What was not decided is **when they go away**. The answer: never. Only an
*amendment* of the head deletes its own
(`Catalog::remove_revision_previews`). There is no size ceiling, no eviction
and no purge command, neither in Studio nor in the CLI.

### What it costs, measured

On a Canon 5D IV photo from the real corpus:

| | Weight |
|---|---|
| The CR2 | ~35 MB |
| A 1024 px preview (the develop view's) | **0.68 MB** |
| A 2048 px preview | 2.47 MB |
| A 4096 px preview | 9.03 MB |

Three virtual copies of a photo therefore cost ~2 MB of previews against 35 MB
of RAW: **+6 %**, and not ×3 — a point worth writing down, because the
spontaneous fear is that "the photos are duplicated", when a develop version is
a **row** referencing the asset and a reprocessing writes a JSON revision.

The real risk is elsewhere and it is real: **a hundred edits on one photo leave
~70 MB of stale previews, more than the RAW itself**. On a library worked for
years, that is where the disk goes.

### What we already know, and what decides the shape

A cold preview costs on the order of a **second** (decoding included), and the
cache is **rebuildable by definition** — deleting it loses nothing. A sliding
window therefore suffices: beyond it, we regenerate rather than keep.

## Decision

### 1. A window per photo, and the heads always

**Kept**, for a given asset:

1. the preview of **each version's head** (a virtual copy) — a copy parked on
   an old revision must keep its preview, without which the grid would start
   rendering again on every scroll;
2. the previews of that asset's **three most recent revisions**.

Everything else is evicted: the row deleted, the file deleted.

The second rule is what keeps undo *and* redo instant around the point of work:
after an undo, the head is a recent revision, and the one just left — redo's
target — is too. Both are in the window without our having to reason about
which way the head last moved.

**Three**, because that is what it takes to cover the back and forth of a
setting without memorizing a whole session. The number is a named constant, not
a setting: a user should not have to arbitrate a cache size, and the window
costs at most ~2 MB per photo *actually edited*.

### 2. Beyond the window we regenerate — and we do not preload

Going further back in the history makes the preview missing. The existing path
handles it already without one extra line: `latest_preview` serves the most
recent stale image, marked `Preview::Stale`, while the right one is computed
(`engine-api.md` §11).

**Nothing is preloaded in the background**, and that is a choice: the case
arises only on the fourth consecutive undo — the first three fall inside the
window — and it then costs a second, once. Building an anticipation for that
would amount to rendering images nobody will look at, which is exactly the
waste this ADR corrects.

### 3. Eviction happens where the cache grows

After each recording of a preview (`Library::preview`), and there alone.

No periodic sweep, no background task, and no "empty the cache" left to the
user: the only moment the cache can exceed its window is the one where
something has just been added to it. A purge attached to that point is bounded
by construction and needs no scheduler.

An accepted corollary: a library one no longer opens does not clean itself. That
is consistent — a cache that no longer grows has nothing to give back.

### 4. What eviction never touches

* **The photos.** Nothing in this document concerns the imported files: the
  cache lives in `Cache/`, and a library whose entire folder is deleted renders
  exactly the same pixels, one second later.
* **The revisions.** None is deleted: the history stays whole and stays
  replayable. We evict *derived images*, never an intent.
* **The §5.1 promise.** A preview is not an export render; there is neither a
  stage version nor a guaranteed pixel in this affair.

## Consequences

* **The cache becomes `O(photos edited × 3)` instead of `O(edits)`.** That is
  the change of shape that matters: the first is predictable and proportionate
  to the library, the second grows with the time spent working.
* **A silent flaw disappears.** Nothing in the interface would have signalled a
  40 GB cache: no error and no slowdown — only a full disk one day, with an
  untraceable cause.
* **`catalog.md` §20 gains its other side.** The document said when a preview
  becomes valid again; it now also says when it ceases to exist.
* **No migration.** The rule bears on rows we delete, not on the schema: an
  existing library settles into its window at the first preview it records.

## Alternatives rejected

* **A global ceiling in gigabytes, with LRU eviction.** The first idea, and the
  worse one: it requires a setting from the user, a size accounting, and it
  evicts by access age — hence potentially the preview of the photo being
  looked at, on a library at its ceiling. A window per photo cannot pick the
  wrong target.
* **Keeping only the head.** One revision, one preview: simple, and it makes
  every undo cost a second when going back and forth on a setting is
  development's most ordinary gesture.
* **Preloading the next revision in the background.** It renders images nobody
  will look at in the common case; see §2.
* **An "empty the cache" command.** It solves nothing — it moves the problem
  onto the user, who must first discover they have one. Nothing prevents adding
  it later as a convenience; it is not the answer.
* **Purging when the library opens.** It makes a complete sweep payable at
  startup for an overrun that arrives one preview at a time.
