# ADR 0096 — Sharpening's fourth slider: masking, and the two that are refused

**Status:** Accepted — 2026-08

## Context

Lightroom's Detail panel gives sharpening four sliders — `Amount`,
`Radius`, `Detail`, `Masking`. Leyline has the first two. `Masking` is the
one that matters most in practice, and for a reason that is not about
taste: an unsharp mask amplifies *everything*, so on a portrait it
sharpens the eyelashes and the skin grain equally, and on a landscape the
branches and the sky's sensor noise equally. Every photographer who
sharpens learns to hold `Alt` and drag until the flat areas go black.

This is the first slice of this parity run that **changes rendering**, so
it is the first that costs a stage version, a golden case and an entry in
the capability rule. That price is the whole reason ADR 0042 exists, and
paying it is the decision.

## Decision

### 1. `sharpen::v2`, and `v1` is not touched

A new module beside the old one (ADR 0042 §1). `Sharpening::masking`
defaults to 0, and **at 0 v2 renders exactly what v1 renders** — the
edge mask is all-ones and the unsharp delta passes through unchanged.
That is what makes the version bump safe rather than merely legal: a
revision that never asked for masking sees the same pixels whichever
version it pins.

### 2. What the mask is

The magnitude of the luma gradient (a Sobel pair on the same luma plane
the unsharp mask already builds — no second plane, no second blur),
normalized, then thresholded by a smoothstep whose lower edge rises with
the slider. At `masking = 0` the threshold is below every gradient, so
everything passes; at 100, only strong edges do. The result multiplies
the unsharp delta per pixel.

The mask is blurred by the same radius as the unsharp mask before it
multiplies, so an edge keeps its halo instead of being cut at exactly the
pixel where the gradient falls off — a hard-edged mask is visible as a
contour, the same failure `LuminanceRange::softness` exists to prevent
(ADR 0048 §2).

### 3. The capability rule applies unweakened

A `masking > 0` on a revision pinning `sharpen` v1 is **refused by
`validate()`**, naming the stage, the version it needs and the
reprocessing that fixes it — never silently dropped. That is the same
refusal ADR 0048 §5 wrote for range masks and the rule
`stage-version-capability-rule` states generally: a slider that does
nothing is worse than an error, because nobody sees it.

### 4. `Detail` is refused, in writing

Lightroom's `Detail` slider blends between a plain unsharp mask and a
deconvolution-like kernel that recovers fine texture at the cost of
halos. It is a genuinely different operator wearing a slider's clothes,
its behaviour is under-documented, and its useful range is narrow enough
that most workflows leave it at its default forever. Refused now, and if
it comes back it comes back as its own decision with its own stage
version — not as a fifth parameter smuggled into this one.

The `Alt`-drag mask visualization is **also refused for now**, and
separately: it needs the engine to render the mask as an image, which is
the mask-overlay path of ADR 0071 pointed at a different mask. That is a
real slice, not a checkbox, and the slider is useful without it.

## Consequences

* `sharpen` gains version 2 at the same rank (190) and the same working
  space; the golden manifest gains cases and moves none — a new case
  (`detail_masking`), never an edit of `detail`, per the golden module's
  own rule.
* `docs/pipeline.md` §3.3's table gains the row, and its `settings_json`
  example gains the field.
* Studio gains a fourth slider, the CLI a third argument to
  `sharpening` — kept optional, so every script written against two
  arguments keeps working and means what it meant.

## Rejected

* **Amending `sharpen::v1`** — the frozen file is frozen; this is the
  case ADR 0042 §1 was written for, and the second time this run has
  declined to touch one (ADR 0091 restated the gains model rather than
  export it from a frozen module).
* **A separate `sharpen_mask` stage** — the mask has no meaning apart
  from the unsharp mask it multiplies, and a stage that only exists to
  modulate its neighbour is a parameter pretending to be a stage.
* **Making masking default to something non-zero** — it would change
  what every existing revision renders the moment it is reprocessed, to
  suit a taste the photographer did not express.
