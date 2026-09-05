# ADR 0124 — The left column of Develop, and the reset that undoes everything

**Status:** Accepted — 2026-09

## Context

Develop is a three-column view: presets on the left, the photograph in the
middle, the settings on the right. [ADR 0058](0058-preset-provenance-and-shelf.md)
§1 settled *which* side the preset library goes on — the right panel is the
modification in progress, the left is the library of modifications — and
nothing has been put beside it since.

The result, on a real library, is a column that is empty. A photographer who
has not yet saved a preset sees one heading, one search box and a sentence
explaining that there are no presets, against a right-hand column carrying
sixteen foldable groups that scroll past the bottom of the window. The two
columns have the same width and opposite densities, and the emptier one is
the one the eye lands on first.

Two things already in Develop are in the wrong column, and
[ADR 0112](0112-four-panel-affordances.md) §4 said so without meaning to. When
every group gained a reset, three headers had to be excluded because they
are *not settings*: Soft Proof is a view, and **Versions and History are
lists**. A panel whose whole job is "the settings of this revision" has two
entries that no setting-shaped operation can touch.

And there is no way to say *put this photograph back*. §4 gave each group a
reset; putting a whole development back to neutral means finding sixteen
headers, opening each one, and clicking sixteen times — with a revision
written each time, so the History fills with the undoing.

## Decision

### 1. The left column is what one *chooses*, and where one is *looking*

Four sections, top to bottom: **Navigator**, **Presets**, **Versions**,
**History**. Each folds under its own header, like the right column's
sixteen; Navigator and Presets start open, the two lists start closed.

The ordering is the reading: where am I looking, what look can I put on
this, which branch of this photograph am I on, and how did it get here.

### 2. The Navigator shows the whole photograph, and where the viewport is

The image already rendered for the middle of the window, drawn small, with a
rectangle over the part the viewer is currently showing. Clicking inside it
moves the view there; dragging moves it continuously.

It earns its place because it answers a question no other part of the
interface can: at 100 % on a 45 Mpx file the middle of the window is showing
about 2 % of the frame, and nothing until now said *which* 2 %. Fitted, the
rectangle covers the whole thumbnail and the section is simply the
photograph — which is still the cheapest "what am I working on" there is,
and costs no render: it is the same `develop-image` the viewer holds, at a
different size.

**No zoom buttons under it.** [ADR 0112](0112-four-panel-affordances.md) §3 put
continuous zoom on the wheel and a percentage on the toolbar over the
photograph; a second set of controls in a second place would be a second
truth.

### 3. Versions and History move, and the reason is ADR 0112 §4

They are lists of *states*, not settings, which is exactly why the group
reset had to except them. What `reset_group` cannot touch does not belong
among the things it resets — and a history is *of* a version, so the two
travel together and in that order: pick the branch, then walk it.

Soft Proof stays on the right. It is excepted from the reset for a different
reason — it is a view **of the current settings**, and it belongs beside
them.

### 4. Reset All: one verb, one revision

`develop::reset_group` gains the group name `all`, whose changes are the
union of every other group's — the same function, called once per group and
concatenated, so a group added later is included by construction and cannot
be forgotten.

It writes **one revision**, not sixteen. That is the whole point: sixteen
resets leave sixteen entries in the History, and the photographer who wants
the photograph back does not want a record of how many groups it took.

**No confirmation dialog.** A revision is written, `Ctrl+Z` takes it back,
and the History section — now in the same window, two sections down — lists
the state it was in before. Asking "are you sure?" about something already
reversible teaches people to click through dialogs. This follows the rule
[ADR 0060](0060-asset-removal.md) draws: a confirmation is for what
cannot be undone.

It lives in the right column, with the `Auto` and `B&W` chips that also act
on the whole development, and in the `Develop` menu beside *Reprocess*.

### 5. What does not move

**Collections.** Lightroom's Develop module repeats its collections list in
this column; Leyline's collections live in the browser
([ADR 0055](0055-library-navigation.md) §2) and stay there. Develop is one
photograph — a list for choosing a different set of photographs is the
Library module's job, and `G` is one keystroke away.

**The histogram.** It stays at the top of the right column, against the
sliders that change it.

## Consequences

* `presets.slint` keeps the preset library as a section and stops being the
  column; a new `develop_left.slint` assembles the four sections. The column
  scrolls as one, so the preset list loses its own scrollbar — which is what
  makes four sections in one column possible at all.
* `DevelopPanel` passes the viewer's zoom state (zoomed, scale, centre, and
  the viewport's size) into the left column, and takes back a new centre.
  The same properties the toolbar and the image already share — the
  Navigator is another reader of them, never an owner.
* `reset_group("all")` is one more arm of an existing `match`; no new
  parameter, no new callback shape, no schema change, no stage version, and
  no pixel that was not already reachable one group at a time.

## Alternatives rejected

* **A confirmation on Reset All.** §4: it is undoable, and a dialog in front
  of an undoable action is training, not safety.
* **Widening the right column instead.** The imbalance is not width, it is
  that two of the right column's entries are not settings. Moving them fixes
  both columns; widening one fixes neither.
* **A Navigator that renders its own preview.** A second render of the same
  photograph at a second size, kept in step with the first — for a thumbnail.
  The viewer's own image scaled down is the same picture and costs nothing.
* **Snapshots as a separate list from Versions.** Lightroom has both because
  its Snapshots are named points in one linear history. Leyline's Versions
  ([ADR 0094](0094-versions-in-the-clients.md)) are branches, each with its own
  history — the stronger of the two ideas, and having both would mean
  explaining the difference.
