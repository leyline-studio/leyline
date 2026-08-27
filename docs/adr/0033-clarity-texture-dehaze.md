# ADR 0033 — Clarity, texture, dehaze: one two-radius local contrast, and dehaze by closed-form dark channel prior

**Status:** Accepted — 2026-07

## Context

`docs/v2-scope.md` §6 ("Dehaze / texture / clarity") notes that only "detail"
exists today (noise reduction plus sharpening, `process2.rs:324`, `:357`), with
no separate controls for **clarity**, **texture** and **dehaze** — three
distinct local/frequency contrast treatments that any RAW developer expects at
parity with Lightroom, Darktable and Capture One.

Three cross-cutting decisions are **consumed here, not relitigated**:

* **ADR 0028** freezes the versioning strategy: one process version per pixel
  feature, each in its own frozen `processN.rs` module, created by copying the
  previous module whole. These three sliders are pixel operators: they take a
  new process version, without this ADR having to re-choose the convention.
* **ADR 0027** confirms that the render's internal working space stays sRGB
  gamma-encoded between operators (`process2.rs:30`) — the space where the
  existing presence/detail block is defined and where these three stages fit
  in.
* **ADR 0029** introduced spatial masking (a per-pixel `[0,1]` coverage); the
  present document ships the three sliders **globally** and defers their
  masked version to a future ADR built on that infrastructure (see below).

§6 left three questions open specific to the item: dehaze's determinism, the
"global first, masked later" order, and the CPU cost of multi-scale local
contrast on large previews (`docs/engine-api.md` §11). This document settles
them at the same time as it fixes the mathematics, the placement and the
storage.

## Decision

Clarity, texture and dehaze are pixel operators: they take a **new process
version**, **the next available process number at the time this feature ships**
(ADR 0028), in its own `processN.rs` module, a complete copy of the previous
module plus the new stages alone. This ADR **does not freeze** a specific
process integer.

The scope is **exactly three independent sliders** — `clarity`, `texture`,
`dehaze` — and no further slider is invented.

### Clarity and texture — one parameterized local contrast, two radii

Clarity and texture belong to **one algorithm family**: local contrast by
blurred mask (*unsharp mask*), that is, amplifying the difference between a
pixel and a low-pass (blurred) version of itself — exactly the shape of the
existing `sharpen`, `L' = L + amount·(L − blur(L, σ))` (`process2.rs:357`), but
at a larger radius and without being bounded to fine detail. They differ **by
the blur's radius**:

* **clarity** — a **large radius**: broad local contrast, the classic "clarity
  look" that gives presence to the midtones;
* **texture** — a **small radius**: fine local contrast, the micro-detail.

**Decision: both are implemented as a single parameterized internal
local-contrast function**, called **twice** with different radius and strength
constants — **not** two independently invented algorithms. That is
***intra-version* code reuse, inside one process module**, and it is explicitly
allowed: **ADR 0028 forbids only sharing code *between* frozen process-version
modules** (the risk of a fix silently altering an earlier frozen rendering),
**not** factoring within a single module. The point is stated explicitly
because it might otherwise seem to contradict ADR 0028: it does not — the
shared function lives entirely inside the new module, frozen with it, with no
link to any earlier module.

### Dehaze — a dark channel prior as a closed, deterministic procedure

Dehaze is an **atmospheric-veil removal of the *dark channel prior* kind** (a
pixel's dark channel being the minimum over its RGB channels within a local
neighbourhood, from which the atmospheric veil and the transmission are
estimated).

**The estimation of the atmospheric light and of the transmission must be an
entirely specified, deterministic, closed-form procedure** — **no iterative
optimization**, nothing whose result depends on an initialization or a
convergence tolerance. **Decision: the atmospheric light is estimated from a
fixed percentile of the brightest pixels of the image's dark channel** (a
**closed-form selection**, not an iterative solver), and the exact selection
rule is **frozen into the render contract** once implemented.

What is **frozen here** is the **choice of algorithm and family** — a dark
channel prior, atmospheric light from an upper percentile of the dark channel,
transmission derived in closed form. The **exact numerical constants** — the
percentile's value, the dark channel's neighbourhood size, the transmission's
guard factor — belong to the implementation PR, at the **same level of
precision** as ADR 0016 (Lensfun's internal mathematics), ADR 0030 (the tonal
spline) and ADR 0031 (the hue model). Freezing the model is enough to
guarantee reproducibility once the process version is published
(`docs/pipeline.md` §5).

