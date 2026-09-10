# ADR 0141 — Selecting everything, and what that then costs

**Status:** Accepted — 2026-09

## Context

Studio has no *Select All*. Counted in the source: no `select-all` anywhere,
no `Ctrl+A`, nothing in a menu. The selection is built one click at a time —
`Ctrl`-click to add, `Shift`-click for a run — which is enough for a handful
of photographs and nothing at all for the audience `vision.md` names second:
*a large library, ratings, collections, keywords, batch processing*.

Adding the key is two lines. What the key uncovers is the ADR.

`App::multi_selected` holds **grid indices**, and its own doc comment says
what happens to the rest: *indices scrolled out of the loaded window are
simply dropped from any batch action, since only currently-loaded `items` can
resolve to a `VersionId`*. The grid is virtual — a window of rows around the
viewport, a few dozen of thirty-eight thousand — so a *Select All* built on
that would rate the forty photographs on screen and drop the rest, silently.
That is worse than not having the feature.

And there was a second thing waiting. Rating sixty photographs at once, on the
built binary: **101 % CPU, keyboard dead, window frozen**. Not a deadlock — a
spin. Each `VersionChanged` event triggers a full grid reload, `set_rating`
emits one per photograph, and sixty reloads each re-read the catalog and
re-request every visible thumbnail. Sixty photographs did that. Thirty-eight
thousand would have been unreachable by any route, which is why nobody had
found it: nothing in the interface made a selection that large easy to build.

## Decision

### 1. `Ctrl+A` selects what the grid is showing

Not the library: the **query**. A grid narrowed to three stars, to a
collection, or to a folder selects what that filter answers — the number the
line under the filter bar already gives. `Ctrl+D` goes back to the one focused
photograph, on Lightroom's key rather than an invented one.

Both are in the Library menu with their keys printed beside them, and on the
shortcuts card, under the rule this interface keeps: a key that exists is a key
that is written down somewhere the user will meet it.

### 2. A selection reaches past the loaded window

`selected_items` resolves the selection against the catalog: rows inside the
loaded window come from it, and the rest are fetched with **one** grid query
over the span they cover, under the query the grid is showing. `Ctrl+A` on
thirty-eight thousand photographs is therefore one query, not thirty-eight
thousand lookups, and every batch action — rating, labels, flags, keywords,
collections, export, print, contact sheet, removal — inherits it by going
through `selected_versions` / `selected_assets`, which they already did.

A failing query drops the rows it could not read rather than failing the
action. That is what this code did before the selection could leave the
window, and a batch that half-works on the visible rows is the behaviour the
photographer already had.

The **undo snapshot** goes through the same door. `classement_of` read the
loaded window, so an undo after a large rating would have restored forty
photographs out of thirty-eight thousand — silently, which is the worst way
for an undo to be wrong.

### 3. The count says what a batch would act on

The line under the filter bar reads « 60 photos » and, while several are
selected, « 60 photos sélectionnées sur 60 ». Said there rather than in a bar
of its own: it is the same sentence one clause longer, in the place a
photographer already looks to know how many photographs are in front of them.

### 4. Printing takes the selection

The print dialog took `item_at(selected)` — the **focused** photograph — and
ignored the rest, while `print_async` has taken a list of versions since
[ADR 0036](0036-print-module.md) and the CLI has printed several from the
start. Select twelve, press `Ctrl+P`, get one print: a selection that some
actions honour and others quietly narrow is worse than one nobody can make.
The dialog now says *Imprimer les 12 photos sélectionnées*, the way export
already did.

### 5. One reload per drain, not one per event

The event pump drains the engine's channel in a loop, and the `VersionChanged`
arm reloaded the grid inside it. A batch action sends one event per
photograph, so the pump reloaded once per photograph — the spin above.

The arm now raises a flag and the pump reloads **once**, after the drain, for
whatever the batch touched. Measured on the same sixty photographs: 101 % CPU
and an unusable window become 0 % and an interface that answers the next
keystroke. `AssetsAdded` goes through the same flag, for the same reason at a
smaller scale.

This is the second time a per-item cost has been the thing that made a feature
impossible rather than slow ([ADR 0082](0082-embedded-preview-at-import.md)
was the first, at 695 ms a file). The pattern to keep: **an engine event that
means "something changed" is a request to refresh, not an instruction to
refresh now.**

## Consequences

* A selection can now name every photograph in a library, and every batch
  action honours it — including the undo of one.
* `App` gains `pending_reload`. Anything that adds a per-event reload from now
  on should set it instead, and the pump is the one place a reload is
  scheduled.
* Batch actions are still **synchronous**: `set_rating` on thirty-eight
  thousand versions is one SQL statement, but the keywords, collections and
  removal paths write per item. If one of them proves slow at that scale it
  becomes a job with a task bar ([ADR 0139](0139-cancelling-a-batch.md)) —
  measured first, as ADR 0139 §5 did for reprocessing.
* Nothing was added to the grid's own chrome: no floating action bar over the
  selection, no per-cell checkbox. The actions already exist in the menus and
  on the keyboard, and the count is the only thing that was missing.

## Alternatives rejected

* **Selecting only the loaded window** — what a naive `Ctrl+A` would have
  done. It is the silent-partial-write this ADR exists to avoid.
* **Holding the selection as version ids instead of indices.** Tempting, and
  it would make `selected_items` trivial; it also breaks `Shift`-click, which
  is a range *of the current sort*, and would make a selection survive a
  filter change into a state nothing on screen explains.
* **A "select all" that also loads every row into the grid model.** Thirty-
  eight thousand cells is the virtual scrolling this grid is built on thrown
  away for a selection.
* **A floating batch-action bar** over the grid. The menus already carry the
  actions with their keys; a second surface for the same verbs is a second
  place to keep in step.
* **Debouncing the reload on a timer** instead of coalescing per drain. A
  timer picks a delay nobody can justify; the drain is already the batch
  boundary.
