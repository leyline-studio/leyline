# ADR 0041 — Interactive rendering: a preview at display resolution, and a pipeline stage cache

**Status:** Accepted — 2026-07

## Context

`docs/roadmap.md` phase 7 ("CPU/GPU optimizations") is the current phase. Two
measurements taken on 2026-07-25
(`crates/leyline-engine/benches/process1.rs`, group `process11`, a synthetic
3 Mpx image, i9-9900K with 16 threads) frame the problem:

* the cost of an isolated slider ranges from **~0 ms** (tone curve, colour
  grading, profile matrix) to **+232 ms** (a brush mask at 64 dabs), **+112 ms**
  (dehaze), **+94 ms** (eight spots), **+70 ms** (noise reduction);
* a complete render of a real CR2 (3888×2592) into a `Small` preview takes
  **0.97 s** at neutral and **2.39 s** with every slider active, decoding
  included.

Two structural wastes explain most of that gap, and **neither is an operator
problem** — each operator taken in isolation is already reasonable:

**1. The pipeline runs at a resolution that is never displayed.**
`preview::plan_preview` decodes at `half_size` for the `Thumbnail` and `Small`
classes, then develops **the whole decoded buffer** before
`leyline_preview::cache` reduces the result to `max_edge`. For a 10 Mpx body in
a `Small` preview, that develops 1944×1296 (2.5 Mpx) in order to display
1024×683 (0.7 Mpx): **~3.6× the pixels computed and then thrown away**, on
every slider move.

**2. Every render starts again from the decoded buffer.**
`process11::develop` rebuilds the complete chain on every call. A neutral stage
is skipped, but every **non-neutral stage upstream** of the one being changed
is recomputed for nothing: moving `sharpening` (the last stage, ~13 ms) re-runs
dehaze, clarity, texture, HSL and the local adjustments already computed
identically on the previous render. That is the structural difference from
Lightroom, Capture One and Darktable, which cache intermediate stages and
replay only what is downstream of the edited node.

