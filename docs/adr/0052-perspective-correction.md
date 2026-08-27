# ADR 0052 — Perspective correction: one homography with two sliders, between rotation and crop

**Status:** Accepted — 2026-07

## Context

The pipeline knows how to straighten a horizon (`rotation`) and to crop
(`crop`). It does not know how to straighten **converging verticals**:
photographing a building with the camera tilted up makes its edges converge,
and no Leyline setting touches that. It is the geometric pipeline's plainest
functional gap — it does not even have an ADR rejecting it, unlike *heal*
([ADR 0032](0032-spot-removal-clone.md)) or the GPU
([ADR 0012](0012-rayon-data-parallelism.md)).

Every competitor has it: darktable (*rotate and perspective*), RawTherapee
(*transform*), Lightroom (*Transform*), digiKam. It is also the first setting
anyone photographing architecture or copying documents asks for.

**What is not at issue.** The coordinate frame of
[ADR 0026](0026-mask-spot-coordinate-referential.md) (masks and spots are
placed *before* geometry), the pipeline's order, and preview scaling
([ADR 0041](0041-interactive-preview-rendering.md)).

## Decision

### 1. Two sliders, not six

`Settings` gains an optional field:

```rust
pub struct Perspective {
    /// Vertical correction, a slider in [-100, +100].
    pub vertical: i32,
    /// Horizontal correction, a slider in [-100, +100].
    pub horizontal: i32,
}
```

Absent means neutral. Two terms, because they are the two the photographic
gesture produces: tilting the camera up (converging verticals) and turning it
(converging horizontals).

What Lightroom calls *Aspect*, *Scale*, *X/Y Offset* does not enter. `Aspect`
is a stretch, not a perspective; `Scale` and the offsets are a crop, which
`crop` already does — adding them here would give two ways of expressing the
same thing, with two possible orders of application and a `settings_json` that
would no longer say which took place.

**Automatic correction does not enter either**: detecting vanishing lines
requires edge detection and a Hough vote, that is, an image-analysis algorithm
whose result depends on the content. It would be a decision in its own right
(and a serious candidate: the render mechanics would be these).

### 2. A homography, not two shears

The correction is a **projective transform** (a 3×3 homography) whose
coefficients come from the two sliders: each slider brings the two corners of
one edge closer together and spreads those of the opposite edge apart, in the
frame's normalized coordinates. The matrix is then **inverted** and the render
samples the source backwards (as `rotate::v1` does), bilinearly.

An affine shear — simpler to write — does *not* correct a perspective: it
tilts the verticals without changing their convergence. What distinguishes a
perspective correction from a mere straightening is precisely the division by
the third coordinate.

### 3. Rank 205: after rotation, before crop

The order is constrained on both sides:

* **after `rotate`** (rank 200), because a level horizon is the reference
  against which a vertical is vertical; correcting the perspective of a tilted
  image would ask the user to compose the two mentally;
* **before `crop`** (rank 210), because the correction widens the frame (the
  edges become trapezoids) and one crops what one sees, not the reverse.

Rank 205 was free, which is exactly what ADR 0042's tens are for.

### 4. The frame grows, it does not fill in

Like `rotate::v1`, the stage renders the **bounding box** of the transformed
quadrilateral, and the pixels with no source stay black. No fill, and no
automatic crop into the useful content.

That is consistent with rotation, which already does exactly this, and it is
what `crop` serves to correct — the user sees what the correction produced and
decides for themselves what to keep. An automatic crop would decide for them,
and would lose pixels they might have wanted to keep.

### 5. A normalized value, hence independent of size

Neither slider expresses any length in pixels: they move corners as fractions
of the frame. A reduced preview (ADR 0041) and a full-resolution export
therefore undergo **the same** transform, with no scale factor to propagate —
unlike the blur radii of clarity or denoising.

### 6. Out of scope

* **Automatic correction** (§1).
* **Lens correction** — distortion, TCA, vignetting — which is another problem,
  already handled by Lensfun
  ([ADR 0016](0016-process-3-lens-correction.md)–[0018](0018-process-5-tca.md))
  and at another rank.
* **Automatic cropping into the useful content** (§4).
* **`Aspect`, `Scale`, offsets** (§1).

## Consequences

* **The geometric pipeline's last gap closes**, with a stage neutral by
  default: no existing revision changes its rendering.
* **One more stage in the geometric pipeline**, hence one more resampling when
  it is active. That is the price of a legible order: composing rotation and
  perspective into a single matrix would be cleaner in pixels, but it would
  make `rotate`'s rendering depend on a setting that is not its own, and would
  break `rotate::v1`'s freeze.
* **`settings_json` gains an optional field whose neutral value is absence**,
  so `schema` is not incremented (`docs/pipeline.md` §3.4).
* **All three clients expose it**: two sliders in Studio's *Geometry* group,
  `develop … perspective <vertical> <horizontal>` in the CLI.

## Alternatives rejected

* **An affine shear.** It does not correct a perspective (§2).
* **Composing perspective inside `rotate`**, so as to sample only once.
  Forbidden by the freeze: `rotate::v1` renders what it renders, and a
  `rotate::v2` reading a new setting would force every revision wanting
  perspective to change its rotation version too, and hence its rotation
  rendering. Two independent stages are the shape ADR 0042 makes possible.
* **Eight parameters (the four corners).** More expressive, and unusable from
  the keyboard; the interface that would make them useful is a quadrilateral
  drawn on the image, to be built on top of this same mechanics the day the
  need arises — like the mask handles of
  [ADR 0049](0049-local-adjustments-clients.md) §6.
* **Cropping automatically after correction.** §4.
