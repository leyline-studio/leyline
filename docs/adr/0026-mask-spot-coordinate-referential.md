# ADR 0026 — The coordinate frame of masks and local adjustments

**Status:** Accepted — 2026-07

## Context

`docs/v2-scope.md` §2, §5 and §9 identify two families of features to come —
local/masked adjustments (brush, radial/graduated/linear filters) and spot
removal — which both need to store geometry (points, radii, rectangles) per
revision. Both share the same unresolved problem, flagged as §9's first
cross-cutting "lock": in what coordinate frame is that geometry defined?

The pipeline's fixed order (`docs/pipeline.md` §3.1) puts Rotation/Crop
**last**, after every tonal operator. And yet `crop` itself is already
defined as "relative to the image **after** rotation"
(`crates/leyline-core/src/settings.rs:99`, `docs/pipeline.md` §3.2):
coordinates normalized within the rotated but not yet cropped canvas — not
the final visible frame, and not the raw decoded frame either. It is the only
existing precedent for normalized geometry in `settings_json`.

Rather than let every future feature ADR (masks, spot removal, regional
colour grading) settle this choice independently, this document settles it
once, up front.

## Decision

**All mask and local-adjustment geometry is stored in normalized `[0,1]`
coordinates relative to the image after rotation and before crop — exactly
the frame `crop` uses.** No new frame is introduced; the one already accepted
for `crop` is extended.

The render stages that evaluate that geometry (the tonal/local block, ahead
of §3.1's Rotation/Crop stage) run over a pixel buffer still in the
decoded/lens-corrected orientation — **before** rotation is applied. The
engine must therefore lift the geometry stored in that pre-rotation frame by
applying the **inverse** of the pending rotation to it, ahead of
rasterization or evaluation — the same backward remapping technique already
used by `rotate` (`process2.rs`) and by lens correction (`process3.rs`, ADR
0016), simply traversed the other way and applied to input geometry rather
than to output sampling coordinates. No new technique: the same family of
backward remapping, with a third consumer.

This choice prejudges neither the storage format (compact JSON vs. a
dedicated table) nor the coalescing of user gestures — those remain for the
ADR of each feature to settle (`docs/v2-scope.md` §9).

## Consequences

* **Stable under crop editing**: cropping only trims the post-rotation
  canvas, never repositions it — masks and spots stay aligned with the
  photographed content whatever crop rectangle is chosen afterwards, exactly
  as `crop` itself stays coherent under this convention.
* **Inherits `crop`'s existing behaviour under rotation editing**: if the
  rotation angle changes after a mask or a spot is placed, the stored
  geometry is reinterpreted against the new rotated canvas — a limitation
  already accepted for `crop` (§3.2), not a regression introduced here.
* The engine gains a coordinate-transform utility (forward/inverse rotation)
  shared between `rotate` and the future masks/spots stage — infrastructure
  extracted from `process2.rs`, without changing `rotate`'s own frozen
  rendering.
* This frame applies identically to regional colour grading
  (`docs/v2-scope.md` §4, a future extension of item 3 under a mask) with no
  further decision.

## Alternatives rejected

* **Coordinates relative to the raw decoded frame (before rotation)**:
  trivial to evaluate on the engine side (already the buffer's frame at that
  stage), but it pushes the rotation's inversion onto Studio, which would
  then have to recompute it on every mask or spot interaction on top of
  already doing so for the crop handles — complexity moved to the layer least
  equipped to carry it, and a break with `crop`'s precedent.
* **Coordinates relative to the final frame (after rotation AND crop)**: the
  most intuitive while editing (it is what the user sees), but it makes
  cropping destructive for masks and spots: shrinking or moving the crop
  rectangle changes the frame's origin and extent, silently invalidating
  every stored position — rejected, as it contradicts non-destructive editing
  of the crop independently of the other settings.
* **Reordering the pipeline to run masks and spots after Rotation/Crop**: it
  would make the frame trivially coherent (the stage would run in the same
  frame as the geometry), but any change to an existing pipeline's order
  already imposes a new process version (§3.3) — no saving there — and it
  would move spot removal and local adjustments after the crop, changing
  which pixels feed noise reduction and sharpening. A wider reordering
  undertaking, and unnecessary here: a coordinate transform suffices.
