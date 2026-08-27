# ADR 0070 — A mask can be a computed coverage, not only a geometry

**Status:** Accepted — 2026-08

## Context

`Mask` can describe four things (ADR 0029,
[ADR 0048](0048-range-masks.md)): an ellipse, a gradient, a brush stroke, and
"everything". All four have one thing in common — they are **formulas**.
`rasterize_coverage` evaluates them per pixel, at the resolution of the render
under way, and therefore needs nothing but a few numbers stored in
`settings_json`.

That is exactly what a subject, sky or background mask lacks (C2 of
`measured-findings.md`). Its coverage is not derivable from six parameters: it
is an image. While `Mask` can carry only formulas, such a mask is **not
expressible**, and [ADR 0069](0069-closed-extension-boundary.md) has nothing to
attach to — its rule "an extension produces settings, never pixels" presupposes
that the setting produced can exist.

This ADR is therefore the **open** and free half of that arrangement: the free
engine learns to *store and render* a coverage, whatever produced it.

Nor is that specific to AI. A mask painted in another program, a selection
exported as a PNG, a luminance mask computed once and frozen: all hit the same
wall today.

## Decision

**`Mask` gains a `Coverage` variant, which references a coverage file instead
of describing a shape.**

```rust
Mask::Coverage {
    /// A library-relative path (`catalog.md` §2.3).
    path: String,
    /// BLAKE3 of the file's bytes, "blake3:<hex>".
    checksum: String,
}
```

### 1. The shape is the one that already exists twice

A relative path plus a BLAKE3 checksum: that is exactly the shape of
[`CameraProfile`](0035-camera-profile-dcp.md) and of
[`Lut`](0053-creative-lut.md), and for the same reasons — a library stays
portable, and a file substitution is **detected** instead of being rendered in
silence. A checksum that no longer matches is an error, never a different
rendering with no warning.

The file is resolved **before** the render, as for those two
(`camera_profile::resolve_from_settings`, `lut::resolve_from_settings`): the
pixel path never reads a file, and failure has a single, named point.

### 2. The file: a 16-bit grey PNG, at its own resolution

**Lossless**, because a lossily compressed mask would make a revision's
rendering drift with nothing to signal it.

**16-bit and not 8.** A coverage multiplies a setting: on a gentle gradient
pushed by several EV, 256 levels show as banding. The geometric masks are
evaluated in `f64` precisely to avoid that; storing in 8 bits would make the
*stored* path worse than the *computed* one exactly in the case where the
difference shows. A 16-bit grey PNG costs twice as many bytes before
compression, and a mask — large uniform areas separated by a fine transition —
compresses very well.

**At its own resolution, with no imposed ceiling.** A segmentation model
typically produces 512 to 1024 pixels across; storing that resampled to the
sensor's size would manufacture detail that does not exist and multiply the
bytes by thirty for nothing. The file therefore carries the resolution its
producer actually had, and the engine does not invent it.

A corollary to accept: **a stored mask's fineness is that of its file.**
Enlarged towards a full-resolution export, a 1,024-pixel mask gives a soft edge,
not a sharp one. That is a limit of the mask, not of the engine, and it is up
to the producer to store at a resolution equal to the complexity of the edge it
describes.

### 3. The coordinates are those of the other masks

The file is sampled **bilinearly on the normalized `[0,1]²` canvas** — the
post-rotation frame in which `rasterize_coverage` already evaluates the four
existing variants (`CanvasFrame`, ADR 0026).

In other words, a stored mask is **the same function of position as the
geometric masks, tabulated instead of computed**. It follows the rotation, it
combines with a range mask (ADR 0048) and an opacity like any other, and it is
independent of the render's resolution: preview and export sample it alike.

### 4. `local_adjustments::v3`, and the frozen versions' refusal

Rendering a variant `v1` and `v2` do not know is a new rendering, hence a **new
stage version** (`pipeline.md` §5.1).

`v3` renders the four existing variants **exactly** as `v2` does: it adds only
one more expressible case. The existing reference renders therefore do not
move; the manifest gains `v3` entries with identical fingerprints.

And the capability rule applies as it stands: **a revision pinned at `v1` or
`v2` carrying a `Mask::Coverage` is refused by `validate()`**, with the error
that says so. It is not rendered by ignoring the mask — an ignored mask is a
local adjustment applied to the whole image.

### 5. Writing a coverage: the surface the extension uses

```rust
impl Library {
    /// Files a coverage into the library and returns the mask ready to be
    /// placed in a revision.
    pub fn store_mask_coverage(&self, width: u32, height: u32,
                               coverage: &[u16]) -> Result<Mask>;
}
```

That is the only entry point, and it is the one a closed crate of ADR 0069
calls — through the SDK, like any client. It writes the file, computes the
checksum, and returns the corresponding `Mask::Coverage`. The caller never has
to know the path, the format or the location.

