# ADR 0108 — Texture, clarity, sharpness and noise on a mask

**Status:** Accepted — 2026-09

## Context

`LocalAdjustmentValues` ([ADR 0029](0029-local-adjustments.md), extended by
[ADR 0048](0048-range-masks.md) and [ADR 0070](0070-stored-mask-coverage.md))
carries ten values: temperature, tint, exposure, contrast, highlights,
shadows, whites, blacks, vibrance, saturation.

All ten are **per-pixel functions**, and that is not a coincidence — it is
what made ADR 0029's composition work. Develop a full copy through the
re-parameterized global operator, blend it back by coverage: exact, and
cheap enough that nobody had to think about it.

The operators that are missing are exactly the ones with a **neighbourhood**:
`clarity` (rank 90), `texture` (100), the two noise stages (170, 180) and
`sharpen` (190). They exist globally and nowhere else. What a photographer
therefore cannot ask for:

* clarity on a sky and not on a face;
* noise reduction in the shadows, where the noise actually is, instead of
  across a whole frame that is mostly clean;
* sharpening the subject without sharpening the bokeh it sits in;
* **negative texture** on a chosen area — a softening this develop module
  cannot express at all today, at any strength, anywhere.

`LIGHTROOM-PARITY.md` declared Develop closed and **missed this**. Lightroom
Classic has had Texture, Clarity, Sharpness and Noise on its masks for
years; the report does not contain the words. That is worth recording,
because a blind spot in a six-filter audit is worth as much as its
findings: twenty items were checked one by one, and not one filter asked
*"which global sliders are absent from the local panel?"* — the single
question that finds this in a minute.

## Decision

### 1. `local_adjustments::v4`, and the shape does not change

`LocalAdjustmentValues` gains five optional values, absent meaning neutral
exactly as the ten existing ones do:

```rust
pub clarity:         Option<i32>,  // [-100, +100]
pub texture:         Option<i32>,  // [-100, +100]
pub sharpness:       Option<i32>,  // [-100, +100]
pub noise_luminance: Option<i32>,  // [0, 100]
pub noise_color:     Option<i32>,  // [0, 100]
```

`sharpness` rather than `sharpening`: the global setting is a struct of
three numbers (`amount`, `radius`, `masking`), the local one is a single
slider, and the same name on both would promise a correspondence §3 refuses
to make. Noise reduction keeps the range it has globally — there is no
meaningful negative — while sharpness has one, and it is the whole point:
below zero the unsharp mask subtracts its own detail, which is the softening
named in the Context.

**No new architecture is required, and that is the load-bearing observation
of this ADR.** `v3` already clones the entire buffer and runs global
operators on the clone. A neighbourhood operator is one more global operator
run on that same full-frame copy. The composition ADR 0029 chose for
per-pixel operators is, without anyone having planned it, exactly the
composition a neighbourhood operator needs.

### 2. What a masked neighbourhood operator means — stated, not hidden

The operator runs on the **whole image**, and its result is blended by
coverage. It does not run "inside the mask". Near a mask's edge, the value a
pixel receives was computed from neighbours the mask **excludes**.

This is a real semantic, not an approximation of a better one. The
alternative — running the operator on the mask's bounding box — is faster
and wrong: a blur or an unsharp mask reading a truncated neighbourhood
produces a halo along that box, an artifact whose position depends on where
the user happened to draw. Lightroom behaves the same way, for the same
reason.

The consequence to accept, since it is visible: clarity's 40-pixel radius
inside a 20-pixel brush stroke is reading mostly excluded pixels. That is
what a large-radius operator *is*. The engine does not second-guess it.

### 3. The constants belong to the version, as clarity's and texture's already do

* **clarity** and **texture**: `CLARITY_RADIUS` (40) and `TEXTURE_RADIUS`
  (6), the very constants ranks 90 and 100 bind, multiplied by the render
  scale exactly as those ranks multiply theirs. A local operator that did
  not scale would look different in the loupe and in the export.
* **sharpness**: radius **1.0 px** — `Sharpening::default()`'s radius — and
  masking **0**. Deliberately **not** the revision's `sharpening.radius` or
  `sharpening.masking`: a local slider whose meaning shifts when a *global*
  one is touched is a slider nobody can reason about. One number in, one
  behaviour out.
