# ADR 0058 — Presets: filed, legible before being applied, and traceable down to the photo

**Status:** Accepted — 2026-08

## Context

A develop preset in Leyline today: a name, a JSON of partial settings, a date.
A flat list, in a menu. One can create one, apply it, delete it — and that is
all. No folder, no favourite, no modification: a preset one wants to correct is
deleted and recreated.

Lightroom does better on filing: a left panel, folders, favourites, a preview
on hover. That is a legitimate expectation as soon as one goes past a dozen
presets, and there is no reason to do worse.

**But Lightroom has a hole, and it is structural.** Once the preset is applied,
*the link is lost*: nothing in the catalog says a photo was developed with
*Kodak Gold*. The consequence is a scenario every photographer knows and no
tool solves:

> I improved my preset after processing 340 photos from a wedding. Which ones?

In Leyline the answer is within reach for an architectural reason, not a
functional one: **applying a preset already produces an ordinary revision**
([ADR 0014](0014-develop-presets.md), `docs/presets.md` §2). All that is
missing is for the revision to say *where it comes from*.

## Decision

### 1. Presets have a home: a left panel in develop

[ADR 0054](0054-first-run-and-basic-mode.md) §2 took them out of the right
panel with a reason that still holds: **the right is the modification in
progress, not the library of modifications**. Their place is therefore on the
left, where [ADR 0055](0055-library-navigation.md) §2 has already put what one
*chooses* — folders, collections. Develop gains the same panel, carrying a
single thing: the presets.

It collapses with `Tab` like the others (ADR 0055 §6).

### 2. Filed: one level of folders, and favourites

Folders are **a table**, not a prefix in the name: they can be renamed, deleted
(their presets move up to the root, nothing is lost) and can be empty. A
favourite is a flag, and favourites are shown first.

**One level only.** Lightroom offers no more, nobody complains, and an
arbitrary depth here would file nothing more than it would complicate.

### 3. Legible before being applied

The panel says **what the preset changes**, in plain terms — "Exposure +0.35 ·
Contrast +12 · Temperature 5200 K" — and not merely its name.

That is possible because our settings are structured, and it is exactly what
Lightroom does not show: there, a preset is a closed box one understands only
by applying it and then undoing.

### 4. Triable without being applied

Hovering a preset shows the current photo **with** it, writing nothing. Three
safeguards, because a render is not free:

* the hover must last (a quarter of a second) before triggering anything —
  running down a list triggers nothing;
* **one render in flight** at a time: the next waits, and a result arriving for
  a preset no longer hovered is thrown away;
* it is a preview, never a revision. Leaving the hover returns the photo as it
  really is.

### 5. Traceable: the revision says which preset it comes from

A revision produced by applying a preset records **which one**, and **in which
version** of that preset.

**In the catalog, not in `settings_json`.** That is this decision's sensitive
point: `settings_json` is the *render* contract, and
[ADR 0043](0043-collapse-prerelease-render-history.md) showed what a foreign
field in that document costs. A provenance is not a pipeline input: two photos
with the same settings must render the same pixels, whether they come from a
preset or from twelve sliders moved by hand. It therefore lives in two nullable
columns of `develop_revisions`, which the render engine never reads.

### 6. A preset has a version, and can be updated

Today one can only create and delete. A preset becomes modifiable, and every
modification **increments a counter**.

From then on, two questions have an answer — the two Lightroom cannot ask:

* "which photos were developed with *Kodak Gold*?";
* "which of them were, with a version earlier than the current one?"

And the answer to "run it again over those" is **a batch of ordinary
revisions**: undoable one by one, visible in the history, like everything else.

### 7. Nothing updates itself

Modifying a preset touches **no** existing revision — that is already
`docs/presets.md` §2's rule, and this decision does not chip at it. Photos
already developed keep their pixels; the catalog can only say they were made
with an earlier version, and the user decides. A preset that changed a photo
without being asked would be the exact reverse of the project's promise.

### 8. The catalog migrates, it is not re-imported

Two columns and two tables more: it is **additive**, and the incremental
migration mechanism (`docs/catalog.md` §34) is made for that. Nothing like ADR
0043, which changed the shape of a *stored rendering* and therefore had nothing
sensible to migrate. Here an existing library opens and carries on.

## Out of scope

* **Presets shipped with the application.** `docs/vision.md` refuses to let the
  publisher impose a taste, and ADR 0054 §4 had already rejected it.
* **A preset's amount** (Lightroom's *Amount* slider). Interpolating a map of
  partial settings makes no sense for a boolean, a file path or a mask; one
  would have to decide *what* is dosed, which is a decision in its own right.
* **Importing Lightroom presets** (`.xmp`). It is the most obvious adoption
  lever in this whole document, and that is precisely why it cannot be handled
  in passing: translating Adobe's parameter names into ours is a compatibility
  promise, with its cases where the equivalent does not exist. Its own ADR.
* **Nested folders** (§2).
* **Sharing a preset between libraries**: the JSON format is already
  self-contained (`docs/presets.md` §2), but importing and exporting files is a
  distribution subject, not a filing one.

## Consequences

* **Implemented in Studio on 2026-08-31**, the panel last of all: the catalog
  half (`preset_folders`, favourites, versions, provenance) had shipped with
  the decision, and §1–§4 — the left column itself, folders and favourites,
  the summary line, the hover trial — had not. §4's three safeguards needed
  less machinery than the decision anticipated: the trial renders
  synchronously through `Library::preset_preview`, so "one render in flight"
  and "throw away a result for a preset no longer hovered" hold by
  construction, and only the quarter-second wait is code. Two Slint 1.13
  traps were paid for on the way: a `MenuItem` whose `title` is an
  expression, or a `Menu` subtree that reads a struct field of the enclosing
  `for` element, makes the Rust code generator panic outright; and the
  viewer's image has to be a *property* binding, since that is the form that
  re-runs when the trial flag flips.
* **A preset's amount is still out of scope**, as this decision's "Out of
  scope" says. Nothing about implementing the panel changed the argument:
  interpolating a map of partial settings has no meaning for a boolean, a
  file path or a mask.
* **The catalog moves to schema 2**: `preset_folders`, three columns on
  `develop_presets` (folder, favourite, version) and two on `develop_revisions`
  (the originating preset, that preset's version). `docs/catalog.md` is
  updated.
* **Develop gains a left panel**, which did not exist — and with it the place
  to put, later, what is *chosen* rather than what is adjusted.
* **The CLI gains two commands** so as not to fall behind Studio (ADR 0011):
  updating a preset from a photo, and running it again over the photos that
  carry an earlier version of it.
* **No pixel changes.** No stage, no stage version, and not a line of
  `settings_json`: the reproducibility promise is intact, and that was the
  condition that made this decision acceptable.

## Alternatives rejected

* **Putting the provenance in `settings_json`.** The render's document must stay
  the render's document (§5). One more field, and one is back in exactly the
  situation ADR 0043 had to clean up.
* **A `revision ↔ preset` link table.** More normalized, but a revision is
  *one* commit: applying two presets makes two revisions. Two columns say the
  same thing without a join.
* **Automatically re-applying a modified preset** to all its photos ("dynamic
  presets"). Appealing and wrong: yesterday's revision would become different
  today without anyone asking (§7).
* **Filing by a prefix in the name** (`Film / Kodak Gold`), instead of a folder
  table. Free to write, and renaming a folder becomes a rewriting of N names,
  the order depends on the punctuation, and an empty folder does not exist.
* **A hover preview with no delay and no queue.** A list of fifty presets run
  through with the cursor would launch fifty renders (§4).
