# ADR 0113 — Defringe: the colour an edge is not

**Status:** Accepted — 2026-09

## Context

[ADR 0111](0111-adaptive-chromatic-aberration.md) closed *lateral* chromatic
aberration: red and blue magnified differently by the lens, a geometric error,
fixed by resampling each channel from where its content actually is.

The other half of the family is not geometric and no warp can touch it.
**Axial** chromatic aberration — the lens focusing different wavelengths at
different distances — and the blooming of a badly overexposed edge both leave
a *coloured halo* beside high-contrast transitions: purple in front of the
focal plane, green behind it. A branch against a bright sky, a chrome rim, a
backlit hair: the fringe sits next to the edge, in colour the subject never
had.

Every serious RAW application has a *Defringe* tool for it, and
[ADR 0108](0108-local-texture-clarity-sharpness-noise.md) already refused to
build the *local* version of one, with the order written into the refusal:

> **Moiré and defringe** […] Leyline has no such stage globally, so there is
> nothing to re-parameterize. A global stage first; then this question becomes
> trivial.

This is that global stage. The local one is still not this ADR.

## Decision

### 1. A new stage, `defringe`, at rank 22

A **new stage** and not a version of an existing one — so `pipeline.md` §5.1
is untouched for every revision already written and the golden manifest does
not move a digit, which is [ADR 0103](0103-red-eye-correction.md)'s lesson
measured rather than assumed. It also needs no capability rule: an old
revision simply has no `defringe` entry, which is what an inactive stage
looks like.

**Rank 22 — after `lens` (20), before everything that moves or paints
pixels.** Three reasons, in order of weight:

* A fringe is a **defect of the capture**, like the lateral aberration
  corrected two ranks earlier and the noise corrected before that. It belongs
  with them, not among the choices a photographer makes.
* Correcting it before `spot_removal` (30) means a clone copies pixels that
  are already clean — a spot that samples a fringed edge would otherwise
  paint the fringe somewhere new.
* Correcting it before the tonal stages means the fringe is judged at the
  contrast the sensor recorded, not at the contrast a curve has since
  stretched.

Rank 25 is left alone: [ADR 0109](0109-reshape-stage.md) reserved it for
`reshape`, which is decided and not yet built, and stepping on a reserved
rank to save three digits would be a poor trade.

### 2. Two amounts, and the bands are fixed

```rust
pub struct Defringe {
    pub purple: i32,   // 0..100
    pub green: i32,    // 0..100
}
```

Neutral is `0, 0`, and the stage does not run there.

Two **amounts** and no hue ranges. Lightroom offers a movable band per colour;
this ADR fixes both bands — magenta-violet around 285°, green around 120°,
each with a soft falloff — because a band is a control that only earns its
place the day the default band is wrong, and nothing yet says it is. If that
day comes it is a **new version of this stage**, not a third and fourth
slider smuggled into it.

### 3. What it does, exactly

Inside the display-encoded axis (`in_display`, the same axis
[ADR 0031](0031-hsl-color-grading.md)'s mixer works on, so that "hue" and
"saturation" mean here what they mean there):

1. the magnitude of the luma gradient, **blurred**, is the mask — a fringe
   sits *beside* an edge, not on it, so a one-pixel-wide edge mask would
   correct everything except the halo it exists for;
2. a pixel's hue decides how much of each band it belongs to, with a
   smoothstep falloff so a hue crossing a band boundary does not step;
3. the pixel's **saturation** is reduced by `amount × band × edge`, its
   **hue and its lightness untouched** — lightness meaning HSL's
   `(max + min) / 2` **on that display axis**, the same quantity and the same
   axis the mixer preserves. Not linear luminance: the two differ, because
   the transfer function between the axes is not linear, and claiming the
   stronger of the two would be claiming something the operator does not do.

Desaturation, not hue rotation and not a copy from a neighbour: the fringe's
error is that the colour is *there at all*, and the honest correction is to
take it out, not to invent a replacement. It is also what makes the operator
safe at 100 %: the worst it can do is leave an edge grey.

The blur radius follows the render's `scale`, like `sharpen`'s radius does:
the same recipe has to look the same on a proxy and on an export.

### 4. What protects an actually-purple photograph

Three gates, and the ADR states them because each one is a way the tool could
have been wrong:

* **The edge mask.** A purple flower on a flat background has no gradient, so
  no correction. A purple flower's own petal edges do — hence the second gate.
* **The amount.** Fixed at 0 by default; nothing happens to anybody's photo
  until they ask.
* **Saturation only.** A subject that *is* purple, at an edge, at amount 100,
  loses saturation and keeps its hue and its lightness — a visible cost, not a
  destroyed picture, and one the photographer sees immediately at the slider
  they just moved.

There is no fourth gate that could be added without inventing a subject
detector, which is a different ADR and a different licence
([ADR 0102](0102-paid-extensions-and-the-pixel-boundary.md)).

### 5. Where it lives for the clients, and in a preset

The two sliders sit in Studio's **Lens correction** group, under
ADR 0111's chromatic aberration pair: lateral and axial aberration are one
family, and putting them side by side is what makes the pair legible. The
group's reset ([ADR 0112](0112-four-panel-affordances.md) §4) therefore takes
defringe with it.

For the same reason `SettingsGroup::LensCorrection` — the preset category —
captures `defringe` alongside `lens_correction`. The category is *what the
lens did to this photograph*, not *the `LensCorrection` struct*, and a preset
that carried one and dropped the other would be a preset that silently means
something else.

## Consequences

* One new stage at rank 22; the golden manifest gains a `defringe` case and
  **moves nothing**, because no existing case activates the stage.
* `docs/pipeline.md` §3.1's order and §3.3's table gain a row, and the
  settings table a field.
* Three clients: `leyline develop <v> defringe <purple> <green>`, two sliders
  in Studio, `Defringe` through the SDK façade.
* Presets in the *Lens correction* category start carrying two more numbers;
  a preset written before this ADR simply does not mention them, and applying
  it leaves defringe alone — the rule `PresetSettings` has always had.
* ADR 0108's refusal is half-answered: the global stage now exists, so the
  local version has something to re-parameterize. It is still a decision to
  take, and taking it means a new `local_adjustments` version and its
  capability rule.

## Alternatives rejected

* **A version of the `lens` stage.** Tempting, since it is the same family and
  the same panel group. Rejected: `lens` reads a Lensfun profile and resamples
  geometry; defringe reads hue and touches no coordinate. Folding them would
  force every future defringe change to re-freeze a geometric correction, and
  ADR 0103 measured what a new stage costs the goldens — nothing.
* **Movable hue ranges now.** Rejected in §2: an unproven control on a tool
  nobody has used yet. A new stage version can add them the day a photograph
  demands it.
* **Replacing the fringe colour instead of desaturating it** (copying chroma
  from just outside the halo, as some implementations do). Rejected: it
  invents colour where the correction should only remove it, it needs a second
  radius nobody would know how to set, and it fails exactly where fringes are
  worst — a thin bright edge, where "just outside the halo" is the other side
  of the subject.
* **Running before the noise stages** (ranks 5–6), so the denoiser sees
  corrected chroma. Rejected: those stages are driven by a **measured sensor
  profile** ([ADR 0072](0072-measured-noise-profile.md)) and expect the
  counts the sensor produced; desaturating edges first would put a
  photographic decision inside a physical model.
