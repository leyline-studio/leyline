# ADR 0116 — Defringe on a mask, and the two ways it is not the global one

**Status:** Accepted — 2026-09

## Context

[ADR 0108](0108-local-texture-clarity-sharpness-noise.md) put five
neighbourhood operators on a mask — clarity, texture, sharpness and the two
noise reductions — and refused two others in the same breath, with the order
written into the refusal:

> **Moiré and defringe**, which Lightroom also puts on a mask — Leyline has
> no such stage globally, so there is nothing to re-parameterize. A global
> stage first; then this question becomes trivial.

[ADR 0113](0113-defringe.md) built the global stage. This is the question
that became trivial, and the reason it still needs a decision is that
"trivial" is not "free": it costs a `local_adjustments` version, its
capability rule, and an honest answer about what a *local* defringe actually
is.

Why a photographer wants it on a mask rather than on the frame: fringing is
local by nature. It lives on the backlit branches at the top of a landscape
and nowhere else; the global slider that clears them also desaturates a
purple flower forty degrees away, which the three gates of ADR 0113 §4 make
survivable but not free. A mask is the tool that makes the trade disappear.

## Decision

### 1. Two amounts, the global pair, on the mask's values

`LocalAdjustmentValues` gains `defringe_purple` and `defringe_green`, both
`Option<i32>` in `[0, 100]`, exactly as the five of ADR 0108 are:

* the same fixed hue bands as [ADR 0113](0113-defringe.md) §2 — a movable
  band is still a control nobody has needed, and it would now have to be
  invented twice;
* the same operator, called rather than copied;
* the radius of the edge mask's dilation is the **stage version's constant**,
  scaled by the render scale, never read from the revision's global
  `defringe` — ADR 0108 §3's rule, for the reason it gave: a local slider
  whose meaning moved when a global one was touched would be unreasonable
  about.

### 2. It runs first among the local operators, because rank 22 says so

ADR 0108 §4 stated the rule the ten value-operators had been following
without anyone writing it down: **inside a local adjustment, operators run in
the order of their own ranks in the pipeline.** Defringe's rank is 22, ahead
of every other operator a local adjustment can run — exposure at 40, the tone
stages, the neighbourhood five from clarity at 90 to sharpen at 190.

So it runs first, before the per-pixel values rather than after them. That
breaks the *visual* grouping of `v4`'s code, where the operators that read
their neighbours sit together at the end, but that grouping was a description
and the rank rule is the decision. Following the rule keeps one sentence
governing the whole sequence instead of two.

### 3. Two ways this is **not** the global defringe, stated rather than discovered

* **It runs after the tone stages.** The global defringe is at rank 22
  precisely so a fringe is judged at the contrast the sensor recorded
  (ADR 0113 §1); a local adjustment is at rank 160, so its edge mask reads a
  contrast the tone curve has since stretched. Setting the local slider to
  the same number as the global one will not produce the same correction.
  This is not a defect to fix: it is what "on a mask" costs, and the same
  thing is already true of every one of ADR 0108's five.
* **The operator sees past the mask.** `v3` develops a full copy of the
  buffer and blends it back by coverage (ADR 0108 §2), so near a mask edge a
  pixel's new value was computed from neighbours the mask excludes. For a
  defringe that is *desirable* — the edge whose fringe is being removed
  usually straddles the mask boundary — but it is the same property, and it
  is named here for the same reason.

Both make one thing true: the local defringe is the same operator run
somewhere else, dosed by eye. It is not the global one restricted to a
region, and nothing in the interface will pretend it is.

### 4. `local_adjustments::v5`, and the capability rule for the sixth time

A new version beside the frozen `v4`, same rank, same working space. An
adjustment that sets neither amount renders **exactly** what `v4` renders —
each amount is guarded by its own `Option`, and the golden manifest records
the identity: every `v4` entry keeps a `v5` twin with the same digest.

A revision pinned below `local_adjustments: 5` that carries either amount is
**refused by `validate()`**, naming the version it needs and the reprocessing
that reaches it. That is the rule
`stage-version-capability-rule` states generally, applied for the sixth time,
and the reason has not changed: a slider that silently does nothing is worse
than an error, because nobody sees it.

## Consequences

* `local_adjustments` gains version 5; the golden manifest gains one entry
  per case that exercises a local adjustment, and moves none.
* The Studio mask editor gains two sliders, in the panel section ADR 0108's
  five already occupy; the CLI's `local` payload gains two optional fields.
* ADR 0108's refusal is now **half discharged**: defringe has its local form.
  Moiré is still refused, and for the reason that has not moved — Leyline has
  no moiré stage at all, globally or otherwise, and its detection is an
  analysis rather than a slider ([ADR 0113](0113-defringe.md) left that
  where it was).

## Alternatives rejected

* **Making the global stage read the local masks** — one defringe at rank 22,
  restricted to a region. It would keep the operator where the fringe is best
  judged, and it would make a stage read another stage's settings, which is
  the coupling the pipeline's ordering exists to prevent. ADR 0108 faced the
  identical choice for its five and answered the same way.
* **Reusing the revision's global `defringe` amounts as the local default.**
  Rejected by ADR 0108 §3's rule: the local value would then change meaning
  when the global slider moved, and a photographer could not reason about
  either.
* **Adding the hue-band controls at the same time**, since a masked defringe
  is where an unusual band would first be wanted. Rejected: that is a *new
  version of the global stage* (ADR 0113 §2) and it would have to be decided
  there first. Two sliders now, or four sliders in two places — the second is
  how a tool becomes unexplainable.