**The files are content-addressed**: `Masks/<blake3-hex>.png`, under the
library's root, beside `Profiles/Camera/` and `Profiles/LUT/`
(`catalog.md` §3). Two identical masks become one file, and rewriting the same
mask does nothing. The `path` field nevertheless stays explicit in the setting,
as with its two predecessors: uniformity is worth more than a saved field, and
it leaves the door open to another arrangement later.

### 6. What this ADR does not do

* **No mask is produced here.** The free engine can *store and render* a
  coverage; what *proposes* one — a model, a runtime, weights — is out of
  scope, and ADR 0069 explains why that separation is the arrangement's core
  rather than its reservation.
* **No creation tool in Studio.** Nothing in the project *makes* a coverage.

  Studio does however gain an **import** (§7): this ADR at first excluded any
  interface, on the grounds that no producer existed — but its own §Context
  already named three ("a mask painted in another program, a selection exported
  as a PNG, a luminance mask computed once and frozen"). The producer is the
  program next door. Shipping the variant without the means of using it would
  have made a capability that does not depend on AI wait for it.
* **No garbage collection of orphaned files.** A mask referenced by a revision
  in the history must survive an `undo`, otherwise a `redo` breaks. Masks are
  therefore **never** deleted implicitly. Collecting the files no revision cites
  any more is a piece of work apart, with its own ADR — it touches the history,
  hence what we promise not to lose.

### 7. Importing a coverage from an image file

Studio opens a file picker, reads **any image** the project can decode,
converts it into a coverage and files it through
[`Library::store_mask_coverage`]. It is an *import*, not a drawing tool: the
mask comes from elsewhere, whole, and Studio does not retouch it.

**Which channel becomes the coverage** is the only real question, and getting
it wrong would silently invert or flatten someone's work:

1. **The alpha channel**, if it exists and is not uniformly opaque — that is a
   selection exported with its transparency, and its alpha *is* the mask;
2. **otherwise the luminance** — that is a black and white mask, white =
   covered, the convention of every image editor.

The order matters: a selection exported as a PNG often carries black pixels
*and* an alpha, and reading the luminance would give an empty mask. A wholly
opaque image, for its part, has nothing to say through its alpha, hence the
fallback.

**No resampling**: the file is stored at its size, §2. An 800 px mask imported
for a 30 Mpx photo stays an 800 px mask, with the edge softness that implies —
and that is what its author produced.

The formats accepted at import are broad (whatever the decoder reads); the
*stored* form stays §2's, with no exception. The import converts, it does not
widen the contract.

## Consequences

* `Mask` stops being closed on geometry: a mask from elsewhere — another
  program, an exported selection, a model — becomes expressible, and everything
  that already exists (ranges, opacity, rotation, stacking) applies to it with
  nothing new.
* **The free version renders everyone's masks**, which is the property ADR 0069
  §1 promised and this ADR delivers.
* **And it can already receive one**, without waiting for any model: whoever
  has a mask somewhere can bring it in (§7).
* A library gains a `Masks/` directory and weighs a little more. `catalog.md`
  §3 documents it.
* Resolved coverages **travel like the DCP profile and the LUT**: from
  `resolve_from_settings` at the engine's edge, through `render`,
  `render_scaled`, `develop_scaled` and the stages' `Context`, down to
  `rasterize_coverage`. It is not a single signature change but the same chain
  as the two references that already existed — and the frozen stage versions,
  for their part, receive an **empty** map, which makes their immunity
  structural instead of depending on `validate()`'s refusal.
* A missing or modified mask file is an explicit **render error**, exactly like
  a vanished `.dcp` or `.cube`.

## Alternatives rejected

* **Storing the coverage in `settings_json`**, in base64. A 30 Mpx coverage
  weighs 60 MB in 16-bit, ~80 MB encoded — in a TEXT column, for *every*
  revision in the history. The path-plus-checksum shape already exists twice in
  the project precisely for that kind of data.
* **A lossy format** (JPEG, lossy WebP) to save space. A revision's rendering
  would drift with re-encoding, which §5.1 forbids.
* **Imposing the sensor's resolution.** It manufactures detail the producer did
  not have, for thirty times the bytes.
* **Vectorizing the coverage** into outlines so as to stay a "formula". A mask
  of hair or foliage is not vectorizable without betraying it, and the
  approximation would be invisible in the setting while changing the rendering.
* **Filing the file beside the photo**, like a sidecar. It contradicts the
  non-destructiveness contract (`pipeline.md` §6): nothing is written beside
  the originals.
* **Changing nothing and rendering an AI mask from a plugin called at render
  time.** That is the alternative ADR 0069 rejected, and this ADR is what makes
  it unnecessary.