A third avenue — **running the pipeline on the GPU** — was explicitly rejected
by **ADR 0012** ("determinism across GPUs not guaranteed; deferred to a later
phase-7 exploration"). The present ADR **does not reopen** that choice: it
addresses the two CPU wastes, which carry no determinism risk, need no new
dependency, and multiply with each other. The GPU question will be taken up
again with the post-optimization measurements, in its own ADR, if it still
warrants it.

## Decision

Both optimizations bear **exclusively on the preview path**. Export and
printing stay unchanged, at full resolution, with no stage cache, **bit for
bit identical to today**. There is therefore **no new process version**: the
process version describes what the render contract produces
(`docs/pipeline.md` §5), and that contract does not move.

### 1. The preview is developed at its display resolution

`plan_preview` stops developing the decoded buffer and develops a buffer
already reduced to the requested class's size (`leyline_preview::max_edge`).
The resizing moves **before** the pipeline instead of after.

The exact order becomes: decode (`half_size` unchanged) → reduce to `max_edge`
→ develop → encode. `PreviewKind::Full` has no `max_edge`: that path is
unchanged, and develops at full resolution as it does today.

> **A sequel, 2026-08-05.** This paragraph says nothing of what becomes of the
> reduced buffer: it was rebuilt on every render, which became the dominant
> cost once §3 was in place. [ADR 0076](0076-proxy-cache.md) caches it
> alongside the decode.

### 2. Radii expressed in pixels are scaled to the proxy

Developing a reduced image with unchanged radii would give a **wrong**
rendering, not merely a fast one: a blur of σ = 40 px on a buffer 3.6× smaller
covers 3.6× more subject. Every parameter denominated in pixels is therefore
multiplied by the scale factor `s = proxy_width / decoded_width`:

| Parameter | Where | Unit |
| --- | --- | --- |
| `sharpening.radius` | a user parameter | σ pixels |
| `CLARITY_RADIUS` (40.0) | a stage constant | σ pixels |
| `TEXTURE_RADIUS` (6.0) | a stage constant | σ pixels |
| `DEHAZE_PATCH_RADIUS` (7) | a stage constant | a pixel radius |
| the noise reduction's σ (`k·2.0`, `k·3.0`) | derived from the strength | σ pixels |

The other spatial parameters are already **normalized** to `[0, 1]` and
therefore scale-invariant: crop, rotation, radial/gradient/brush masks, spots
(`spot.radius × max(w, h)`), lens correction (geometry in normalized
coordinates). They are untouched.

That scaling is an **approximation**, not an identity: reducing and then
blurring at σ·s does not equal blurring at σ and then reducing. For a Gaussian
the difference is small and errs in the right direction. The important point is
that today's preview is **already** an approximation of the export — it
develops at 2.5 Mpx and then reduces to 0.7 Mpx, which for instance makes a
1 px-radius sharpening vanish. The proxy preview introduces no new infidelity:
it replaces one with another, less costly and closer to what the export will
give at equal display size.

### 3. The preview pipeline caches intermediate stages

Preview rendering gains a cache of **intermediate states**, in memory, held by
the `Library` alongside the decode cache, and discarded with it.

> **An amendment of 2026-08-02, at implementation time.** This paragraph said
> "held by the open edit session". That was untenable: Studio's develop view
> renders through `Library::preview`, never through an `EditSession`, so that
> a cache carried by the session would never have been touched by the very
> interaction it targets. It therefore lives where `DecodeCache` already
> lives. Nothing else changes — the cache stays purely derived, specific to
> the preview path, and discardable at will.

The pipeline is a **linear sequence** of stages. Each checkpoint retains
`(stage index, a fingerprint of every upstream stage's settings, buffer)`. On
every render, the engine computes the prefix fingerprints, keeps the **deepest
valid checkpoint**, and replays only what is downstream. Moving `sharpening`
with dehaze and clarity active then recomputes `sharpening` alone.

Checkpoints are not placed at every stage — the memory cost would not justify
it — but **ahead of the expensive stages**, where the gain pays for its copy:
after lens correction and spots (expensive, and almost never adjusted in rapid
succession), after the tonal block, after clarity/texture/dehaze, after the
local adjustments. At proxy resolution a buffer costs ~8 MB (0.7 Mpx × 3
channels × `f32`), some thirty megabytes for the set: acceptable for a session,
and one more reason for this cache to **exist only on the preview path**, never
in export where full-resolution buffers would make it prohibitive.

**A threshold designates a position, not a stage.** A checkpoint is placed
ahead of the first stage whose rank reaches the threshold, never in front of an
exact rank: the stage occupying that rank is often neutral, and therefore
absent from the plan. Measurement showed it — aiming at the exact rank, three
of the four checkpoints were never taken, and the gain fell to 10 %.

**Measured on 2026-08-02** (a 1024×683 test card, with the tonal block plus
clarity, texture, dehaze and sharpening active, `--release`): moving the
sharpening slider, the plan's last stage, goes from **~60 ms to ~14 ms**, that
is **−78 %**. That is the gain the §Context announced.

The cache is purely **derived**: discarding it at any moment changes no pixel,
only the render time. That is what makes it safe — it cannot introduce state
inconsistency, at worst slowness.

### 4. What does not change

* **The reproducibility contract** (`docs/pipeline.md` §5): unchanged, it
  bears on export rendering.
* **Process versions**: none new. A preview is not a revision.
* **The frozen `processN.rs` modules** (ADR 0028): radius scaling is applied
  **by the caller** (the preview planner), which passes a scale factor; it
  does not rewrite the frozen mathematics of an existing process module.

## Consequences

* **The cost of a slider move falls by two independent factors that
  multiply**: ~3.6× fewer pixels, and skipping every unchanged upstream stage.
  On the worst case measured (a brush mask, 255 ms at 3 Mpx), the two together
  bring the latency class back into the same band as ordinary sliders.
* **The preview is no longer the same computation as the export.** That was
  already true (see §2) but it becomes an **owned and documented** property
  rather than a side effect of the final resizing. An accepted corollary: a
  flaw visible only at full resolution (fine noise, a sharpening halo) is not
  judged on a reduced preview — that is what `PreviewKind::Full` is for.
* **A scale factor now travels through the internal render API.** That is one
  more input to pass correctly; the wrong factor gives a wrong rendering and
  not an error, so it is covered by dedicated tests (a proxy render and a
  reduced full-resolution render must stay close within a given tolerance).
* **The stage cache adds mutable state to the edit session.** It is derived
  and disposable, hence free of corruption risk, but the prefix fingerprint
  must be **exhaustive**: a setting forgotten from the fingerprint would
  produce a stale rendering. That is this ADR's only real correctness risk, and
  it is directly testable (changing each parameter in turn must invalidate the
  cache).
* **The GPU question stays open, and better posed.** These two optimizations
  remove *useless* work; a GPU accelerates *useful* work. Measuring them first
  avoids porting to GPU a pipeline that was computing 3.6× too many pixels —
  and will give the figures ADR 0012 lacked in order to decide.

## Alternatives rejected

* **Moving the pipeline to the GPU (wgpu/OpenCL) right away**: rejected by
  ADR 0012 for determinism across GPUs, and premature here — it would optimize
  a pipeline that structurally does too much work. To be taken up after
  measurement, in its own ADR.
* **Progressive rendering (showing a coarse version and then refining)**: it
  masks the latency instead of reducing it, and doubles the render paths to
  maintain. The display-resolution proxy gives the same feel with no second
  path.
* **A stage cache persisted to disk**: rebuilding a stage costs less than
  serializing and re-reading it at the sizes in play, and it would introduce a
  cache artefact to invalidate between engine versions — everything
  `docs/pipeline.md` avoids by storing settings alone.
* **Caching a single state (the decoded buffer)**: that is exactly the current
  state (`DecodeCache`) — it avoids re-decoding, not re-computing.
* **Developing the preview at `max_edge` without scaling the radii**: faster
  and **wrong**; the detail and local-contrast sliders would no longer mean the
  same thing from one preview class to the next.
