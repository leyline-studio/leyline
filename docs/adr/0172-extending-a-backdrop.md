# ADR 0172 — Extending a backdrop, without a model

**Status:** Accepted — 2026-11 · engine and CLI built (§6); Studio not yet

## Context

A photographer working in a dance school, a gym or a rented hall puts
the subjects in front of whatever backdrop fits in the car, and the
frame is wider than the backdrop. Three real sessions show three
versions of the same problem:

* **A painted muslin backdrop** (`2026_11_11`, 412 frames): mottled
  blue-grey, lighter in the centre, darker towards the edges, with a
  second cloth of a different blue on the floor. Beside it, in most
  frames: the white wall, the **feet of the two stands**, sometimes the
  crossbar, a strip of grey floor.
* **Black stage curtains** (`Photos 60D 2025-11-06`, group shots): a
  black molleton with **vertical folds**, a radiator, a wall and the stands
  at the sides.
* **No backdrop at all** (`Photos 60D 2023-11-23`): blue gym crash mats
  standing against a yellow wall, the mats' vertical seams, a printed
  **logo** on the mats, a parquet floor.

The request is the one a photographer would make in Photoshop: remove
everything that is not the backdrop and extend the backdrop to fill the
frame. Today Leyline offers the crop, which gives away the margin around
a pose (a split, a hoop), and the clone of
[ADR 0032](0032-spot-removal-clone.md), which is a frame-by-frame job and
needs a source as big as what it covers.

A local adjustment cannot do it: every value of `LocalAdjustmentValues`
*modifies* what is there; none *replaces* it.

### What a prototype measured

Before this ADR, a Python prototype ran on two frames of the first
session (`5D4_2740`, `_MG_9881`), at 1600 px, with the subject coverage of
the existing `leyline-assist` detector (ADR 0073). Four findings shaped
the decision:

1. **Finding the backdrop by its colour fails.** The muslin is barely
   saturated (median `b − r` ≈ 0.05, saturation ≈ 0.10), and the wall's
   hue shifts with the light (from neutral to warm between two frames).
   At the first try, the whole backdrop was taken as "to fill". The
   same thing with **a polygon traced around the backdrop** worked first time.
2. **A flat colour or a gradient is visibly fake** on a mottled cloth.
   What reads as believable is the cloth's **slow field** (the light
   falloff) extrapolated outwards, plus a synthetic mottle with **the
   same energy per frequency band** as the real cloth.
3. **Energy must be measured robustly.** A plain standard deviation
   counted the folds and the hem as mottle, and the fill came out about
   twice too contrasted. A median absolute deviation fixed it.
4. **The subject detector is necessary and imperfect.** It saves every
   dancer's arm that crosses the backdrop's edge, and it also keeps bits of
   a stand touching a hand, and loses a hoop. The coverage needs a
   correction by hand in both directions.

The other two sessions add what the first did not show: the folds of a
curtain and the seams of the mats are **oriented**, so the mottle must be
too; and a backdrop may sit on a floor that must not be extended
(parquet), so **what is extended** has to be said, not inferred.

## Decision

### 1. A new stage, `backdrop` v1, at rank 32

Not a version of `spot_removal` and not a value of `local_adjustments`:

* `spot_removal` copies existing pixels from a source of the same size.
  Here, there is nothing to copy: the strip beside a backdrop is often
  wider than any clean patch of cloth.
* `local_adjustments` runs at rank 160, after the tone curve, and
  modifies. A fill must be measured and placed **before** the tonal block,
  so that white balance, exposure and every later setting develop the
  extension and the real cloth **alike**. Raise the exposure after the
  fill, and both move together.

Rank **32**: after `lens` (20, so vignetting is already corrected and the
slow field is measured on the real light), after `spot_removal` (30, so a
stand pole cloned out of the cloth is gone before the cloth is measured),
before `red_eye` (35) and `gains` (40). The buffer is linear Rec. 2020
(ADR 0044); measuring and synthesising both happen in that light.

This is the **first stage that invents pixels** (ADR 0109 was careful to
invent none). It stays inside the free pipeline because it is defined
entirely by the revision and by code in the tree: no model, no weights,
no seed stored. [ADR 0102](0102-paid-extensions-and-the-pixel-boundary.md)
is untouched: its rule is about what an extension may supply, and nothing
here can be bought or is needed to render.

### 2. The settings: regions, each saying what it extends and where

