# ADR 0165 — Colour moiré, globally first

**Status:** Accepted — 2026-10

## Context

Moiré is the first gap of the Phocus parity report, and the one Leyline has
answered least: its only lever is the DCB demosaic of
[ADR 0061](0061-demosaic-algorithm.md), which reduces false colour at the
demosaic and does nothing for a pattern already in the frame — a shirt, a
screen, a roof shot from far enough away. Lightroom has a *Moiré* slider on
its local tools; Phocus and Capture One have a global one.

[ADR 0108](0108-local-texture-clarity-sharpness-noise.md) refused moiré on a
mask for a reason of order, not of principle: « Leyline has no such stage
globally, so there is nothing to re-parameterize. A global stage first; then
this question becomes trivial. » This ADR is that global stage.

Two kinds of moiré must be told apart, because only one can be removed
without knowing the scene:

* **Colour moiré** — bands of false hue where a fine pattern aliased against
  the colour filter array. The luminance underneath is right; the chroma
  oscillates where nothing in the scene does.
* **Luminance moiré** — the pattern itself, aliased into lower frequencies.
  It is made of the same stuff as detail: no local operator can tell a false
  stripe from a real one.

## Decision

### 1. Colour moiré only, as a stage of its own

A stage `moire`, version 1, at **rank 7**: just after the measured noise
reduction (luminance at 5, colour at 6) and before anything that resamples
the image (camera profile 10, lens 20). Moiré is a sensor artefact; it is
treated next to the other sensor artefacts, and before a distortion
correction bends its bands into shapes that are harder to tell from the
scene.

Luminance moiré is **refused** (see Rejected): an operator that removes
false stripes removes true ones.

### 2. The operator: chroma guided by luminance

On the display axis, like the colour noise reduction
([ADR 0046](0046-edge-preserving-denoise.md)), each channel's
deviation from luma is filtered by a **guided filter** (He, Sun & Tang,
2010) whose guide is the luma plane:

* in a window, the chroma is modelled as a linear function of the luma,
  `C ≈ a·Y + b`, fitted by least squares with a regulariser `ε`;
* what the luma explains — a red jacket's edge, where colour and brightness
  change together — is kept;
* what it does not — chroma that swings back and forth over a flat luma,
  which is what colour moiré *is* — is replaced by its local mean.

One filter alone is not enough, and the first measurement said so: on a
real photograph at full strength, a window holding green leaves, a dark
background and a brown stem cannot be one linear function of luma, and the
stem turned grey. So the filtered chroma is **believed in proportion to how
much of the window's detail is chroma rather than luma**:

```
belief = var(C) / (var(C) + 4 · var(Y))
C'     = C + belief · (guided(C) − C)
```

Bands over a flat grey have chroma variance and next to no luma variance:
belief near 1, flattened. A thin coloured branch has edges in luma too:
belief small, kept. On that photograph the mean change at full strength fell
from 2–3.5 to 0.5–0.9 levels out of 255, and the stem kept its colour.

The radius grows with the slider: `r = max(1, round(12 · k · scale))`
pixels at `k = strength / 100`, multiplied by the preview's `scale` like
every radius ([ADR 0041](0041-interactive-preview-rendering.md)). `ε = 10⁻³` on the
display axis, luma weight `4`. All three constants are frozen with the
version.

Box means are exact sums over the window clipped at the image border,
accumulated in `f64`: no drift, no dependence on the number of threads.

### 3. The setting

`moire: i32` in `[0, 100]`, `0` meaning the stage does not run. A slider in
the **Detail** group next to the noise reduction it neighbours in the
pipeline; in presets, the **Detail** group. CLI `develop … moire <0-100>`;
MCP through `apply_edits`.

### 4. No detection, no default

Nothing turns it on. Phocus detects moiré and proposes a correction;
[ADR 0105](0105-detector-conformance-and-cli.md) §2 and the precedents gathered under
*automatic tools are a choice* say why Leyline does not: a false positive
damages a photograph nobody was looking at. A photographer who sees bands
moves the slider.

## Consequences

* One stage, one module, one golden case; no migration.
* A colour edge at **equal luminance** — a saturated red against a green of
  the same brightness — is softened at high strength, because the guide
  cannot see it. It is the known limit of every luma-guided chroma filter;
  the slider's hint says *colour bands*, not *colour*.
* The mask question of ADR 0108 is now the trivial one it promised: a
  `moire` value on a local adjustment is a later slice, with this operator.

## Rejected

* **Luminance moiré.** §1: false and true stripes are the same signal.
* **A wider window instead of a guide** (a chroma blur): it bleeds every
  colour edge in proportion to the radius, which is what makes a moiré tool
  feared.
* **Automatic detection** — §4.
* **At the demosaic.** The demosaic is chosen per revision
  ([ADR 0061](0061-demosaic-algorithm.md)) and is LibRaw's; a moiré control
  inside it would be one per algorithm, and would not reach a JPEG or a TIFF,
  whose moiré is already baked in.
