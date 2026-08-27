# ADR 0030 — Tone curve: a point curve, a monotone cubic spline, applied in luminance through a LUT

**Status:** Accepted — 2026-07

## Context

`docs/v2-scope.md` §3 ("Tone curve") notes that no curve exists today —
neither parametric nor point-based: only the coarse sliders of the tonal
block (exposure, contrast, highlights/shadows, whites/blacks) are implemented
(`crates/leyline-engine/src/process2.rs:236`, `:257`, `:280`). A tone curve
is the fine-tuning tool those sliders do not cover: it lets any input level
be freely repositioned onto any output level.

§3 sketches three fields (`tone_curve.points`, `tone_curve.parametric`,
`tone_curve.channel`) and leaves two questions open: the interpolation to
freeze into the render contract (`docs/pipeline.md` §5: two engines, the same
points, the same pixels), and the choice between "per-channel RGB curves from
V2 on, or luminance alone first". §9 confirms that it warrants an ADR of its
own ("a new process, frozen interpolation (a light ADR)").

Three cross-cutting decisions are already taken upstream and **consumed here,
not relitigated**:

* **ADR 0028** freezes the versioning strategy: one process version per pixel
  feature, each in its own frozen `processN.rs` module, created by copying
  the previous module whole. This curve is a pixel operator: it therefore
  takes a new process version, without this ADR having to re-choose the
  convention.
* **ADR 0013** established the table-based transfer-function convention: a
  LUT of `LUT_SIZE` intervals whose entry `i` is the exact formula evaluated
  at `i / LUT_SIZE`, with lookups interpolating linearly between adjacent
  entries (`process2.rs:54`–`:101`). This ADR **reuses** that convention for
  the curve, it does not reinvent it.
* **ADR 0027** confirms that the render's internal working space stays sRGB
  gamma-encoded between operators — the space where the existing tonal block
  is defined and where this curve fits in.

## Decision

The tone curve is a new pixel operator, and therefore a **new process
version**: it takes **the next available process number at the time this
feature ships** (ADR 0028), in its own `processN.rs` module, a complete copy
of the previous module plus the curve operator alone — exactly the
per-module duplication convention reaffirmed by ADR 0028. This ADR **does not
freeze** a specific process integer: the shipping order of items 3/4/5/6/8
belongs to the future implementation plan, not to this document.

### The central simplification — a point curve only, no parametric engine

**V2 ships only the point curve** (`tone_curve.points`, a list of `{x, y}`
control points normalized to `[0,1]`). The `tone_curve.parametric` field
sketched in §3 (highlights/lights/darks/shadows regions plus pivot points) is
**removed from the engine's render contract**.

The reasoning, stated as this ADR's central decision and not as an oversight:
a parametric curve is **not distinct render mathematics**, it is a
**different UI for generating a list of control points**. A parametric
curve's region sliders produce, in the end, a curve — that is, a set of
points. If Studio one day wants to offer a parametric-style editor, it
computes the equivalent point list **on the client side** and writes it into
`tone_curve.points`; the engine then has **one single curve mathematics** to
freeze and to keep to "the same pixels in ten years" (`docs/pipeline.md`
§3.3), instead of two. Carrying two separate curve engines in the frozen
render would be two contracts to freeze and two regression surfaces, for a
capability the first entirely subsumes.

### Interpolation — frozen into the render contract