* **noise**: the rank-170/180 operators (`v2`, edge-preserving), **not** the
  measured rank-5/6 ones ([ADR 0072](0072-measured-noise-profile.md)). Those
  read sensor counts, and at rank 160 — past exposure, past the tone curve —
  a measured threshold means nothing. That is the reason ADR 0072 moved them
  to rank 5 in the first place, and it applies here unchanged.

### 4. Within an entry, the pipeline's own order — a rule that already held in silence

The five run after the ten tonal values, in the order `clarity`, `texture`,
`noise_luminance`, `noise_color`, `sharpness`.

That is the order of their **ranks** (90, 100, 170, 180, 190), and it is the
rule `v1` has silently followed since ADR 0029: its tonal operators run
gains (40), contrast (50), highlights/shadows (60), whites/blacks (70),
vibrance (120), saturation (130) — rank order, never written down anywhere.
An entry replays the pipeline's order among the operators it
re-parameterizes. Stating it costs nothing today and settles every future
addition without a discussion.

### 5. The capability rule, for the fourth time

`v1`, `v2` and `v3` **refuse** a revision carrying any of the five, rather
than dropping it silently. [ADR 0048](0048-range-masks.md) §5's rule,
applied as ADR 0070, [ADR 0096](0096-sharpening-masking.md) and
[ADR 0098](0098-per-channel-tone-curves.md) applied it: a setting a pinned
version cannot express is an error naming the version, never a slider that
does nothing.

### 6. `v4` renders `v3` exactly

An entry that sets none of the five renders through `v4` bit for bit what
`v3` renders — structurally, since the five are the only new code and each
is guarded by its own `Option`. The reference renders record it: every `v3`
case gains a `v4` twin with the same hash, as ADR 0070 did for the `v2`/`v3`
pair.

### 7. Dehaze is refused, and the reason is not laziness

`dehaze` (rank 110) is the sixth global operator that reads more than one
pixel, and it is deliberately left out.

Its dark-channel prior estimates the **atmospheric light of the whole
frame** — a global statistic, not a neighbourhood. Re-parameterized on a
copy and blended by a mask, the number driving the local result would be
computed from pixels the mask excludes, and would move when the user edited
a *different* part of the picture. The other five read a neighbourhood; this
one reads the image. A local dehaze worth having needs a different operator,
and that is a different ADR.

## Consequences

* [`pipeline.md`](../pipeline.md) gains a `local_adjustments: 4` row at rank
  160, and `v4` becomes the version a new revision pins.
* The clients gain five sliders each: Studio's local-adjustment panel, and
  the CLI's mask payload keys.
* **Cost.** Each entry that sets one of the five pays one full-frame pass of
  that operator, on top of the copy `v3` already pays. An absent value costs
  nothing — the operator is not called. The expensive one is clarity, whose
  40-pixel radius is the largest kernel in the pipeline; it is the same pass
  rank 90 already pays once.
* The gesture this unlocks — a negative texture through a coverage mask — is
  neither portrait-specific nor AI. It is a slider that already exists
  globally, made local, on a mask the engine has been able to store since
  ADR 0070.
* **Nothing here is ever applied on its own.** The five are sliders a person
  moves, starting at neutral, on a mask a person placed. Should a detector
  ever propose the mask ([ADR 0073](0073-external-mask-detectors.md)), it
  proposes it — an automatic tool is a user's choice, never a default and
  never a standard, the same rule [ADR 0084](0084-assisted-culling.md) gave
  the culler ("it proposes and never writes") and
  [ADR 0088](0088-auto-tone-and-black-and-white.md) gave Auto (a button, not
  an import step).

## Rejected

* **Local dehaze** — §7: a global statistic driven by pixels the mask
  excludes.
* **Reusing the revision's `sharpening.radius` and `masking`** — §3: it
  couples a local slider to a global one.
* **Moiré and defringe**, which Lightroom also puts on a mask — Leyline has
  no such stage globally, so there is nothing to re-parameterize. A global
  stage first; then this question becomes trivial.
* **Computing the operator on the mask's bounding box** — §2: faster, and it
  manufactures a halo at a boundary the user drew for other reasons.
