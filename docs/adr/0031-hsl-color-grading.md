# ADR 0031 — HSL mixer and colour grading wheels: HSL derived from RGB, tonal zones weighted by luminance

**Status:** Accepted — 2026-07

## Context

`docs/v2-scope.md` §4 ("Colour grading / HSL mixer") notes two related gaps,
absent today when only **global** vibrance and saturation exist
(`crates/leyline-engine/src/process2.rs:298`):

* the **per-hue HSL mixer** — 8 hue/saturation/luminance bands, in the manner
  of Lightroom's HSL panel or Darktable's mixer;
* the **colour grading wheels** for shadows/midtones/highlights (colour plus
  luminance per zone), in the manner of Darktable's *color balance rgb* or
  Lightroom's *color grading* panel.

§4 sketches the fields `hsl` (8 bands × `{hue, saturation, luminance}`) and
`color_grading.shadows/.midtones/.highlights` plus `.blending`/`.balance`,
and leaves two questions open: the **hue model** to freeze into the render
contract (`docs/pipeline.md` §5) and **regional colour grading** (under a
mask). §9 confirms that it warrants an ADR of its own ("a new process, a
frozen hue model").

Three cross-cutting decisions are **consumed here, not relitigated**:

* **ADR 0028** freezes the versioning strategy: one process version per pixel
  feature, each in its own frozen `processN.rs` module, created by copying
  the previous module whole.
* **ADR 0027** confirms that the render's internal working space stays sRGB
  gamma-encoded between operators (`process2.rs:30`) — the space where these
  colour operators are defined.
* **ADR 0029** introduced spatial masking (a per-pixel `[0,1]` coverage,
  process 6); the present document explicitly distinguishes itself from it
  (see below) and leaves regional colour grading to a future ADR built on
  that infrastructure.

## Decision

The HSL mixer and the colour grading wheels are pixel operators: they take a
**new process version**, **the next available process number at the time this
feature ships** (ADR 0028), in its own `processN.rs` module, a complete copy
of the previous module plus the new colour operators alone. This ADR **does
not freeze** a specific process integer: the shipping order of items 3/4/5/6/8
belongs to the future implementation plan, not to this document.

This ADR **designs the two together** — HSL and colour grading — because
`docs/v2-scope.md` §4 scopes them as a single item and because they share the
same hue/luminance mathematics derived from the working RGB. It does **not
force** them to ship together (see *Alternatives rejected*): each can, if the
implementer prefers, take its own process number when it is ready (ADR 0028)
— this ADR designs them jointly, it does not couple their delivery schedule.

### The HSL mixer — HSL derived from the working RGB

The mixer operates in **standard HSL derived from the working RGB buffer**
(sRGB gamma-encoded, `process2.rs:30`), **not** in a perceptual/CIE space.
**8 fixed hue bands** — red, orange, yellow, green, aqua, blue, purple,
magenta — each defined by a central hue angle, with a **falloff function**
between adjacent bands so that a pixel whose hue falls between two centres
receives a weighted contribution from both. That model is exactly the
Lightroom/Darktable convention this feature aims at parity with.

What is **frozen here** is the **choice of model**: HSL derived from RGB, 8
bands at fixed centres, falloff between adjacent bands. The **exact numerical
constants** — band centre angles, the precise shape of the falloff curve —
belong to the implementation PR, at the same level of precision as the tone
curve (ADR 0030) and Lensfun's internal mathematics (ADR 0016). Freezing the
model is enough to guarantee reproducibility once the process version is
published (`docs/pipeline.md` §5).

Each band carries three sliders `{hue, saturation, luminance}` in `[-100,
+100]`, reusing the unit vocabulary of the existing sliders
(`process2.rs:236`): `hue` shifts the hue of the band's pixels, `saturation`
and `luminance` modulate their chroma and lightness — the same family of
operation as the existing `saturate` (`process2.rs:298`), re-parameterized
per hue band.

### The colour grading wheels — tonal zones weighted by luminance

The three zones — shadows / midtones / highlights — are separated by a
per-pixel **luminance-based weighting function** (the Rec. 709 luminance
already used in the engine,
`crates/leyline-engine/src/pixels.rs:110`): *smoothstep*-style zone masks
(the house already uses `x²(3 − 2x)`, `process2.rs:242`) which, for every
pixel, distribute its membership across the three zones according to its
luminance value. Two controls tune that distribution:

* **`balance`** — a pivot point that moves the shadows↔highlights boundary,
  deciding which range of luminances counts as "midtones";
* **`blending`** — the width of the zones' overlap, controlling how smooth the
  transitions between adjacent zones are.

Each zone carries `{hue, saturation, luminance}`: the chosen colour (hue plus
saturation) is blended into the zone's pixels in proportion to their zone
weight, and `luminance` sets their lightness.

**This is a blend by tonal zone, not spatial masking.** The point is
underlined so that it is **not confused** with ADR 0029's spatial mask
mechanism: colour grading weights by the pixel's **luminance value** (a dark
pixel is "a shadow" wherever it sits in the frame), whereas an ADR 0029 mask
weights by the pixel's **spatial position** (a `[0,1]` coverage lifted back to
the pre-rotation buffer by ADR 0026). They are **two orthogonal mechanisms
operating on different axes** — tonal value against spatial position — and
colour grading borrows **nothing** from the masking infrastructure.