```json
"backdrop": {
  "regions": [
    {
      "source": [{"x": 0.037, "y": 0.0}, {"x": 0.777, "y": 0.0},
                 {"x": 0.80, "y": 0.762}, {"x": 0.986, "y": 0.788},
                 {"x": 1.0, "y": 1.0}, {"x": 0.0, "y": 1.0}],
      "zone": "outside_source",
      "holes": []
    }
  ],
  "protect": [
    {"mask": {"type": "coverage", "path": "Masks/….png", "checksum": "blake3:…"},
     "within": "everywhere"}
  ],
  "release": [],
  "feather": 0.003,
  "texture": 100
}
```

* **`regions`**: an ordered list. A region's **`source`** is a polygon in
  ADR 0026's referential (rotated, not yet cropped, normalised), the area
  that is **measured**. Its **`zone`** is what is **replaced**: either
  `"outside_source"` (the frame minus the polygon, the common case, and
  the default), or `{"polygons": [...]}`. **`holes`** are polygons *inside* the source that are
  replaced too: a logo on a mat, a socket on a wall. Where two zones
  overlap, the later region wins.
  * The muslin session needs one region (cloth + floor cloth in a single
    polygon: the slow field passes from one to the other on its own).
  * The gym session needs two: the mats extended sideways above the
    parquet line, and the parquet extended sideways below it, each measured
    on its own source, so that wood never bleeds into the mats.
* **`protect`**: masks whose union is never replaced, the subjects.
  They use the existing `Mask` shapes, including a stored coverage
  (ADR 0070) written by a detector (ADR 0073). Each says **`within`**
  which part of the frame it holds in: `everywhere`, `source` or
  `outside_source`. That is what lets a "main subject" coverage (which
  also catches accessories, and sometimes a stand) be kept on the cloth
  only, and a "people" coverage outside it, so a stand foot beside the
  cloth is never protected. **`release`**: masks
  subtracted from that union, the stand piece the detector stuck to a
  hand. Without `protect`, nothing is protected; detection is a gesture the
  photographer makes, never a default
  ([ADR 0084](0084-assisted-culling.md), 0088, 0105 §2).
* **`feather`**: the width, as a fraction of the long side, of the blend
  between the fill and what it meets.
* **`texture`**: 0–200, the mottle's amplitude as a percentage of the
  measured one. 0 leaves only the slow field.

An empty `regions` list is neutral: the stage is a no-op, bit-identical to
its absence.

### 3. What the stage computes

For each region, on the buffer as it reaches rank 32. Every length is a
fraction of the buffer's larger side.

1. **Known pixels**: the source, minus the protected subjects, eroded by
   the feather plus 0.2 % so neither the cloth's hem nor a subject's edge
   is measured. A region with fewer than 64 known pixels writes nothing.
2. **Slow field**: a normalised convolution of the known pixels (radius
   1.56 %), extrapolated into the zone by a push-pull pyramid (average
   down with the weights, fill the coarse level, bring it back up where
   the fine level had nothing), then smoothed (0.94 %). At a distance *d*
   from the edge it reads the cloth about *d*/2 inside: a soft mirror,
   which carries the lit centre and darker edges past the border without
   darkening the extension forever.
3. **Mottle, measured on the logarithm of the luminance**: a cloth's
   mottle multiplies the light falling on it. Measured additively in
   linear light, the first build painted blotches far too dark at the
   cloth's edge (§6). Five bands, box-blur differences at radii 0.0625 %,
   0.1875 %, 0.5 %, 1.25 % and 2.8 % (1, 3, 8, 20 and 45 px at 1600 px).
   For each band, the robust energy (median absolute deviation) over the
   known pixels, and its **anisotropy**: the ratio of the horizontal and
   vertical gradient energies, bounded to 16. These are statistics of the
   region, not fields: a floor cloth that must keep its own energy is a
   region of its own.
4. **Synthesis**: per band, a value noise that is a pure function of the
   position normalised to the larger side (so the same at any
   resolution, no seed stored), with cells stretched along the folds by
   the square root of the anisotropy, passed through **the very band
   filter that measured the source**, and scaled by its own deviation to
   the measured energy. The coarsest band is weighted ½: it also holds
   what is left of the lighting once the slow field is taken out. Each
   channel follows the luminance mottle by its own regression gain.
5. **Blend**: `slow field × exp(gain × texture × mottle)`, written into the
   zone with the feathered edge, the protected pixels untouched.

The preview and the export compute the same thing at two resolutions;
the box radii are rounded to whole pixels, so they are close, not
identical. Sums of the box blurs are `f64`.

### 4. Reproducibility

`pipeline.md` §5.1 holds by construction: the inputs are the decoded
buffer, the revision's polygons and masks, and a stored coverage pinned by
its BLAKE3. Nothing reads a clock, a random generator or the machine.