Interpolation between control points is a **monotone cubic spline**
(Fritsch–Carlson or an equivalent that preserves monotonicity). That choice
is frozen here because it is part of the reproducibility contract
(`docs/pipeline.md` §5: two engines, the same points, must produce the same
pixels) — just like ADR 0013's transfer function or ADR 0016's internal
Lensfun mathematics. A **monotone** cubic spline is chosen specifically to
avoid the *overshoot* and *ringing* a naive cubic spline introduces between
widely spaced control points: between two points, a naive cubic can overshoot
and come back, creating visible tonal inversions (banding, halos) where the
user expects a monotone transition. A tonal operator must stay monotone, as
`contrast`, `highlights_shadows` and `whites_blacks` already are ("*both
blends are monotone*", `process2.rs:235`; "*so the endpoints are fixed and
the response is monotone*", `process2.rs:256`).

This ADR freezes the **choice of model** (a monotone cubic spline). The exact
numerical details of the implementation (the precise tangent formulation, the
handling of collinear points) belong to the implementation PR, at the same
level of precision as ADR 0016 for Lensfun's internal mathematics — freezing
the model is enough to guarantee reproducibility once the process version is
published.

### Channel — luminance alone

V2 applies the curve **in luminance only**: a single shared curve, applied to
the same sRGB gamma-encoded RGB working buffer (`process2.rs:30`), and **not**
three independent per-channel R/G/B curves. The `tone_curve.channel` field
sketched in §3 is therefore reduced to its single implicit neutral value
(luminance); per-channel curves are **cut from V2** as a separate and heavier
feature (a larger UI, three times the curve state, the question of the order
in which the three curves apply) — to be designed later if wanted, in an ADR
of its own. That is a deliberate cut, in the same spirit as ADR 0016 cutting
vignetting and TCA from process 3 in order to ship the core first.

### Precomputation — a LUT, not per-pixel evaluation

The curve is precomputed into a **LUT** and then applied by interpolated
lookup, reusing exactly ADR 0013's convention (`process2.rs:74`–`:90`): a
table plus linear interpolation between entries, rather than evaluating the
spline at every pixel. The spline is evaluated once per table entry when the
revision is built; per-pixel rendering is only a lookup. The LUT's exact
**resolution** is a constant of the implementation PR, not decided here (as
`LUT_SIZE` is a frozen constant of the process module, `process2.rs:54`, and
not an ADR's choice); only the *method* — precomputing into a LUT — is
frozen.

### Place in the pipeline

The curve stage fits into the fixed order of `docs/pipeline.md` §3.1 **after
Whites/Blacks and before Vibrance/Saturation** — which the "Pipeline (§3.1)"
row of `docs/v2-scope.md` §3's table already fixes. This ADR **confirms and
consumes** that placement, it does not re-derive it. It is the last step of
the tonal block before the colour block: the curve operates on tones already
set by the coarse sliders, before saturation applies.

### Storage — an additive schema

`tone_curve.points` is an **additive** field of `settings_json`. **Absent or
an empty list means the identity curve** (every level maps onto itself),
rendered **bit-for-bit identically** to the previous process version. That
preserves the invariant "*a parameter at its neutral value skips its operator
entirely, so the neutral rendering is bit-for-bit the decoded image*"
documented at the head of `process3.rs:25`. No schema bump is required,
consistent with the additive "process +1, schema unchanged" pattern of most
V2 items (`docs/v2-scope.md` §1) — fields unknown to an older engine are
already preserved verbatim (`Settings::extra`,
`crates/leyline-core/src/settings.rs`).

### A JSON sketch

The style follows `docs/pipeline.md` §3.2. A gentle S-curve (lifting the
shadows, lowering the highlights) and the neutral case:

```json
{
    "schema": 1,
    "process": 7,

    "exposure": 0.2,
    "tone_curve": {
        "points": [
            { "x": 0.0,  "y": 0.0 },
            { "x": 0.25, "y": 0.30 },
            { "x": 0.75, "y": 0.70 },
            { "x": 1.0,  "y": 1.0 }
        ]
    }
}
```

The neutral case — the field absent (or `"points": []`), rendered bit-for-bit
identically to the previous process version:

```json
{
    "schema": 1,
    "process": 7,
    "exposure": 0.2
}
```

> *The `process: 7` above is purely illustrative: the real number is whatever
> is next available at shipping time (ADR 0028), not fixed by this ADR.*

### Masking — outside this decision

This ADR does **not** decide whether the tone curve becomes a maskable
setting under ADR 0029 (`LocalAdjustment`): that would be a small future
extension of that struct's set of adjustable fields, not designed here.

> **An implementation note (not a spec edit here).** This ADR does **not**
> modify `docs/pipeline.md` §3.1's diagram nor §3.3's process-version table.
> As for ADR 0029, the spec is updated in the same change as the actual
> implementation, in keeping with CLAUDE.md. The present document fixes only
> **where** the stage lands and **which** mathematics it freezes; the §3.1
> diagram and the §3.3 table will be amended by the PR that ships the curve's
> process module.

## Consequences

* **The `process` field keeps its semantic legibility** (ADR 0028): the new
  number will mean exactly "point tone curve active", a single readable fact,
  as `process: 3` means "distortion correction active".
* **A frozen neutral output**: with no points, the stage is bit-for-bit the
  previous process version. The invariant "a neutral value → the operator
  entirely skipped" (`process3.rs:25`) stays true for the whole stage.
* **One curve path to maintain**: cutting the parametric mode keeps the
  frozen render minimal; a future parametric editor in Studio adds no render
  code, it writes points.
* **The reproducibility contract** (`docs/pipeline.md` §5) is respected: the
  monotone interpolation is frozen by the process version, the LUT is pure
  and deterministic (ADR 0012), and everything serializes into
  `settings_json` — "the same revision → the same pixels".
* **Two extensions stay open for future ADRs** without blocking the core:
  per-channel R/G/B curves, and a masked curve under ADR 0029. Neither is
  required to ship the point curve in luminance.
* **One more `processN.rs` module** (ADR 0028): a bounded and known cost; no
  earlier version's module is touched, and the "same pixels in ten years"
  freeze stays mechanically unfalsifiable (`docs/pipeline.md` §3.3).
* **The `docs/pipeline.md` spec (§3.1, §3.3) is not edited by this ADR**: it
  will be by the implementation PR, in keeping with CLAUDE.md.

## Alternatives rejected

* **Shipping region-based parametric curves as a distinct first-class engine
  feature**, alongside the point curve. Rejected: it is this ADR's central
  simplification. A parametric curve produces only a list of points; making
  it a second render engine would force **two** curve mathematics paths to be
  frozen and maintained at "the same pixels in ten years" (§3.3), two
  regression surfaces, when the point curve subsumes both. Studio computes
  the equivalent point list client-side if a parametric editor is ever wanted
  — no extra render code, and no extra frozen contract.
* **Per-channel R/G/B curves from V2 on.** Rejected as a separate and heavier
  feature: three times the curve state, a UI per channel, and the question of
  the order in which the three curves apply to one another. Luminance alone
  covers the main tonal use; per-channel curves (colour toning by curve) are
  a piece of work apart, to be designed later in an ADR of their own if
  wanted — the same spirit of cutting as ADR 0016.
* **A naive (non-monotone) cubic spline.** Rejected: between widely spaced
  control points, a naive cubic *overshoots* and comes back, creating visible
  tonal inversions (banding, halos) where the user expects a monotone
  transition. Every existing tonal operator is monotone by design
  (`process2.rs:235`, `:256`); a curve that broke that property would be a
  perceptible quality regression. The monotone cubic spline
  (Fritsch–Carlson) gives smooth transitions **without** overshoot.
* **Evaluating the spline per pixel rather than through a LUT.** Rejected:
  the spline is costly to evaluate and depends only on the `[0,1]` input
  value — the exact use case for a table (ADR 0013). Precomputing a LUT once
  per revision and then doing an interpolated lookup per pixel reuses the
  engine's already-frozen convention (`process2.rs:54`–`:90`), for a constant
  per-pixel cost instead of a full spline evaluation at every sample.
