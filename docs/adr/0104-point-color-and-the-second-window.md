# ADR 0104 — Point Color, and the second window

**Status:** Accepted — 2026-08

## Context

Two questions the parity survey left explicitly unanswered, and which are
answered here so they stop being asked.

**Point Color** (Lightroom 12+): sample a colour from the photograph, get
hue / saturation / luminance sliders for *that* colour with a tolerance,
rather than for one of eight fixed bands.

**A second window** on another monitor, showing the grid or the loupe
while the main window develops.

## Decision

### 1. Point Color is refused as a feature, because two thirds of it
### already exist and the last third is a different question

Leyline can already do the Point Color gesture, and the survey missed it
because the pieces have other names:

* a **local adjustment** whose mask is `Mask::Everything` — the geometry
  of "no geometry", which the code comments already describe as the shape
  a range mask stands on alone;
* narrowed by a **colour range** (ADR 0048): a centre hue, a half-width
  and a softness;
* whose centre hue is set by **clicking the photograph** — the range
  eyedropper, delivered this month (ADR 0093);
* carrying `saturation` and `exposure`, i.e. two of Point Color's three
  sliders.

Verified by running the binary on 2026-08-31 rather than by reading the
code: `sample-range` returns the hue under a click, and a
`local-adjustment` payload combining `Mask::Everything`, a colour range
centred on that hue, and `saturation` + `exposure` commits and renders.
This ADR's central claim is therefore a measurement, not an argument —
the same discipline ADR 0095 applied when the survey was wrong about
duplicate detection.

Adding a module called Point Color would therefore be a **third way to
say what the HSL mixer and the range mask already say**, which is the
argument that refused Quick Develop in ADR 0101 §5 and it does not get
weaker for being about colour.

What is genuinely missing is the third slider: `LocalAdjustmentValues`
has no **hue**. That gap is real and it is *not* a parity item — it is an
asymmetry in Leyline's own model, since the `hsl` stage shifts hue
globally while a local adjustment cannot shift it at all. It is recorded
here as an open question about that struct's completeness, to be settled
on its own merits (a new `local_adjustments` version, a capability rule,
a golden case) by whoever wants it, and **not** as a way of shipping
Point Color under another name.

### 2. The second window is deferred, with its blocker named

Not refused: a second monitor showing the grid while the first develops
is a real working habit, and tethered capture (ADR 0087) makes it more
useful rather than less.

Deferred, because the blocker is structural and worth stating: Slint
supports several windows, but **every global in `ui/state/` assumes
exactly one** — `GridState.selected`, `DevelopState.develop-image`,
`DialogState.dialog` are singletons, and two windows reading them would
share one selection and one dialog. ADR 0045's modularisation helps by
grouping them, and does not solve it: the work is deciding which state is
per-window and which is per-library, then splitting it, which is the
whole cost of the feature.

Recorded as deferred rather than refused so that the day it is built, the
first question — *which state belongs to a window?* — is already on the
table.

## Consequences

* `docs/specification.md` §4 gains a Point Color row, phrased as what it
  is: refused because it already exists in parts.
* The parity survey has nothing left open.

## Rejected

* **Shipping Point Color as a named panel** — §1: a third vocabulary for
  a thing that has two.
* **Adding `hue` to `LocalAdjustmentValues` inside this decision** — it
  would arrive as an Adobe feature's shadow rather than as an answer to
  Leyline's own asymmetry, and a rendering change deserves to be argued
  for itself.
* **Refusing the second window** — §2: the habit is real, and the only
  honest obstacle is work, not principle.