### 5. Clients

* **Studio**: a *Backdrop* tool in the rail. Trace the source polygon with
  handles (the handle machinery of ADR 0097), choose *outside the
  backdrop* or draw zones, add holes, then *Protect the subject*, which runs
  the detector on this frame, plus a brush in both directions. An overlay
  shows the zone in the colour of ADR 0071.
* **Sync across a series**: copying `backdrop` to other frames copies the
  regions, `feather` and `texture`, **never the `protect` coverage**,
  which belongs to one frame. Studio then offers *Protect the subject on
  N frames*, which runs the detector on each of them, as one explicit
  action. The polygons usually need a nudge when the camera moved. That is
  why handles matter more than automation here.
* **Presets**: excluded by default under ADR 0132 §6 (a backdrop polygon
  is a place in one frame), includable explicitly, like a crop.
* **CLI**: `backdrop` with a JSON payload, as local adjustments do
  (ADR 0049). **MCP**: through `apply_edits`.
* **GPU**: no kernel in v1. At rank 32 the stage sits before the first
  checkpoint, like `spot_removal` and `red_eye`, so it is not in
  `stages/gpu_coverage.rs`'s list of stages a drag cannot cross, and that
  test agrees.

### 6. What is built, and what the four acceptance frames showed

Built: the settings and their validation (the mask rules shared with
local adjustments), stage `backdrop` v1 with its unit tests and a golden
case, the preset category `backdrop` (last, so stored bit masks keep their
meaning), `Param::Backdrop`, the CLI (`develop … backdrop <json|@file|reset>`,
`detect … --backdrop [--within …]`) and the MCP group list.

The photographer named four frames of the first session as the
acceptance test: `_MG_9881`, `5D4_2740`, `_MG_0025`, `_MG_9869`, run
through the CLI end to end (import, polygon, detection, export):

* With the mottle measured additively in linear light, all four failed:
  dark blotches, reddish on `_MG_9881`. Hence §3's logarithm.
* With the logarithm, `5D4_2740`, `_MG_9881` and `_MG_9869` read as a
  continuation of the cloth at screen size. What remains visible comes from
  hand-typed polygons (a hem, the top bar), which Studio's handles are
  for, and a light halo where the detector's soft edge kept some wall
  around a hand.
* `_MG_0025` fails, and not because of the stage: the detector protects
  the left dancer and only the legs of the right one, whose head and arm
  overlap the wall and were filled. A brush typed by hand did not save
  it. **The subject mask is the first problem to solve**, before any
  refinement of the fill.
* Cost: about 20 s per full-resolution 5D IV export with the stage
  active, on all cores. To be brought down (the slow field and the
  statistics do not need full resolution) before Studio's slice, where it
  would run on every committed render.

## Consequences

* A whole session shot in front of a too-narrow backdrop can be finished
  in Leyline, still non-destructive and still developable.
* Polygon geometry enters the settings for the first time; `reshape`'s
  and the masks' coordinate rules apply unchanged.
* The quality ceiling is stated: the fill is a **plausible continuation of
  a soft texture**. Mottled muslin, black molleton and plain mats fit it.
  Regular structure does not: **parquet planks**, a mat's straight
  **seams**, a brick wall. Extended this way they come out smooth. For those,
  and for very large areas (a wall occupying half the frame), the answer
  is either the crop or a generative fill as a **derived asset** through
  `leyline-assist` (ADR 0102, 0107), never a stage.
* Costs: one more full-buffer stage with large blurs on every render
  where it is active. To be measured on a 30 MP frame before the slice
  lands; the push-pull itself is linear in pixels.

## Rejected

* **Detecting the backdrop by colour or by a model.** Measured to fail on
  the first session (finding 1), and the photographer knows where the
  backdrop is far better than a threshold does.
* **A flat colour or a gradient** (`fill` as a `local_adjustments` value).
  Visibly fake on all three sessions, and at rank 160 it would sit after
  the tone curve, so a later exposure change would split the extension from
  the cloth.
* **Patch-based synthesis** (copying patches of the cloth, PatchMatch
  style). It would reproduce folds and seams better, but its result
  depends on a search order, it repeats visibly over large areas, and its
  cost on 30 MP is several orders above a blur. It is a candidate for a v2
  if the soft-texture limit proves too narrow in use.
* **A generative fill as a stage.** Refused by ADR 0102 §2: a revision
  would need a model to render.
* **Storing a seed.** The grain's rule applies: a field recomputed from
  coordinates is the same at every resolution, and there is nothing to
  keep in sync.
