# ADR 0118 — Grain that is not one grey

**Status:** Accepted — 2026-09

## Context

[ADR 0090](0090-effects-vignette-grain.md) §3 shipped `grain` with three
settings — `amount`, `size`, `roughness` — and its §4 refused a fourth in a
single line:

> **Coloured grain.** Chroma noise is what `noise_color` spends its time
> removing (ADR 0072).

That line is wrong, and correcting it is worth more than quietly adding a
field. It answers a question about **film** with an argument about **a
sensor**, and the two are not the same object:

* **They are 224 ranks apart.** `noise_color` runs at rank 6
  ([ADR 0072](0072-measured-noise-profile.md) put the profiled version there,
  before exposure); `grain` runs at 230, after the crop, one stage from the
  display signal. Nothing in the pipeline is removing what this stage adds.
  There is no fight between them — only a sequence, with every tone operator
  in between.
* **Colour film's grain is chromatic because of how film is built.** A colour
  negative is three emulsion layers, each with its own silver-halide
  crystals, each developing independently. Their grain does not agree, and
  the disagreement is visible: that is what colour grain *is*. A
  strictly monochrome grain is a black-and-white film's grain laid over a
  colour photograph — a perfectly good look, and not the only one.

So the field is added. The other half of the same gap — a list of grain
*types* — is refused instead, in §5, with its reason.

## Decision

### 1. `grain::v2`, one new field

```rust
pub struct Grain {
    pub amount: i32,     // [0, 100], neutral at 0
    pub size: i32,       // [0, 100]
    pub roughness: i32,  // [0, 100]
    pub color: i32,      // [0, 100], default 0   ← new
}
```

Rank stays 230, space stays linear-with-a-display-detour, and `is_neutral`
still reads `amount` alone: a colour setting at zero strength is not a
rendering, the same rule `Vignette::is_neutral` and v1 already follow.

A new **version**, not an edit: `grain::v1` is frozen and a revision citing
it renders through it forever (`docs/pipeline.md` §5.1).

### 2. Colour is *added* as chroma, never traded against grey

The obvious construction — cross-fade the shared field into three
independent ones — is wrong, and measurably so. Two independent fields of
variance σ² blended at `k` have variance `((1−k)² + k²)·σ²`: at the middle
of the slider the grain would be **30 % quieter** than at either end. A
control whose middle is a dip is a control that lies.

So the mono field keeps its full amplitude and a chroma-only deviation is
added on top. With `n_r, n_g, n_b` three independent fields from the same
lattice, salted per channel:

```text
offset_c = mono + k · (n_c − (n_r + n_g + n_b)/3)
```

The added term sums to zero across the three channels **by construction**,
so it carries no luminance in the working space. The consequence is the one
a photographer expects from a slider called *colour*: the grain's grey stays
as loud at every setting, and what grows is how much the three layers
disagree. At `k = 1` they disagree as much as three independent emulsions
would.

Measured on a neutral ramp at `amount: 100`, the grain's grey moves the mean
of the three channels by **4.22 levels at `color: 0` and 4.36 at 100** — 3 %
across the whole travel, where the cross-fade would have lost 30 % in the
middle. The residual is honest and named: the perturbation cancels in the
working space, and `output_rendering`'s primaries rotation is not
sum-preserving, so a little of it reappears as luminance in the encoded
file. The per-channel fade `4·L·(1 − L)` adds a second, larger residual on
strongly coloured pixels, where the three channels are weighted unalike.
Neither is a defect to fix: fixing them would mean weighting the fade by a
single per-pixel value, which would make the *grey* grain behave differently
at `color: 1` than at `color: 0` — a discontinuity in exchange for a
cancellation nobody can see.

`4·L·(1 − L)` still weights the whole thing, per channel as in v1: grain
that survives into a black or into a specular highlight reads as a hot
sensor whatever colour it is.

### 3. At `color: 0`, v2 **calls** v1

