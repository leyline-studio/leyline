# ADR 0021 — Context menus (right-click)

**Status:** Accepted — 2026-07

## Context

ADR 0020 settles global discoverability (the menu bar), but an action aimed at *one particular object* — a photo in the grid, the develop canvas — stays more natural as a right-click than as a hunt for the corresponding object in an already-open global menu. Ordinary desktop applications (and the expectation of a user coming from other photo software) offer both: a menu bar for global commands, right-click for commands bearing on whatever sits under the cursor.

## Decision

Two context menus, both strictly shortcuts to actions already decided (ADR 0020) or already implemented — the same constraint: **no new functionality**, only a second way in.

**The library grid, right-clicking a thumbnail:**

* Develop *(D)*
* ——
* Rate ▸ (0–5), Label ▸ (colours), Flag ▸ (Pick/Reject/Clear) — `docs/catalog.md` §8, the same actions as `classify.rs`
* ——
* Add to Collection *(B)*, Remove from Collection *(Shift+B)*
* ——
* Reprocess — **not** the `EditSession::reprocess` path of the `R` shortcut (which requires an open develop session), but `Library::reprocess` (§10.4) applied to the clicked version (or to the current selection if several thumbnails are selected): the same call as `Shift+R`, merely bounded to a subset instead of the whole library. No API change required.

If several thumbnails are selected and the right-click lands on one of them, the context menu acts on the whole selection (the standard convention) rather than on the clicked thumbnail alone.

**The canvas, right-clicking in develop mode:**

* Reset Crop — sets `crop` back to `None` (the same effect as the *Reset* button already in the Geometry panel)
* Compare Before/After — toggles the same state as the control already at the top of the canvas
* ——
* Reprocess *(R)*
* ——
* Back to Library *(G)*

## Consequences

* No new engine capability: every entry calls a path already decided by ADR 0020 or already wired in `main.rs`/`leyline-engine`.
* The collections and keywords tree (the left panel) has **no** context menu in this V1: `leyline-engine` exposes no renaming or deletion of a collection or a keyword on the façade today — adding a right-click there would invent a capability that does not yet exist. To be revisited if and when that capability is decided separately.

  > **Correction, 2026-08-05.** The condition posed here came about, and the
  > revisit took place: the façade exposes `rename_collection`,
  > `move_collection` and `delete_collection`, and the collections tree now
  > carries the corresponding context menu — three entries that each open a
  > dialog, because each needs a piece of data Rust must fetch first (the
  > current name, the legal parents, the size of the subtree). That is exactly
  > the "if and when" this sentence anticipated, and the right-click pattern
  > decided here is what applies to it.
  >
  > **Keywords still have no context menu**, and for the original reason,
  > which is intact: the façade knows how to create a keyword and attach it,
  > not how to rename or delete one.
* Reprocess from the grid using the batch API (rather than the single-session API) avoids requiring a photo to be open in develop just to reprocess it — consistent with the spirit of `Shift+R` (reprocess without opening).

## Alternatives rejected

* **A single generic context menu shared everywhere**: it would lose the distinction between actions bearing on a photo (the grid) and actions bearing on the current editing state (the canvas) — the two lists barely overlap.
* **Reprocess through `EditSession::reprocess` (as `R` does) instead of `Library::reprocess`**: it would force a develop session open for every clicked photo, slower and more intrusive than the batch reprocessing already designed for this use.
