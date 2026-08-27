# ADR 0048 — Range masks: a deterministic luminance and colour refinement, on top of the geometric masks

**Status:** Accepted — 2026-07
**Followed by:** `local_adjustments::v2`, which this ADR creates, is no longer
the current version: [ADR 0070](0070-stored-mask-coverage.md) adds **stored**
coverage (`v3`). The luminance and colour ranges decided here apply unchanged
to that new kind of mask, which is sampled on the same normalized canvas as the
four existing variants.

## Context

[ADR 0029](0029-process-6-local-adjustments.md) delivered three masks: brush,
radial, graduated. All three are **pure geometry** —
`crates/leyline-engine/src/mask.rs` says so in its first paragraph, and its
rasterization function receives no pixels at all, only a size and an angle. The
photographer therefore designates *where*, never *what*.

That is half the gesture. Darkening a sky means designating the sky, not the
top of the frame: a graduated filter bites into the mountain, a brush into
every tree branch that sticks out. Competitors solve exactly this case, and it
is their most visible selling point:

* **DxO** with its U Point masks, presented as "AI masks" but which are, under
  the marketing, a selection by **colour and luminance**;
* **Lightroom** with its *Range Mask* (luminance range, colour range), which
  comes to **refine** an existing local mask rather than replace it;
* **Capture One** with its combined masks.

Subject or sky selection by neural network (Capture One *People Masking*,
Luminar *Sky AI*) falls, for its part, under `docs/specification.md` §4's AI
exclusion: it is not aimed at here and the present ADR does not reopen it. What
is aimed at is precisely the part competitors' marketing calls AI without its
being any: a thresholding over quantities the pixel already carries.

**What is not at issue.** ADR 0029's composition model
(`output = lerp(buffer, local_operators(...), coverage)`), the coordinate frame
of [ADR 0026](0026-mask-spot-coordinate-referential.md), and the set of
settings a mask can re-parameterize. Only the **provenance of the coverage**
changes.

## Decision

### 1. A refinement, not a fourth mask

A range does not replace a mask: it **multiplies** it.

```
coverage = geometry(x, y) × luminance_range(pixel) × colour_range(pixel)
```

That is Lightroom's model, and it is more expressive than a standalone mask's
for a concrete reason: the real gesture is "I brush roughly, then restrict to
the sky's blue". A standalone range mask could not express that; a refinement
expresses both, the standalone case being the refinement of a geometry covering
everything.

Concretely, `LocalAdjustment` gains an optional field:

```rust
pub struct LocalAdjustment {
    pub mask: Mask,
    pub range: Option<RangeMask>,   // new, None = ADR 0029's behaviour
    pub opacity: f64,
    pub adjustments: LocalAdjustmentValues,
}
```

and `Mask` gains an `Everything` variant — full coverage, two lines in the
rasterizer — so that a range can do without geometry rather than our having to
divert a giant radial or a degenerate graduated filter.

### 2. Two terms, both optional and both soft-edged

```rust
pub struct RangeMask {
    /// A luminance band on the display axis, `None` = no term.
    pub luminance: Option<LuminanceRange>,
    /// A hue band, `None` = no term.
    pub color: Option<ColorRange>,
}
```

* **Luminance**: `min`, `max` in `[0, 1]`, plus `softness`. Full coverage
  between `min` and `max`, with a smoothed falloff (`smoothstep`) over a width
  of `softness` on either side. A hard edge would produce a visible contour as
  soon as noise made a pixel oscillate around the threshold — that is the
  parameter's reason for being, not an ornament.
* **Colour**: `center` (a hue in degrees), `width` (a half-width in degrees),
  `softness`. Hue is circular, so the distance is taken modulo 360.

A pixel **with no chroma has no hue**: a grey is neither red nor blue, and
assigning it one by convention would bring every grey into any colour band. The
colour term therefore weights by the pixel's saturation, so that a grey
receives zero coverage. That is what makes "restrict to the sky's blue" usable
without also selecting the clouds.

### 3. The axis: the display's

Both terms are evaluated on the display axis (`kernel::v1::display`, ADR 0044),
not in linear light. "Luminance 0.3 to 0.7" must designate what the user sees
on their histogram, and an equivalent linear interval would place its lower
bound in absolute black. The same reasoning as for the tonal operators.

