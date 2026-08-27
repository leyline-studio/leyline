# ADR 0046 — Edge-preserving denoising: à-trous wavelets and soft thresholding (`noise_luminance::v2`, `noise_color::v2`)

**Status:** Accepted — 2026-07
**Followed by:** the two stage versions this ADR creates are no longer the
current ones. [ADR 0072](0072-measured-noise-profile.md) delivers
`noise_luminance::v3` and `noise_color::v3`, which replace §3's universal
threshold with a **per-pixel** threshold derived from the sensor's measured
variance, and which also change **rank** — 5 and 6, in linear light, instead of
the 170 and 180 §1 fixes. The operator itself — à-trous wavelets, soft
thresholding per scale — is unchanged and is still described here.

## Context

The denoising that shipped is, literally, a blur.

`stages/noise_luminance/v1.rs` blends the luma plane towards its Gaussian blur
(`add_luma_delta(px, |i| k * (blurred[i] - plane[i]))`, σ = `k · 2 · scale`),
and `stages/noise_color/v1.rs` does the same on each channel's departure from
the luma. No term depends on the local content: an edge and a flat area receive
exactly the same treatment. Raising the slider removes noise **and** detail, in
the same proportion, which caps the tool very low — beyond fifteen or so it no
longer denoises, it softens.

That is the engine's most serious gap against the software on the market, and
it lands on the usage profile those programs advertise first (wildlife, sport,
astrophotography, weddings without flash — all at 6400 ISO and beyond). A RAW
developer whose denoising is a Gaussian blur is unusable on those images.

**What is not at issue.**

* **The exclusion of AI** (`docs/specification.md` §4). It is neither
  circumvented nor renegotiated: what follows is a deterministic, closed
  operator, with no learned model, no weights to embed, and no inference.
  Learned denoisers (DeepPRIME, Topaz) stay out of scope and the present
  document does not claim to match them (§7).
* **The reproducibility contract** (`docs/pipeline.md` §5.1). The rendering of
  `noise_luminance::v1` and `noise_color::v1` does not move by a bit: the fix
  is a **new stage version**, which
  [ADR 0042](0042-versioned-stage-pipeline.md) §1 requires and makes cheap —
  a few dozen lines beside `v1`, and not a twelfth copy of the pipeline.
* **The settings surface.** Two `0..100` sliders
  (`noise_reduction.luminance`, `noise_reduction.color`), `settings_json`
  unchanged, `schema` not incremented. No client (Studio, CLI, SDK) has a field
  to add: they are the same values, better spent.

## Decision

### 1. Two new stage versions, at unchanged ranks and in the same space

`noise_luminance::v2` at rank 170, `noise_color::v2` at rank 180, both in
`LinearRec2020`, with the same `active` predicates as their `v1`s. The `v1`s
are untouched and stay registered: a revision citing them renders identically,
forever. A new revision pins `v2` through the ordinary mechanism
(`Stage::current`), with no correspondence table and no special case.

### 2. The operator: à-trous wavelets, soft thresholding per scale

A **non-decimated** (*à trous*) decomposition of the treated plane by the
separable B3-spline kernel `[1, 4, 6, 4, 1]/16`, with a hole spacing of `2^l`
at level `l`, and replicated borders:

```
a₀ = plane
a_{l+1} = B3(a_l, spacing 2^l)      d_l = a_l − a_{l+1}
denoised plane = a_L + Σ threshold_l(d_l)
```

The thresholding is **soft**: `sign(d) · max(|d| − t_l, 0)`.

Two properties make it superior to `v1`'s blur, and they are the two reasons
for choosing it:

* **Noise does not have one scale, it has several.** A blur at a single σ can
  attack only one: tuned to fine grain, it leaves the chroma blotches; tuned to
  the blotches, it destroys detail. The decomposition explicitly separates the
  scales and applies its own threshold to each.
* **Preserving edges requires no edge detector.** An edge produces coefficients
  large relative to the threshold, which therefore pass through the operator
  (reduced by `t_l`, not crushed); noise produces small coefficients, which
  fall to zero. No heuristic, no sensitivity parameter, nothing for the user to
  tune.

The residual `a_L` — structure coarser than the last analysed scale — is
**never** thresholded: denoising does not touch the image's tonality.

### 3. Thresholds: the kernel's noise profile, scaled by the slider

For white Gaussian noise, the decomposition above concentrates the energy in
the first levels, with known standard deviations per level
(`0.890, 0.201, 0.086, 0.041`). The threshold follows that profile:

```
t_l = k · BASE · σ_l          k = strength / 100
BASE_LUMA = 0.05              BASE_CHROMA = 0.12
```

`BASE_CHROMA` is more than double because chroma noise is at once more visible
and less informative: a photo's chrominance is spatially smooth almost
everywhere, and an aggressive threshold costs far less there than in luminance.
It is the same asymmetry `v1` expressed through its σ values (2 and 3), but
placed where it means something.

