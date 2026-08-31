# ADR 0098 — The tone curve, one per channel

**Status:** Accepted — 2026-08

## Context

ADR 0030 gave Leyline a point curve: a monotone cubic Hermite spline
applied identically to every channel — a *luminance* curve. Lightroom's
Tone Curve panel offers the same curve plus one per channel (Red, Green,
Blue), and that is where a large part of colour grading actually happens:
lifting the blue channel's black point is the split-toned shadow every
film emulation starts from, and no combination of luminance curve, HSL
mixer and colour grading reproduces it — they act on hue and saturation,
not on a channel's transfer.

## Decision

### 1. `tone_curve::v2`, with `v1` untouched and reused

`ToneCurve` gains `red`, `green` and `blue` — three more point vectors,
each empty by default and each validated exactly like `points` already
is (at least two entries, strictly increasing `x`, both coordinates in
[0, 1]).

**When all three are empty, v2 delegates to v1.** Not "computes the same
thing": calls it. Bit-identity is then a property of the control flow
rather than an argument about floating point, and the golden manifest
proves it the way it proved ADR 0096's — the v1 and v2 entries of a case
whose per-channel curves are empty carry the same digest.

### 2. Master first, then the channel

A sample passes through the luminance curve exactly as v1 applies it,
then through its own channel's curve. That order is the one the panel
implies — the master curve is the contrast, the channel curves are the
colour laid over it — and it is what makes the master curve keep meaning
what it meant when a channel curve is added afterwards.

Both act on the **display axis**, like v1: a curve is a statement about
the histogram the photographer is looking at, and the same control points
in linear light would put their midpoint in near-blackness. That is
ADR 0048 §3's reasoning, applied to the operator it was first written
about.

Above white, each channel is scaled by its own curve's value at 1.0 —
v1's headroom rule (`display_curve`), applied per channel rather than
once. A highlight above the display white keeps its headroom instead of
being crushed onto the curve's endpoint (ADR 0044 §2).

### 3. The capability rule, again

A non-empty `red`, `green` or `blue` on a revision pinning `tone_curve`
v1 is refused by `validate()`, naming the stage, the version it needs and
the reprocessing that fixes it. Third application of the rule in this
run, unweakened.

### 4. Clients

Studio: the curve editor gains a channel selector — four chips above the
canvas, the edited curve being whichever is selected, and the canvas
tinted by the channel so the state is visible without reading the chip.
CLI: `tone-curve` takes an optional channel name before the points
(`leyline develop … tone-curve red 0,0 0.5,0.6 1,1`), absent meaning the
master curve, so every existing invocation keeps its meaning.

## Rejected

* **A parametric curve** (Lightroom's Highlights/Lights/Darks/Shadows
  region sliders) — a second way to say what the point curve already
  says, and the sliders it would duplicate (`highlights`, `shadows`)
  exist at rank 60. Refused, not deferred.
* **Per-channel curves in linear light** — §2: the control points would
  stop meaning what the histogram shows, and the panel would need a
  second mental model for the same gesture.
* **Applying the channel curve before the master** — a channel curve
  would then be re-mapped by the master, so moving the master would
  silently change what a channel curve does.
