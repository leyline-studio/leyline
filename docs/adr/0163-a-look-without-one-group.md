# ADR 0163 — A look without one group

**Status:** Accepted — 2026-10

## Context

[ADR 0112](0112-four-panel-affordances.md) refused a checkbox per tool —
disable *Curves* without zeroing it — and named the better answer without
building it: « a *preview* that renders without one stage, which is not
stored at all and is much closer to what a photographer actually wants ».
The Phocus parity report lists it as the third gap still open: their tool
headers carry that checkbox, and a photographer coming from it reaches for
it to ask *what is this tool doing to my picture?*

Two things exist already and make the answer small:

* **`reset_group`** ([ADR 0112](0112-four-panel-affordances.md) §4,
  [ADR 0124](0124-develop-left-column.md) §4) maps each of the develop panel's
  headers to the parameters it owns and to their neutral values. It is what
  the ⟲ glyph on a header writes, in one revision.
* **`Library::preview_live`** renders a set of settings without committing
  anything and without touching the preview cache — the « a view, never a
  revision » path the soft proof, the profile browser and the preset trial
  ([ADR 0058](0058-preset-provenance-and-shelf.md) §4) already use; the
  trial already has a way to put such a render in the viewer and take it
  away again.

The word *stage* in ADR 0112 is not quite the right unit. A header of the
panel is not a stage: *Basic* spans white balance, exposure, the tone
operators, dehaze and saturation, which are several stages; *Local* is a
stage per mask. What the photographer turns off in their head is a header,
so that is what this ADR turns off.

## Decision

### 1. The look is the reset, uncommitted

Looking at the photograph without a group renders the current settings with
**exactly the changes the group's reset would write**
(`develop::reset_group`), through `preview_live`. Nothing else is invented:
there is no « disabled » state to define per stage, and the question *what
does « without Curves » mean for the curve's stage version?* has the answer
the reset already gives. It follows that the look and the ⟲ glyph next to it
always agree: what you see while holding is what one click on ⟲ would
commit.

A group already at neutral changes nothing, and the look shows the
photograph unchanged — which is itself the answer to the question asked.

### 2. Hold to look, release to come back

A glyph `◐` on every header that has a reset, beside ⟲ and shown under the
same rule (only while the group is open). **Pressing** it shows the look;
**releasing** it, or the pointer leaving the window, shows the photograph
again. A toggle was rejected: a look that stays on after the hand has left
it is a state, and a state on screen that is not in the settings is the
confusion ADR 0112 refused the checkbox for. The before/after comparison remains
the tool for the whole history; this one answers for one group.

### 3. Stored nowhere, written nowhere

No `settings_json` field, no `enabled` flag, no migration, no engine
surface, no new stage version. No revision is written and no preview file
is created or invalidated. Presets, reprocessing, exports and the golden
cases never see it, because it is not anything they could see.

### 4. It shares the trial's place in the viewer

The look is shown through the same viewer image as the preset trial: one
uncommitted render on screen at a time, cleared by the same path. A render
that fails shows nothing and reports nothing — a held button is not a
command whose failure deserves a dialog — and the photograph stays as it
was.

## Consequences

* One glyph per header, one callback, one Rust handler of a dozen lines; the
  mapping stays the one `reset_group` already owns, so a group added there
  gets its look by construction.
* The look costs one `Small` render per press, uncached. On a large RAW that
  is the same latency as a slider release.
* *Basic* turns off as a whole. A photographer who wants *without the
  exposure only* moves the exposure slider; splitting the look finer than
  the reset is what a checkbox per parameter would be, and that is refused
  below.

## Rejected

* **An `enabled` flag per tool in `settings_json`** — refused by ADR 0112
  for the reasons it gives, and not reopened.
* **Skipping a stage inside the engine** (`render` without stage *n*).
  Groups are not stages (see Context), and a bypass would be a second
  meaning of « neutral » next to the one every stage already has.
* **A toggle that stays on** — §2.
* **A keyboard shortcut** for now: which group would it act on? The open
  one is not a single group. Named, not built.
