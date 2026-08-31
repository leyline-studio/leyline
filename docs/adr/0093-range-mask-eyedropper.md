# ADR 0093 — The range-mask eyedropper: pointing at what the band should hold

**Status:** Accepted — 2026-08

## Context

ADR 0049 shipped the local-adjustment tools and named its own leftovers;
one was **range selection by eyedropper** — click the sky, get the
luminance band; click the leaf, get the hue band — deferred because it
needed sampling infrastructure nobody had built. ADR 0091 has since built
exactly that for the white-balance picker: click on the proxy render,
mean of the 5×5 neighbourhood, an engine that proposes and a session that
writes. This ADR is that consequence coming due.

One honesty question decides the shape. The frozen `local_adjustments`
modules read their range terms from the working buffer *at rank 160*, on
the display axis; the pixel the user clicks is the **final** render,
after sharpening, vignette and output rendering. The two are close and
not identical.

## Decision

### 1. A proposal measured on the final render, said plainly

`Library::sample_range(asset, x, y)` returns the display-axis luminance
and the hue of the 5×5 mean around the click — read from the same proxy
render the picker of ADR 0091 reads, because that is the image the user
is pointing at. The gap between rank 160 and the final render is
accepted and stated rather than hidden: **the eyedropper proposes a
band's starting point, the sliders own its truth**. A range is a band
with softness, not a key; a starting point a few percent off is corrected
by the falloff the model already has, and the sliders are right there.
Like `auto_tone` and `neutralize_wb`, it writes nothing and touches no
stage: §5.1 untouched by construction.

### 2. What a click writes

Through the ordinary session, one commit:

* **Luminance band**: full coverage from the sampled luminance ±0.1
  (clamped to [0, 1]), existing softness kept, default softness if the
  term was off. Ten percent either side is wide enough to survive the
  rank-160 gap and narrow enough to mean the thing clicked.
* **Hue band**: center at the sampled hue, existing width and softness
  kept (defaults if off). The click moves the *center* only — width is a
  statement about tolerance, and the eyedropper has no opinion on it.

A click on a near-grey pixel still centers the hue band (some hue always
computes); it is the width sliders' job to make that band mean anything.
Turning the term on is implicit — pointing at a luminance *is* asking for
a luminance band.

### 3. Three clients

Studio: a `Sample` chip beside each range term arms the eyedropper; one
click on the image writes the band and hands the tool back to Select.
CLI: `leyline sample-range <library> <version-id> <x,y>` prints the two
numbers, for scripts that write `LocalAdjustment` payloads themselves
(ADR 0049 §4 made the payload the CLI surface; the sample is the missing
measurement, not a new payload). SDK: `sample_range` re-exported.

## Rejected

* **Sampling the rank-160 buffer exactly** — an engine render path that
  stops mid-pipeline, plumbed through the stage cache, to improve a
  starting point by a few percent that the softness absorbs anyway.
  Reconsider only if real photographs show proposals landing visibly off.
* **The eyedropper setting the width too** (narrow band on a clean
  sample, wide on a noisy one): a guess dressed as a measurement, and it
  would overwrite a width the user already chose every time they re-click
  the center.
