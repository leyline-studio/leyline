# ADR 0089 — The profile browser: choosing a rendering by looking at it

**Status:** Accepted — 2026-08

## Context

Leyline can reference one DCP camera profile per revision (ADR 0035,
`camera_profile` stage). Choosing one, in Studio, is a file dialog: `Choose
.dcp…`, a native picker, a filename. To compare two profiles a photographer
has to import one, look, import the other, look again, and remember.

Lightroom's Profile Browser answers this by showing, for each profile, **the
photo currently open rendered through it** — a grid of thumbnails, grouped,
starrable, with an `Amount` slider and `All / Color / B&W` tabs.

Three of its parts are ours to take and three are not, and the split is worth
writing down rather than rediscovering:

* **The grid of real renders** is the whole point, and Leyline has every piece
  of it already: `camera_profiles()` lists what the library holds,
  `preview_live` renders a photo through settings that were never committed.
* **The groups** (`Adobe Raw`, `Camera Matching`) exist because Adobe ships
  profiles. We do not, and will not — see §2.
* **The `Amount` slider** dials a profile's strength. That is a rendering
  change, not an interface one.

## Decision

### 1. The browser shows this photo, not a stock swatch

Each entry renders the **open photo** through that profile, at thumbnail
size, through `preview_live` — the same "a view, never a revision" path the
soft proof and the preset trial already use (ADR 0058 §4). Nothing is
committed until an entry is clicked.

A stock reference image would have been cheaper and is the wrong answer: what
a profile does depends on the sensor that shot the file and on the light it
was shot in, so a swatch would be a picture of some other camera's answer.

The renders are done once, when the browser opens, and kept for as long as
the same photo stays open. A profile library is a handful of files, not a
catalog: paying for the renders up front and keeping them is simpler than a
queue, and the whole grid is then instant to re-open.

### 2. Leyline ships no profiles

`docs/vision.md` refuses to let the publisher impose a taste, and ADR 0054 §4
already rejected bundling develop presets for that reason. A profile is the
same argument one layer down — it decides what the photo's colours *are* —
so the browser lists exactly what the photographer imported, and nothing
else.

That is why there are no groups and no `All / Color / B&W` tabs: those exist
to organise a shipped catalogue. Ours has one section, and its first entry is
**"No profile — the decoder's own colours"**, which is a real choice and
belongs in the grid rather than being a state one reaches by removing
something.

### 3. Profile amount is out of scope, and this is why

Lightroom's `Amount` interpolates between the profile's rendering and the
neutral one. In our terms that is a new `camera_profile` stage version, with
a strength field in `CameraProfile` — a pipeline change, a pinned version, a
golden entry.

It is refused **now** rather than never, and for a specific reason: the
matrix path is still marked experimental (`docs/pipeline.md`, the panel says
so where the user chooses), because its colours have never been validated
against reference renders. Adding a dial that interpolates toward an
unvalidated rendering would be building a control on top of a number nobody
has checked. Validation first, the dial after.

### 4. Nothing about the pipeline changes

No stage, no stage version, no field. The browser is a way of *choosing*
what already exists, and the only new engine surface it needs is none:
`camera_profiles()` and `preview_live` both shipped with earlier decisions.

## Consequences

* Studio's Camera profile group becomes a grid: the profiles the library
  holds, each showing the open photo, plus `Import .dcp…` where the old
  button was.
* The experimental warning stays exactly where it is. A browser that makes
  choosing a profile easy makes it *more* important to say the colours are
  not validated yet, not less.
* A library with no imported profile shows one entry — the decoder's own
  colours — and the import button. That is a complete and honest browser,
  not an empty state.
* `leyline-cli` already has what it needs (`develop <lib> <v> camera-profile
  <path>`); nothing is added, because a grid of thumbnails is not a thing a
  terminal does.

## Alternatives rejected

* **A dropdown instead of a grid.** It is what the Basic panel shows in
  Lightroom, and it is the half of the feature that does not answer the
  question: a list of filenames is what we already have behind a file dialog.
* **Rendering the thumbnails lazily, as they scroll into view.** Correct for
  a photo grid, over-engineered for a list whose length is the number of
  files a person has imported by hand.
* **Bundling the DCPs camera makers publish.** Redistribution terms vary per
  manufacturer, and it would put the publisher's taste in the default —
  twice over.