The pixel evaluated is the one **in the buffer as it arrives at the stage** —
hence after every global operator, like the rest of ADR 0029. A range is tuned
by looking at the image as it is on screen, which is also the only definition a
user can predict.

### 4. The range's code lives in a stage version, not in `mask.rs`

`mask.rs` is shared, unversioned, and explicitly justifies that: it "encodes no
pixel transformation formula". A range **is** a formula over pixels. Putting it
there would make `local_adjustments::v1`'s frozen rendering depend on modifiable
code, which ADR 0042 §1 forbids.

Therefore:

* `mask.rs` keeps geometry alone, including `Mask::Everything` (which is
  geometry);
* the range's computation and its composition with the geometry live in
  **`local_adjustments::v2`**, frozen like any stage version;
* `local_adjustments::v1` is untouched and goes on rendering what it rendered.

### 5. A range on a revision pinned at `v1` is **refused**, not ignored

The trap in this shape: a 2026 revision pins `local_adjustments: 1`; a user
adds a range to it in 2027; ADR 0042 §2's pinning rule says the stage **keeps**
its version, so `v1` would render — and `v1` knows nothing of ranges. The
setting would disappear without a word.

That is unacceptable, and the answer is not to loosen the pinning:
`Settings::validate()` **refuses** a non-empty `range` when the `stages` map
pins `local_adjustments` at version 1. The message names the remedy, which is
the project's: reprocess the photo towards the current versions
(`docs/pipeline.md` §4.5), which creates a new revision pinned at `v2`.

An explicit error where silence was possible: it is the same rule
`MixedWorkingSpaces` already applies to an incoherent plan (ADR 0044 §4), and
the first case where a *capability* — not a rendering — proves tied to a stage
version. The general rule that emerges, and that will hold for any future
feature added to an existing stage: **a setting a pinned version cannot express
is a validation refusal, never a lost value.**

### 6. Out of scope

* **Subject, sky or face detection.** The AI exclusion
  (`docs/specification.md` §4), unchanged.
* **Combining several geometric masks** (union, intersection, subtraction —
  Capture One's *Combined Masks*). Useful, independent, and it would require
  changing `Mask`'s shape rather than extending it: its own ADR.
* **Depth range.** There is no depth map to read.
* **Exposure in Studio and the CLI.** ADR 0029's local adjustments themselves
  are not yet exposed there — neither brush, nor radial, nor graduated — so
  ranges arrive at the same level as what they refine: engine, SDK and edit
  sessions (`Param::LocalAdjustment`). Exposing masks to the clients is a piece
  of work in its own right, and the gap predates this decision.

## Consequences

* **The "darken this sky" gesture becomes feasible** without biting into the
  mountain, with the same vocabulary as the competitors (luminance range,
  colour range) and without borrowing their AI marketing.
* **`settings_json` gains an optional field whose neutral value is absence**,
  which is explicitly compatible in `docs/pipeline.md` §3.4's sense: `schema`
  is not incremented.
* **The project's second real stage version**, after ADR 0046. ADR 0042's
  mechanism now serves twice, and here serves for a different reason — not
  fixing a rendering but **extending** an operator — which exercises the
  pinning rule from an angle nothing had yet tested (§5).
* **The cost is proportional to the refinement asked for**: with no `range`,
  nothing changes in pixels or in time; with one, an extra pass over the
  covered pixels.
* **`mask.rs` keeps its invariant** — pure geometry, shareable by every version
  — which was the reason not to touch it.

## Alternatives rejected

* **A fourth, standalone `Mask` variant.** Less expressive (no refinement of a
  brush), and it would still place a pixel formula in the shared rasterizer,
  hence in `mask.rs` — §4's problem without §1's benefit.
* **Extending `mask.rs` with the ranges.** Rejected in §4: `v1`'s frozen
  rendering would depend on code nothing prevents from being modified.
* **Silently ignoring a `range` on a `v1` revision.** Rejected in §5. It is the
  behaviour one would have got without thinking, and the worst: the user sees
  their slider do nothing.
* **Making `local_adjustments::v1` able to read ranges** (hence modifying a
  frozen module). Forbidden by ADR 0042 §1, and unnecessary: `v2` costs a few
  dozen lines.
* **A selection by colour proximity to the clicked point**, in the manner of
  DxO's control points. That is an interface on top of the same mechanics, not
  different mechanics: it computes a `center`/`width` from the designated
  pixel. To be done when masks have an interface, with no new engine decision.
