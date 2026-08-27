# ADR 0071 — Seeing a mask: the coverage as an overlay

**Status:** Accepted — 2026-08

## Context

[ADR 0049](0049-local-adjustments-clients.md) delivered the masking tools in
all three clients and set three things aside, the first of them: **an overlay
of the coverage the engine computes**. It has stayed open since.

It is no longer a convenience. Three reasons, in the order in which they
weigh:

1. **A stored mask has no handle at all.**
   [ADR 0070](0070-stored-mask-coverage.md) has just added `Mask::Coverage`: a
   radial draws its ellipse, a gradient its axis, a brush its dabs — an
   imported coverage draws *nothing*. One imports it and guesses its effect
   from the result. The overlay is the only way to see it.
2. **A range mask cannot be guessed at all.** ADR 0048 multiplies the geometry
   by luminance and hue bands; the result no longer has a predictable shape.
3. It is how one works a mask everywhere else. Seeing the covered area is not a
   debugging aid, it is the tool.

## Decision

**Studio can display, over the preview, the coverage of the selected mask.**

### 1. It is a *view*, never a rendering

The overlay produces no photo pixel, enters no revision, and has **no stage
version**. `pipeline.md` §5.1 is not concerned: nothing frozen changes, and
nothing displayed here will ever be exported.

It is the same nature as the soft proofing of
[ADR 0051](0051-watermark-rasterization-and-soft-proof-surface.md): a display
transform, decided by the interface, which the engine computes but does not
record.

### 2. It must land in the right place, so it goes through the geometry

That is the whole problem, and the reason the overlay is not a mere drawing on
the interface side.

A coverage is rasterized in the **working buffer's** frame, unrotated
(`mask::rasterize_coverage`, ADR 0026). The displayed preview, for its part,
comes out of three geometry stages that run *after* the local adjustments:
`rotate` (rank 200), `perspective` (205) and `crop` (210). A coverage drawn
without them would be offset, tilted, and would spill outside the frame — the
more visibly the tighter the crop.

The coverage is therefore **pushed through those three stages**, at the
versions the revision pins, exactly like the photo's pixels. Nothing is
reimplemented: they are the same frozen stages, applied to another buffer.

The stages between 160 and 200 — LUT, noise, sharpening — are by contrast
**skipped**: they are pixel operators, and they would distort a mask image
without bringing it anything.

### 3. It shows the *effective* coverage, ranges included

Showing the geometry alone would be showing what one already knows, and staying
silent about what one cannot guess (§Context, point 2). The overlay is
therefore computed on the buffer as it stands at rank 160 — after the colour
operators, where the range terms read their values — and then multiplied by the
entry's opacity.

What the user sees is what the engine applies.

**The price, accepted:** the range term lives in `local_adjustments`'s frozen
modules, where it is already copied from version to version. The overlay copies
it once more, into a module that **dispatches on the pinned version**. That is
the counterpart of freezing, and it is bounded by the same property: a frozen
module never receives a fix, only a successor, so two copies cannot diverge.

### 4. One mask at a time: the selected one

Superimposing several coverages would give a mush in which one no longer knows
which entry does what. The panel already has a notion of selected row (ADR
0049); the overlay follows that selection, and disappears when nothing is
selected.

### 5. Red at 50 %, and a switch

The convention of every program in the field, and it is a good one: a frank hue
no photo contains uniformly, transparent enough that the image beneath can
still be judged.

The display is an **interface switch**, not a setting of the photo: it is not
recorded in the revision, and reverts to the displayed state on each mask
selection — it is what one wants to see while working a mask, and never what
one wants to find frozen on a photo three months later.

### 6. What this ADR does not do

The two other leftovers of ADR 0049 stay open, and each is a distinct piece of
work: the **range eyedropper**, and the **drag handles** of an
already-drawn geometry.

## Consequences

* The engine gains a surface: rendering a mask's coverage at a preview's size,
  `Library::mask_coverage_preview`. It returns a greyscale image, never a
  colour — the hue is an interface decision.
* A stored mask becomes **usable**: until now one imported it blind (ADR 0070
  §7).
* An extra computation cost, of the same order as a preview, and paid only when
  the overlay is on.
* The range term now exists in two more copies, dispatched by version. It is
  written here so that it is not discovered later as an oversight.

## Alternatives rejected

* **Drawing the geometry on the interface side**, in Slint, without going
  through the engine. It knows nothing of ranges nor of stored coverages —
  hence mute precisely where one needs to see — and would have to reimplement
  rotation, perspective and crop in order to land correctly.
* **Showing the geometry only**, without the ranges. Cheaper, and it stays
  silent about what one cannot guess.
* **Superimposing every mask** with a colour per entry. Illegible from three
  entries on, and it no longer says which one is being tuned.
* **A complete render with the mask substituted for the image**, letting every
  stage run. The LUT, the noise and the sharpening would distort the mask
  image: one would see a sharpened mask, not the mask.
* **Recording the overlay's state in the revision.** It is not a property of
  the photo; finding it on three months later would be a surprise, not a
  service.