### CPU cost — an approximate blur by downsampling, not a full-resolution large kernel

Clarity's large-radius blur is **costly** at full resolution on large previews
and exports (`docs/engine-api.md` §11, flagged by §6's open question #3). The
existing separable `gaussian_blur` (`process2.rs:396`) has a kernel of radius
`⌈3σ⌉`: at large σ it becomes prohibitive.

**Decision: the blur's algorithm family is an approximation by
downsampling / box filter (of the Gaussian-pyramid kind)** of the large-radius
blur — **not** a literal large-radius Gaussian kernel at full resolution — so
as to bound the cost. The **exact downsampling factor** and the **kernel size**
are constants of the implementation PR, not decided here — the **same level of
precision** as everywhere else in this series of ADRs; only the **family** — a
bounded approximate blur — is frozen.

### Global only in V2 — the masked version is deferred

The three sliders ship **globally**. **Masked or regional dehaze, clarity and
texture** (combining them with ADR 0029's spatial mask infrastructure) is
**explicitly outside V2's scope** — the same one-line cut as ADR 0031 for
regional colour grading: it is a natural extension once both this feature and
masking exist, deferred to a future ADR built on ADR 0029, not designed here.
Global sliders are shippable **without waiting for** item 2
(`docs/v2-scope.md` §6, open question #2).

### Place in the pipeline — clarity → texture → dehaze, before Vibrance/Saturation

The three stages fit into the fixed order of `docs/pipeline.md` §3.1 in the
**tonal/presence block, before Vibrance/Saturation**: clarity and texture
(local contrast) near the presence sliders, dehaze after the coarse tonal
sliders. Which the "Pipeline (§3.1)" row of `docs/v2-scope.md` §6's table
already fixes ("clarity/texture… in the tonal/presence block; dehaze after the
tonal block") — this ADR **confirms and consumes** that placement, it does not
re-derive it.

**The precise order among the three, and relative to Vibrance/Saturation:
clarity → texture → dehaze → Vibrance/Saturation.** All three land in the tonal
block **before** Vibrance/Saturation. An intended consequence: the position of
ADR 0029's "Local adjustments" stage (immediately **after**
Vibrance/Saturation) stays **unchanged**, whichever of this feature or item 2
ships first — both insert their stages at distinct fixed positions of the
order, without overlapping and without reconciliation (ADR 0028).

### Storage — an additive schema

`clarity`, `texture` and `dehaze` are three **additive** sliders of
`settings_json`, in `[-100, +100]`, **neutral = 0, absent = 0** — the same unit
and range convention as the existing sliders with no physical dimension
(`contrast`, `vibrance`… `process2.rs:236`, `docs/pipeline.md` §3.2). At 0, each
stage is **entirely skipped**, rendered **bit-for-bit identically** to the
previous process version — the invariant "*a parameter at its neutral value
skips its operator entirely, so the neutral rendering is bit-for-bit the
decoded image*" (`process3.rs:25`). No schema bump is required, consistent with
"process +1, schema unchanged" (`docs/v2-scope.md` §1); fields unknown to an
older engine are preserved verbatim (`Settings::extra`,
`crates/leyline-core/src/settings.rs`).

> **A note on range.** All three sliders are bidirectional `[-100, +100]` for
> vocabulary uniformity with the existing sliders and for Lightroom parity (a
> negative dehaze adds atmospheric veil back rather than removing it; negative
> clarity and texture soften local contrast). That is a minor UI detail, **not**
> a freezing of the render contract: the implementation PR could restrict
> dehaze to `[0, 100]` if the ergonomics demand it, without reopening this ADR.

### A JSON sketch

The style follows `docs/pipeline.md` §3.2. A non-neutral example, then the
neutral case:

```json
{
    "schema": 1,
    "process": 9,

    "exposure": 0.2,
    "clarity": 25,
    "texture": 15,
    "dehaze": 30
}
```

The neutral case — the fields absent (or at 0), rendered bit-for-bit
identically to the previous process version:

```json
{
    "schema": 1,
    "process": 9,
    "exposure": 0.2
}
```

> *The `process: 9` above is purely illustrative: the real number is whatever
> is next available at shipping time (ADR 0028), not fixed by this ADR.*

> **An implementation note (not a spec edit here).** This ADR does **not**
> modify `docs/pipeline.md` §3.1's diagram nor §3.3's process-version table. As
> for ADR 0029/0030/0031/0032, the spec is updated in the same change as the
> actual implementation, in keeping with CLAUDE.md. The present document fixes
> **where** the stages land and **which** models they freeze; the §3.1 diagram
> and the §3.3 table will be amended by the PR that ships the process module.

## Consequences

* **The `process` field keeps its semantic legibility** (ADR 0028): the new
  number will mean exactly "clarity/texture/dehaze active", a fact as readable
  as `process: 3` meaning "distortion correction active".
* **A frozen neutral output**: at 0, the three stages are bit-for-bit the
  previous process version (`process3.rs:25`).
* **One local-contrast path to maintain**: clarity and texture share a
  parameterized function called at two radii — less frozen code, a single
  regression surface, and the distinction from ADR 0028 (sharing forbidden
  *between* modules, allowed *within* a module) is explicit.
* **Dehaze is reproducible by construction**: a closed-form atmospheric
  estimate (a dark channel percentile), no iteration, no dependence on an
  initialization — frozen with the process version (`docs/pipeline.md` §5).
* **A bounded cost on large previews**: the large-radius blur is approximated
  by downsampling, never a full-resolution kernel — `docs/engine-api.md` §11's
  concern is addressed by the choice of family, the constants being left to the
  PR.
* **Regional dehaze, clarity and texture stay open** for a future ADR built on
  ADR 0029, without blocking the global version shipped here
  (`docs/v2-scope.md` §6).
* **Three stages before Vibrance/Saturation**: the position of ADR 0029's stage
  (after Vibrance/Saturation) stays intact whatever the shipping order (ADR
  0028).
* **The reproducibility contract** (`docs/pipeline.md` §5) is respected: the
  local-contrast model and the atmospheric selection rule are frozen by the
  process version, the operators are pure and deterministic (ADR 0012), and
  everything lives in `settings_json`.
* **One more `processN.rs` module** (ADR 0028): no earlier module is touched,
  and the "same pixels in ten years" freeze stays unfalsifiable (§3.3).
* **The `docs/pipeline.md` spec (§3.1, §3.3) is not edited by this ADR**: it
  will be by the implementation PR, in keeping with CLAUDE.md.

## Alternatives rejected

* **Clarity and texture as two fully independent algorithms** instead of one
  function parameterized at two radii. Rejected: the two **are** the same
  algorithm — local contrast by blurred mask — differing in a single parameter,
  the radius. Writing them separately would duplicate the same mathematics
  within the same module for no benefit, and would multiply the points at which
  a regression could diverge between two treatments meant to be the same
  family. *Intra-module* factoring is allowed (ADR 0028 forbids only sharing
  *between* frozen modules): one function, two sets of constants.
* **An iterative / optimization-based veil estimate** instead of a closed-form
  percentile rule. Rejected: an iterative optimization would make the result
  depend on the initialization and on the convergence tolerance — direct poison
  for "the same pixels" reproducibility (`docs/pipeline.md` §5). A dark channel
  percentile is a **closed, deterministic selection**, freezable as it is with
  the process version, at ADR 0016's precision.
* **A literal large-radius Gaussian blur at full resolution** instead of an
  approximation by downsampling. Rejected: `gaussian_blur`'s `⌈3σ⌉` kernel
  (`process2.rs:396`) becomes prohibitive at the large σ clarity requires, on
  large previews and exports (`docs/engine-api.md` §11). A bounded approximate
  blur (downsampling / box filter / pyramid) gives the same broad contrast at a
  controlled cost; the approximation is part of the frozen rendering, and
  therefore reproducible.
* **Supporting regional dehaze, clarity and texture (under a mask) in V2.**
  Rejected: deferred to a future ADR built on ADR 0029's mask infrastructure —
  the same cut as ADR 0031 for regional colour grading. The global version
  stands alone and can ship without masking; the regional version will build on
  it when the time comes, in the common frame of ADR 0026, without being
  rewritten.
* **Inserting the three stages after ADR 0029's "Local adjustments" stage**
  instead of before Vibrance/Saturation. Rejected: clarity and texture belong
  to the presence block and dehaze follows the coarse tonal sliders, all before
  the colour block (`docs/v2-scope.md` §6). Placing them after ADR 0029 would
  move its stage's position and change which pixels feed masking. The two
  features insert at distinct fixed positions, with no reconciliation (ADR
  0028): whichever ships first imposes nothing on the other.