Those three constants, the kernel and the number of levels are **frozen** under
§1: changing them would be a `v3`.

### 4. The display axis is kept

Like `v1`, both stages work under `in_display` (ADR 0044). That is not a
copying reflex: noise visibility is perceptual, a constant threshold in linear
light would be enormous in the shadows and negligible in the highlights, and
both sliders were calibrated on that axis. The headroom above white passes
through the operator without being clipped, `in_display` already taking care of
it.

### 5. Proxy rendering: it is the number of levels that carries the scale

Level `l` analyses structures on the order of `2^l` pixels. On a preview
reduced by a factor `scale` (ADR 0041), analysing the same *physical*
structures therefore means removing levels, not shrinking a radius:

```
levels = clamp(1, 4, 4 + ⌊log₂(scale)⌋)
```

A preview reduced 4× analyses 2 levels, which covers the same details of the
image as 4 levels on the full render. That is the exact translation, for a
multi-scale operator, of what radius-based stages do by multiplying by `scale`.

### 6. The shared body goes into `kernel::v2`

The transform and the thresholding serve both stages, and therefore can live in
neither: they go into `kernel::v2`, frozen on the same footing as `kernel::v1`
(ADR 0042 §1, point 2). `kernel::v1` is reused as it is for `in_display`,
`luma_plane` and `add_luma_delta` — calling a frozen module from a new module
is safe by construction, since the frozen one will never change.

### 7. What this decision does not claim

Stated here so that nobody has to infer it from a silence:

* **No parity with learned denoisers.** DeepPRIME XD3 and Topaz Wonder
  reconstruct plausible detail; thresholding reconstructs none, it merely
  refrains from destroying what is there. The gap narrows markedly, it does not
  close.
* **No per-body, per-ISO noise profile.** The threshold is a uniform white-noise
  model, not the sensor's measured variance at that sensitivity (what darktable
  does through its profiles). That is this operator's natural sequel, it
  requires a database of measurements as Lensfun has one, and therefore its own
  ADR.
* **No masked denoising.** Like clarity and dehaze in ADR 0033, both sliders
  stay global.

## Consequences

* **Nothing migrates, nothing breaks.** Existing revisions cite `v1` and render
  `v1`. A user who wants the new denoising reprocesses their photo
  (`docs/pipeline.md` §4.5), which creates a new revision — the old one stays
  renderable.
* **The reference renders gain entries, and none of them moves.** The golden
  manifest sees the `noise_*::v2` variants appear; the entries citing `v1` must
  stay bit-identical, and that is precisely what proves §1. The blessing is
  additive by construction.
* **CPU cost rises, by a measured factor.** Four levels means eight separable
  5-tap passes per plane, against a pair of Gaussian passes; chrominance
  processes three. The `denoise/` bench compares the two pinned versions, at
  identical settings (luminance 40, chroma 30) on the synthetic 3 Mpx frame:
  **103 ms for `v1`, 223 ms for `v2`** — a complete render, not the operator
  alone. The ~2.2 factor is paid on a stage that was cheap only because it was
  not doing the work. The operator stays O(N) per level and parallel by rows,
  and the preview pays less of it: fewer levels at reduced scale (§5).
* **`docs/pipeline.md` §3.3 stops being uniform.** The stage table said "all at
  version 1" since ADR 0043: `noise_luminance` and `noise_color` take a second
  row in it. It is the project's first real `v2`, and therefore the first time
  ADR 0042's mechanism serves outside the test fixture stage.
* **The gap announced in the competitive review narrows where it was widest**,
  without touching a scope exclusion.

## Alternatives rejected

* **A bilateral filter.** It preserves edges, but stays **single-scale** —
  which is `v1`'s limit we are trying to lift, and broad chroma blotches escape
  it. O(r²) per pixel into the bargain, where the transform is O(N) per level.
* **A guided filter.** O(N) and edge-preserving, but single-scale too, and a
  guide image must be chosen — one more parameter for a result that does not
  dominate multi-scale thresholding on the case that motivates the ADR.
* **Non-local means.** Better potential quality, deterministic, but the patch
  search puts it out of reach on CPU at 24+ Mpx for interactive rendering (ADR
  0012 having rejected the GPU).
* **A learned denoiser.** Excluded by scope, and it would presuppose embedding
  weights — which the project does not do.
* **Fixing `v1` in place.** Forbidden by ADR 0042 §1: the rendering of a
  published version does not move. That is also what makes the present ADR
  cheap.
* **Waiting for measured noise profiles** so as to ship only once. Uniform
  thresholding is already far above a blur, and nothing in the profiles would
  invalidate the transform: they would change the thresholds, hence a `v3`
  later, not a step backwards.
