# ADR 0130 — Direct manipulation: dragging, unfolding, and pointing at a tone

**Status:** Accepted — 2026-09

## Context

Counted on the built binary, there is **no drag-and-drop anywhere in
Studio**, and no gesture that acts on the photograph itself rather than on a
control beside it. Three consequences, each of which the two reference
applications answer:

* **Filing a photograph in a collection** goes through `B`, or the Library
  menu, or the right-click menu — all of which require the collection to
  already be the *active* one, chosen in the sidebar. What one wants to do is
  put these photographs *there*, and "there" is on the screen.
* **A folded panel has no way back but a key nobody was told about.** `Tab`
  folds the side panels, and when they are folded nothing on screen says they
  exist. [ADR 0125](0125-narrow-window-develop.md) has just made the same fold
  automatic below a threshold, which makes the missing affordance worse: a
  panel can now disappear without the user having done anything at all.
* **Nothing on the photograph adjusts the photograph.** Every tonal decision
  is made on a track in the right-hand panel, and a photographer's actual
  question is about a *place in the picture* — "that sky is too bright" — not
  about a number.

## Decision

### 1. Photographs drag onto a collection

Press on a grid cell, move 6px, and the pointer carries a label saying how
many photographs; a collection row lights up as the pointer crosses it, and
the release files them there.

Two rules that are the decision rather than the implementation:

* **What is dragged is what was under the pointer** — the whole selection
  when the dragged cell belongs to it, that one photograph when it does not.
  Dragging a cell outside the selection is a statement about that cell, and
  must not silently file fifty others.
* **A smart collection is not a drop target.** Its members are its query's
  answer ([ADR 0064](0064-metadata-filters.md), `docs/catalog.md` §24);
  dropping into one would be a request the collection cannot keep.

The mechanism is worth recording because Slint 1.13 has no drag-and-drop and
the obstacle is not the drawing: while a `TouchArea` is pressed it holds the
**pointer grab**, so every move goes to it alone and a sidebar row can never
learn that the pointer is over it — `has-hover` there stays false for the
whole drag. So the dragged cell publishes the pointer in *window*
coordinates, and each row tests that point against its own
`absolute-position`. Nobody computes anybody else's geometry, and a row that
moves takes its own hit test with it.

### 2. A folded panel says so, and the browser folds in two stages

**A handle.** When the side panels are folded *by hand*, a 14px strip at the
edge of the browser brings them back. `Tab` goes on working and stays what an
experienced user reaches for; the handle is what tells everyone else that
there is something there.

Only for the manual fold. When the window is too narrow the fold is a
**constraint**, not a preference — the filter bar cannot shrink past its own
controls — and a handle that promised to undo it would either lie or push a
panel off the edge of the window, which is precisely the defect ADR 0125
exists to remove.

**Two thresholds.** That constraint is also blunter than it needs to be. The
browser folded *both* side panels at 1400px, and its own comment in
`studio.slint` records what the 1340px measurement was actually about: the
**metadata** panel going off the edge. So the two are separated — the
metadata panel folds at 1400px, the sidebar not until 1100px — and a 1366px
laptop, which is still an ordinary machine, keeps its folders and its
collections instead of losing both panels at once.

Which one yields first is the same judgement ADR 0125 §1 made for Develop,
and [ADR 0055](0055-library-navigation.md) §6 had already written the
premise: in the browser the metadata panel "is informational, and the grid is
still a grid without it". The sidebar is how one reaches a folder at all.

### 3. Pointing at a tone adjusts it

A **Target** tool: press on a place in the photograph, drag up to lift that
tone and down to lower it.

The whole of what needs the pixels is *which* setting the tone belongs to,
and that is one function on the displayed render Studio already holds for the
R/G/B readout ([ADR 0112](0112-four-panel-affordances.md) §2). It answers with
a **name** — `blacks`, `shadows`, `exposure`, `highlights`, `whites` — and
everything after is the panel's own arithmetic on a slider it already knows,
through the same preview-then-commit pair a drag on the track uses
([ADR 0074](0074-live-preview-while-dragging.md) §1). No new engine call, no
new stage, no revision shape that did not exist.

**Bands, not weights.** Lightroom's equivalent distributes one gesture across
several sliders by weight. Here the luminance falls in exactly one band and
exactly one slider moves, because a photographer who has just dragged on a
sky should be able to say afterwards what changed — and because a weighted
blend is a rendering decision hidden inside an input gesture.

The tool lives in **Basic**, beside Select and Crop, which amends
[ADR 0054](0054-first-run-and-basic-mode.md) §2's table of two. It has to:
the sliders it drives are Basic's own, and a tool that moved *Exposure* from
a mode that hides *Exposure* would be incoherent. It is also, of the three
tools there, the least technical — it asks the user to point at what is
wrong.

It stays armed after a release, unlike the eyedroppers, which hand back to
Select: a picker answers one question, and adjusting a photograph by pointing
at it is a sequence of pulls.

### 4. Out of scope

* **Dropping onto a folder.** A folder is a place on disk; dropping a
  photograph there means *moving the file*, which is
  [ADR 0100](0100-file-renaming.md)'s territory and a different promise.
* **Dropping files from the desktop into the window.** Genuinely wanted, and
  it depends on backend support Slint 1.13 does not expose to `.slint`.
  Import stays the dialog it is.
* **Reordering a collection by dragging inside it.** `collection_versions`
  carries a position, so it is buildable; it is a decision about what a
  collection's order *means* and does not follow from this one.
* **A targeted adjustment for the HSL mixer** (drag on a colour to move its
  band). The same mechanism, one more question — which of hue, saturation and
  luminance is being dragged — and that question has no obvious answer.

## Consequences

* Two new globals of pure interface state, `DragState` and — from
  [ADR 0127](0127-hints-on-wordless-controls.md) — `HintState`. Both are the
  exception ADR 0045 §2 allows, and both earn it the same way: the two ends
  are in different panels and neither owns the other.
* `GridState` gains `selection-size`, set from the single place the selection
  ever changes, so the label under a drag cannot disagree with what the drop
  will do.
* `BrowserPanel`'s one fold flag becomes two, and `refresh-fold()` sets one
  more property from the same handler.
* The drop reuses [ADR 0129](0129-library-undo.md)'s stack, so a
  mis-drop is `Ctrl+Z`.
* `develop.rs` gains one pure function over the displayed render, with the
  band boundaries in one place.

## Alternatives rejected

* **A `PopupWindow` "drop menu" on release**, listing the collections. It
  turns a direct gesture back into a menu, which is what this ADR is about
  not doing.
* **Auto-scrolling the sidebar while dragging near its edge.** Real, and a
  timer plus a velocity curve for a list that is usually shorter than the
  window.
* **Making the handle undo the automatic fold too.** §2: it would reintroduce
  the overflow, on the smallest screens, in the name of an affordance.
* **A weighted targeted adjustment.** §3.
* **Putting the Target tool in Full only.** It would keep ADR 0054 §2's table
  intact and make the feature incoherent — §3.