Not "computes the same thing" — calls it. Bit-identity becomes a property of
the control flow instead of an argument about floating point, which is
exactly what [ADR 0098](0098-per-channel-tone-curves.md) §1 did for the
per-channel tone curve and what the golden manifest then holds in place: the
`grain` case carries the same digest under v1 and under v2.

### 4. The capability rule, once more

`color` non-zero on a revision pinned at `grain: 1` is **refused by
`Settings::validate`**, by name, rather than silently dropped — the rule
[ADR 0048](0048-range-masks.md) §5 set and ADR 0050, 0061, 0096, 0098, 0107,
0108 and [0116](0116-local-defringe.md) have applied since. The user learns
their revision needs reprocessing instead of watching a slider do nothing.

### 5. No grain *types*, and that is the decision

Rival applications offer a **list of named grain types**. Leyline does not,
for three reasons that compound:

* **A name in that list is a claim.** Either it names a film stock — which
  we have not measured, cannot measure without the stock and a scanner, and
  would be borrowing the reputation of — or it is an adjective
  (*fine*, *classic*, *harsh*) that tells the user nothing about what moved.
  This repository's answer to an opaque control is the parameter that names
  what it does; that is why `size` is a lattice spacing in full-resolution
  pixels and not a *Small / Medium / Large*.
* **A list frozen into a stage version is frozen forever.** Every name in it
  would have to render identically in ten years (§5.1). Someone's taste,
  pinned as hard as the arithmetic.
* **A named grain already has a home.** `vignette` and `grain` are captured
  together by the `effects` preset category (ADR 0090 §5). A grain a person
  liked is saved, named, edited and shared as a preset — user-owned, not
  engine-owned. The cost is stated rather than hidden: such a preset also
  carries the vignette, because the category does, and a *film look* that
  dropped its own vignette would be missing half the look.

Four continuous parameters span the family those lists enumerate. A fifth
axis — the shape of the noise's distribution, sparse hard specks against
dense sand — is a real difference this construction cannot express, and it
is left for a `grain::v3` with a reason, not smuggled in under a name.

## Consequences

* `Grain` gains a field; `Settings` gains no schema version, because `Grain`
  is `#[serde(default)]` and an absent `color` reads as 0 — every revision
  written before today parses and renders exactly as it did.
* `PresetSettings::grain` carries the new field with no change: it stores a
  whole `Grain`, so the `effects` category picked it up the moment the struct
  did.
* The golden manifest **gains** entries and moves none. The `grain` case is
  replayed under v1 and under v2 with the same digest (§3); a new
  `grain_color` case pins the coloured field.
* Cost, stated: when `color > 0` the stage evaluates four noise fields per
  pixel instead of one — six extra `value_noise` calls at two octaves. It is
  the last stage before the output conversion and it runs over rows in
  parallel; the price is paid only by photographs that asked for it, and
  never by a preview of a photograph that did not.
* The clients gain one control: a fourth slider in Studio's *Effects* panel,
  and a fourth optional number on the CLI's `grain` verb.
* `grain::v1` is edited in exactly one way: two items become `pub(super)` so
  v2 can call them. That is a visibility change and not a rendering one —
  the golden manifest is the proof, since the v1 entries did not move — and
  it is the same arrangement `tone_curve::v1` has carried since ADR 0098.

## Rejected

* **Cross-fading grey into colour** (§2). The dip in the middle is not a
  subtlety; it is the whole behaviour of the slider.
* **Independent `size` per channel.** Real emulsion layers do have different
  crystal sizes, and exposing it would be three more numbers for a
  difference nobody can see below 1:1 — where §3 of ADR 0090 already admits
  grain cannot be judged at all.
* **A grain seed**, again. ADR 0090 §3 refused it and nothing here changes
  the argument: a seed's only meaning is *give me a different randomness*,
  and it would have to enter every revision, preset, fingerprint and golden
  entry to deliver it.
* **Making `color` the default.** Grain is opt-in at `amount: 0`, and the
  moment someone raises it they get exactly what ADR 0090 shipped until they
  ask for something else. A new field never changes what an old setting
  means.
