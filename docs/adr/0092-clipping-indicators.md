# ADR 0092 — Clipping indicators: the histogram's two triangles and J

**Status:** Accepted — 2026-08

## Context

Lightroom's histogram carries two small triangles; lit, they say a channel
touches the end of the range, and clicked (or `J`), the clipped highlights
paint red over the image and the crushed shadows blue. Leyline's develop
panel has the histogram and no clipping answer at all — the one thing a
photographer checks *while* moving Exposure and Whites.

The develop preview reaching the panel is an 8-bit sRGB render, and the
histogram already computed from it holds bins 0 and 255. Everything a
clipping indicator needs is therefore already in the client's hands.

## Decision

### 1. A view of the render, computed by the client

Clipping is **display-referred and pure presentation**: a pixel is a
clipped highlight when a channel of the rendered preview sits at 255, a
crushed shadow when all three sit at 0. The scan and the painting are one
pure function in Studio (`develop::paint_clipping`), unit-tested like the
other layout rules; the engine gains nothing, stores nothing, and
`docs/pipeline.md` §5.1 is not in play — this is the same nature as the
mask overlay (ADR 0071) and the soft proof, one layer further out: not
even a render of its own, a reading of the render already on screen.

Raw-referred clipping (what the *sensor* blew, before recovery) is a
different, better question and is **not** answered here: it needs the
pre-pipeline buffer the client never holds, so it would be an engine view
with its own ADR the day it is asked for.

### 2. The triangles tell, the overlays show, J speaks both

Each triangle lights when its end of the histogram is occupied, whether or
not the overlay is on — the *telling* is free, it reads the two end bins.
Clicking a triangle toggles its overlay; `J` toggles both, together, from
either state — the Lightroom muscle memory. The painted colors are the
convention's: red for blown highlights, blue for crushed shadows.

### 3. Precedence among the diagnostic views

The mask overlay already replaces the develop image while a mask row is
selected; clipping paint would stack a second diagnosis on the first and
make both illegible, so the mask overlay **wins** and clipping is skipped
while it shows. The soft proof, by contrast, is the image being judged —
clipping paints *over* the proofed render, which is exactly the question a
proof asks ("what does the destination lose?").

## Rejected

* **An engine-side clipping mask render** — a fourth view plumbed through
  the render path to compute what two histogram bins and a byte scan
  already say. The engine earns its place when the client lacks the data;
  here it does not.
* **Painting on the live drag frames** (ADR 0074's silent path) — the
  drag preview stays untouched diagnostics-free by that ADR's own rule:
  one honest repaint on release.
* **Distinct per-channel triangle colors** — Lightroom tints the triangle
  by the clipped channel; useful, but the bins say only *that* an end is
  occupied per channel, and painting five states into a 10-px triangle is
  decoration before information. The white/lit state answers the real
  question; refinement can come back if asked for.
