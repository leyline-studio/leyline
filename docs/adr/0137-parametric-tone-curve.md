# ADR 0137 — The tone curve one drags by its regions

**Status:** Accepted — 2026-09

## Context

Studio's tone curve is a **point curve**: control points placed by hand,
per channel since [ADR 0098](0098-per-channel-tone-curves.md). It is the
precise instrument, and it asks the photographer to know where on the axis a
tone lives before they can move it.

Lightroom has had a second one since version 1, and it is the one most people
actually use: the **parametric** curve — four sliders named *Shadows*,
*Darks*, *Lights*, *Highlights*, and, along the bottom of the graph, **three
vertical split markers** that are dragged to say where each region begins and
ends. One does not place a point; one says *lift the darks a little*, and then
adjusts what counts as dark.

That is what is missing, and the asymmetry is worth stating plainly: our
Color Grading panel already carries a *balance* and a *blending* — the same
question of where the tonal zones sit — as two numbered sliders. Lightroom's
Color Grading does the same, with sliders and no graph. So a graph of
draggable dividers exists in exactly one place in the reference, and it is the
tone curve.

## Decision

### 1. Four regions, three splits, strictly ordered

`ParametricCurve` carries seven numbers:

* `shadows`, `darks`, `lights`, `highlights` — each in `[-100, 100]`, neutral
  at 0;
* `shadow_split`, `midtone_split`, `highlight_split` — each in `[1, 99]`,
  neutral at 25 / 50 / 75.

The splits are **strictly increasing**, and `validate` refuses anything else.
Lightroom lets two splits touch; refusing it costs a photographer nothing —
a region of zero width has no slider that can reach it — and it buys the
control points of §3 their strictly increasing `x` for free, which is the
condition the interpolant already requires.

### 2. It is a stage of its own, at rank 75

A new stage `parametric_curve`, ranked just **before** `tone_curve` (80), so
the point curve is applied on top of the parametric one.

That order is a decision and not a copy: the parametric curve is the *broad*
instrument, expressed in regions, and the point curve is the *precise* one,
expressed in points. The precise instrument gets the last word, because a
photographer who places a point expects the tone to land where they put it,
and would not accept a region slider moving it afterwards.

A **stage**, not a new version of `tone_curve`, for the reason
[ADR 0103](0103-red-eye-correction.md) established when it added red-eye at
rank 35: a stage absent from an existing revision's map falls back to its
pinned version, and its `active` predicate is false while every region is 0 —
so every revision written before today renders byte for byte what it rendered
before. A new *version* of `tone_curve` would instead have made the seven new
fields inexpressible on any revision pinned to v1 or v2, and the capability
rule would have turned that into a refusal for no reason.

### 3. Six control points through the interpolant that is already frozen

The load-bearing decision, and the one that makes this cheap.

A tone curve that is not monotone solarises, and no combination of four
sliders should be able to ask for that. Rather than invent a shape and then
defend its monotonicity, the parametric curve is expressed as **six control
points fed to `build_curve_lut`** — the monotone cubic Hermite
(Fritsch–Carlson) interpolant `tone_curve::v1` has used since ADR 0024, which
is monotone by construction on monotone data:

| point | x | y |
| :--- | :--- | :--- |
| 0 | `0` | `0` |
| 1 | centre of the shadows region | `x + shadows · 0.25` |
| 2 | centre of the darks region | `x + darks · 0.25` |
| 3 | centre of the lights region | `x + lights · 0.25` |
| 4 | centre of the highlights region | `x + highlights · 0.25` |
| 5 | `1` | `1` |

A slider lifts the **centre** of its region, not its boundary — which is what
makes the sliders and the splits two different controls rather than two names
for one. Dragging a split moves the centres either side of it, so widening the
shadows moves where the shadow slider pulls.

The `y` values are clamped into `[0, 1]` and then forced non-decreasing in one
forward pass, so the data handed to the interpolant is monotone and the curve
therefore is. Both ends stay pinned: black is black and white is white, and no
region slider can lift the black point — that is what *Blacks* is for.

Nothing new is interpolated, sampled or tabulated. The stage builds six points
and calls code that has rendered pixels since process 6.

### 4. The graph, and what dragging it means

In the Tone Curve group, a `Point` / `Region` pair of chips chooses the mode —
the same shape as the `Master` / `R` / `G` / `B` row already there, which now
belongs to the point curve alone.

`Region` draws the curve the six points describe, over the same square the
point curve uses, plus:

* **three vertical splits**, drawn from the axis, each draggable left and
  right and each stopped by its neighbours;
* the four regions faintly shaded, so the split one is dragging is visibly
  the boundary between two named things;
* the four sliders beneath, as ordinary `EditSlider`s.

Rust sends the curve as a polyline, the way the keystone guides already
arrive ([ADR 0119](0119-guided-keystone.md)): what is drawn is then computed
by the same function that renders, and the two cannot drift.

A split drag commits on release, like every other drag
([ADR 0074](0074-live-preview-while-dragging.md) §1), and previews while it
moves.

### 5. One category, not two

`SettingsGroup::ToneCurve` ([ADR 0132](0132-selective-copy-and-full-coverage.md)
§1) carries both curves. They are one block of the panel and one idea — *the
tone curve of this photograph* — and a photographer copying it means both.

That keeps the category count at seventeen and the coverage test honest: the
new field is claimed by an existing category, which is what §7 of that ADR
demands of every new field.

### 6. Out of scope

* **Per-channel parametric curves.** Lightroom has none either, and the
  point curve already answers a per-channel question ([ADR 0098](0098-per-channel-tone-curves.md)).
* **Dragging the curve itself in Region mode**, as Lightroom allows — drag on
  the curve and the nearest region slider follows. It is [ADR 0130](0130-direct-manipulation.md)
  §3's Target tool applied to a second panel, and it should be that or nothing,
  rather than a second gesture with its own rules.
* **A graph for Color Grading's balance and blending.** The same idea, a
  different panel, and the reference draws no graph there either. Recorded
  because it is the obvious next question: `zone_weights` is exactly as
  drawable as this curve, and the day it is wanted the pattern is here.

## Consequences

* `Settings` gains one field and `pipeline.md` §3.2 seven keys. Every existing
  revision is unaffected — the stage is inactive at neutral and absent from
  their maps.
* The golden corpus gains a case. Existing cases must not move: §2's fallback
  is the claim, and a golden run is the proof rather than the argument.
* `docs/presets.md` §3.1's `tone_curve` row gains the second field. Presets
  written before today carry only the point curve, and an absent field is
  "not included" as always.

## Alternatives rejected

* **A new version of `tone_curve`.** §2. It would refuse the new fields on
  every existing revision, for no gain.
* **Displacing the curve additively with smoothstep bumps**, as
  `color_grading::v1` weights its zones. It is the natural first idea and it
  is not monotone: two opposed sliders make a curve that goes back on itself,
  and the fix would have been a clamp invented here rather than an interpolant
  already trusted.
* **Control points on the splits instead of the region centres.** Then a
  slider would lift a boundary, and moving a split would move what the
  neighbouring slider had done — two controls fighting over one number.
* **Making the splits a stage parameter of `color_grading` too**, so both
  panels share one notion of where the tonal zones are. Tempting and wrong:
  the tone curve's regions are about *tones*, the grading's zones are about
  *where a colour is mixed in*, and a photographer who moves one has said
  nothing about the other.