### Place in the pipeline

Both stages fit into the **colour block, after Vibrance/Saturation** — which
the "Pipeline (§3.1)" row of `docs/v2-scope.md` §4's table already fixes. This
ADR **confirms and consumes** that placement, it does not re-derive it: HSL
and colour grading refine the colour once global saturation has been applied.

### Regional colour grading — outside V2's scope

Applying HSL or colour grading **under a mask** (combining them with ADR
0029's spatial infrastructure) is **explicitly outside V2's scope**: it is a
natural extension once both this feature and masking exist, deferred to a
future ADR — not designed here.

### Storage — an additive schema

`hsl` (8 bands × `{hue, saturation, luminance}`) and
`color_grading.shadows/.midtones/.highlights` (`{hue, saturation, luminance}`
per zone) plus `color_grading.blending`/`.balance` are **additive** fields of
`settings_json`. **Absent means neutral** (0 everywhere: no hue shift, no zone
colouring), rendered **bit-for-bit identically** to the previous process
version — the invariant "*a parameter at its neutral value skips its operator
entirely, so the neutral rendering is bit-for-bit the decoded image*"
(`process3.rs:25`). No schema bump is required, consistent with "process +1,
schema unchanged" (`docs/v2-scope.md` §1); fields unknown to an older engine
are preserved verbatim (`Settings::extra`,
`crates/leyline-core/src/settings.rs`).

### A JSON sketch

The style follows `docs/pipeline.md` §3.2. A non-neutral example, then the
neutral case:

```json
{
    "schema": 1,
    "process": 8,

    "vibrance": 12,
    "hsl": [
        { "hue": 0,   "saturation": -20, "luminance": 0 },
        { "hue": 5,   "saturation": 0,   "luminance": 0 },
        { "hue": 0,   "saturation": 0,   "luminance": 0 },
        { "hue": -10, "saturation": 15,  "luminance": 8 },
        { "hue": 0,   "saturation": 0,   "luminance": 0 },
        { "hue": 8,   "saturation": 25,  "luminance": 0 },
        { "hue": 0,   "saturation": 0,   "luminance": 0 },
        { "hue": 0,   "saturation": 0,   "luminance": 0 }
    ],
    "color_grading": {
        "shadows":    { "hue": 220, "saturation": 15, "luminance": 0 },
        "midtones":   { "hue": 0,   "saturation": 0,  "luminance": 0 },
        "highlights": { "hue": 45,  "saturation": 10, "luminance": 0 },
        "balance": 0,
        "blending": 50
    }
}
```

The neutral case — the fields absent, rendered bit-for-bit identically to the
previous process version:

```json
{
    "schema": 1,
    "process": 8,
    "vibrance": 12
}
```

> *The `process: 8` above is purely illustrative: the real number is whatever
> is next available at shipping time (ADR 0028), not fixed by this ADR. The 8
> `hsl` entries follow the fixed band order
> red/orange/yellow/green/aqua/blue/purple/magenta.*

> **An implementation note (not a spec edit here).** This ADR does **not**
> modify `docs/pipeline.md` §3.1's diagram nor §3.3's process-version table.
> As for ADR 0029 and ADR 0030, the spec is updated in the same change as the
> actual implementation, in keeping with CLAUDE.md. The present document fixes
> **where** the stages land and **which** models they freeze; the §3.1 diagram
> and the §3.3 table will be amended by the PR that ships the corresponding
> process module.

## Consequences

* **The `process` field keeps its semantic legibility** (ADR 0028): the new
  number will mean "HSL mixer + colour grading active", a fact as readable as
  `process: 3` meaning "distortion correction active".
* **A frozen neutral output**: with no settings, both stages are bit-for-bit
  the previous process version (`process3.rs:25`).
* **No coupling to the masking infrastructure**: colour grading weights by
  luminance, orthogonally to ADR 0029's spatial mask — the two can evolve
  independently, and colour grading ships without waiting for masking.
* **Regional colour grading stays open** for a future ADR built on ADR 0029,
  without blocking the global version shipped here.
* **HSL and colour grading can, if wanted, ship separately** (ADR 0028, each
  with its own process number) even though they are designed together here:
  this ADR describes the mathematics, it does not couple the delivery
  schedule.
* **The reproducibility contract** (`docs/pipeline.md` §5) is respected: the
  hue model and the zone weighting are frozen by the process version, the
  operators are pure and deterministic (ADR 0012), and everything lives in
  `settings_json`.
* **One more `processN.rs` module** (ADR 0028): no earlier module is touched,
  and the "same pixels in ten years" freeze stays unfalsifiable (§3.3).
* **The `docs/pipeline.md` spec (§3.1, §3.3) is not edited by this ADR**: it
  will be by the implementation PR, in keeping with CLAUDE.md.

## Alternatives rejected

* **A perceptual/CIE hue space (LCh, OKLCh…) rather than HSL derived from
  RGB.** Rejected: a perceptual space would give more regular hue
  transitions, but it would break the parity with the Lightroom/Darktable
  convention this feature aims at (the bands and the results users expect are
  defined in RGB HSL), would impose a round trip out of the frozen sRGB
  working space (ADR 0027), and would weigh down the frozen contract for a
  benefit the real gap — HSL mixer parity — does not ask for. HSL derived
  from the working RGB is the parity model and the closest to the existing
  space.
* **Spatial masking (ADR 0029) as the zone-selection mechanism** for colour
  grading, instead of luminance weighting. Rejected: that would conflate two
  orthogonal axes. A "shadow" in colour grading is a **low luminance value**,
  wherever it sits in the frame — not a spatial region. Using a spatial mask
  would force the user to paint the tonal zones by hand, which is neither the
  intended ergonomics nor the tool's semantics. Luminance weighting (a
  smoothstep over Rec. 709 luma, `pixels.rs:110`, `process2.rs:242`) selects
  the zones automatically by value, with no geometry.
* **Shipping HSL and colour grading as two separate ADRs/process versions**
  instead of one combined design. Rejected **as a design choice**, not as a
  delivery constraint: `docs/v2-scope.md` §4 scopes them as a single item and
  they share the hue/luminance mathematics derived from RGB — designing them
  together avoids two ADRs re-deriving the same hue model. This ADR
  nevertheless leaves the implementer free to give them two distinct process
  numbers at delivery time (ADR 0028): the design is shared, the schedule is
  not.
* **Supporting regional colour grading (under a mask) in V2.** Rejected:
  deferred to a future ADR built on ADR 0029's masking infrastructure. The
  global version (by tonal zone) stands alone and can ship without masking;
  the regional version will build on it naturally when the time comes, in the
  common frame of ADR 0026, without being rewritten.
