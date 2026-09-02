# ADR 0109 — Reshape: moving content without inventing any

**Status:** Accepted — 2026-09

## Context

Every stage in the pipeline changes what a pixel *is*. None changes where
it **is**, except the three that recompose the whole frame — `lens` (rank
20), `rotate` (200), `perspective` (205) — and each of those applies one
transform to the entire image.

Nothing can move a part of a photograph relative to the rest. That is a
gap with ordinary uses well outside anything portrait-specific: a
wide-angle lens stretching a face at the edge of the frame, a horizon
bowed by a correction that fixed the centre and not the corner, a
reflection that lands one pixel-row off, a subject whose silhouette the
photographer wants to ease. It is also, on the portrait side, the
operation a photographer would otherwise leave the application to perform.

The reason to build it here rather than defer it to another program is the
one this repository gives every time: the alternative is export → other
tool → re-import, and what comes back is a finished picture with no
development left on it ([ADR 0107](0107-derived-assets-and-the-pixel-socket.md)
opened a socket precisely to stop paying that). But unlike a denoise, a
reshape needs no model, no weights and no second process. It is geometry.
It belongs in the free engine, as a stage, dosable and reversible like
every other setting.

## Decision

### 1. A new stage, `reshape`, at rank 25

A **new stage**, not a version of an existing one — so
[`pipeline.md`](../pipeline.md) §5.1 is untouched for every revision
already written, and the golden manifest does not move a digit. That is
[ADR 0103](0103-red-eye-correction.md)'s lesson, measured there rather than
assumed: a new stage costs the existing renders nothing.

**Rank 25 — after `lens` (20), before `spot_removal` (30).** The position
is the decision, and there are three reasons, in order of weight.

* **Everything a person places afterwards is placed on what they see.**
  Spots (30), red eyes (35) and masks (160) are all hand-positioned on the
  displayed photograph. If the reshape ran after them, a spot placed on a
  reshaped cheek would land on unreshaped pixels. Running it *first* among
  the hand-placed edits makes every later placement land where the user
  meant it, with no compensation anywhere.
* **It is a statement about the subject, not the composition.** `lens`
  precedes it because lens correction is about the optics of the original
  frame, and a reshape corrects what the corrected optics show. `rotate`
  and `crop` follow far behind because they are re-framings; a reshape must
  not move when a photograph is re-cropped.
* **Resampling before sharpening rather than after.** A warp is bilinear,
  so it softens very slightly. At rank 25 what `sharpen` (190) sharpens is
  the final geometry. `rotate` at 200 has the opposite arrangement and has
  to — it is late for compositional reasons — but this stage does not
  inherit that constraint and should not pay it.

### 2. A list of control points, and an empty list is neutral

```rust
pub struct ReshapePoint {
    /// The content to move: the point the user grabbed.
    pub from: Point,
    /// Where it is to appear: the point they dropped it at.
    pub to: Point,
    /// Radius of influence, normalized against the buffer's larger
    /// dimension. Strictly positive.
    pub radius: f64,
    /// How much of the displacement to apply, in [0, 1].
    pub strength: f64,
}
```

`Settings::reshape` is a `Vec<ReshapePoint>`, and — like `spot_removal`,
`red_eye` and `local_adjustments` before it — **an empty list is the
neutral value, and there is no neutral entry**. The convention is the
repository's, not this ADR's, and following it is worth more than any
improvement on it.

`strength` per point rather than one dose on the stage: it is the shape
`SpotRemoval::opacity` and `LocalAdjustment::opacity` already have, and
it dials one handle without touching its neighbours. Dialling *all* of
them back is then a gesture in the client — select and drag — not a
setting the document has to carry.

Coordinates are those of [ADR 0026](0026-mask-spot-coordinate-referential.md):
normalized, post-rotation, pre-crop, mapped back into the buffer by the
`post_rotation_point_to_buffer` that `spot_removal` and
`local_adjustments` already call. No new frame, and `reshape` reads
`rotation` for exactly the reason they do.

### 3. The stage is defined by its *inverse* map, and that is what makes it cheap

For an output pixel `p`, the input is sampled at

```text
source(p) = p + Σᵢ wᵢ(p) · strengthᵢ · (fromᵢ − toᵢ)
wᵢ(p)    = smoothstep falloff of ‖p − toᵢ‖ over radiusᵢ
```

The weight is centred on **`to`**, the destination, and the offset points
back toward `from`. Read it at `p = toᵢ`: the weight is 1, so the sample
lands exactly on `fromᵢ` — the content the user grabbed appears where they
dropped it, by construction. At `‖p − toᵢ‖ ≥ radiusᵢ` the weight is 0 and
the pixel is untouched.

Stating the operation this way, rather than as a forward push of pixels,
is the whole of its implementation cost. A forward warp has to be inverted
numerically before it can be rendered; this needs no inversion, no
scattered writes, no accumulation buffer. It is `rotate::v1`'s technique —
inverse map, `bilinear`, `f64` geometry — with a different formula for the
source coordinate, and ADR 0026 already calls that "the same family of
backward remapping, with a third consumer". This is the fourth.

The falloff is `smoothstep01`, the same one masks and the sharpening edge
mask use.

### 4. Three costs, stated rather than discovered

* **Overlapping points add.** Two handles whose radii overlap sum their
  displacements, and a large enough sum makes the map locally
  non-injective — a fold, which reads as a smear. It is not a crash and it
  is not prevented: refusing the combination would mean solving for
  injectivity on every drag, and the artifact is visible in the loupe the
  moment it appears. Same register as
  [ADR 0108](0108-local-texture-clarity-sharpness-noise.md) §2's large
  radius in a small mask: the engine does not second-guess a gesture whose
  result the user is looking at.
* **The frame does not change size, and the edge is extended.** Unlike
  `rotate`, the output canvas is the input canvas. A sample falling
  outside the frame reads the nearest edge pixel rather than black:
  black would be a hole the user did not ask for and that nothing in this
  program can fill.
* **Nothing is invented.** Every output pixel comes from an input pixel.
  Content is stretched and compressed, never generated — which is exactly
  why this needs no model, and why it stays inside a pipeline whose
  promise is that the same settings give the same pixels forever.

### 5. What may propose the points, and what may not apply them

Control points are **settings**. An extension that computes face landmarks
and turns them into a `Vec<ReshapePoint>` is therefore
[ADR 0069](0069-closed-extension-boundary.md)'s shape exactly — settings,
never pixels — and needs no new boundary, no new socket and no change
here.

One thing is genuinely unsettled and is named rather than glossed:
[ADR 0073](0073-external-mask-detectors.md)'s detector protocol returns a
**coverage image**, and landmarks are not one. A landmark detector would
need either a second output kind in that protocol or to be an ordinary SDK
client. That is a decision for the day someone writes one, and this ADR
does not pre-empt it.

What is settled: **nothing proposes and applies in the same gesture.** A
computed set of points is offered and accepted, exactly as
[ADR 0105](0105-detector-conformance-and-cli.md) §2 has a detector choose
*where* while the photographer chooses *what*, as
[ADR 0084](0084-assisted-culling.md)'s culler proposes and never writes,
and as [ADR 0088](0088-auto-tone-and-black-and-white.md)'s Auto is a
button. An automatic tool is a user's choice, never a default and never a
standard.

## Consequences

* [`pipeline.md`](../pipeline.md) gains a `reshape` row at rank 25 and a
  `reshape` array in the `settings_json` example; `catalog.md` is
  untouched, since this is a setting and not a stored file.
* The clients gain the gesture: Studio a handle tool on the loupe — a drag
  from `from` to `to`, with the radius on a modifier — and the CLI a
  `reshape` verb taking the four numbers, plus `reshape reset`.
* Cost: one pass, one `bilinear` per output pixel, plus one falloff
  evaluation per point per pixel *within that point's radius* — the sum is
  over the handles that actually reach the pixel, not over all of them.
  A photograph with no handles pays nothing: the stage is inactive.
* The existing golden entries do not move. New cases are added for the new
  stage, and the manifest gains entries rather than changing any.

## Rejected

* **A painted displacement field**, the Photoshop-liquify brush. It is not
  compactly storable — the field *is* the data, so `settings_json` would
  grow without bound — and it cannot be dialled back after the fact. The
  control-point list is precisely what keeps a reshape a **setting**
  instead of a picture, and therefore what keeps it non-destructive.
* **Moving Least Squares or line-pair (Beier–Neely) warping.** Both hold
  shapes together better when handles are numerous, and both are
  materially more code that version 1 would freeze forever. The sum of
  radial displacements is the honest starting point; if handles ever get
  numerous enough for its weaknesses to matter, that is a `reshape::v2`
  with a stated reason, which is how this repository changes a rendering.
* **A second falloff knob** (density, softness). The radius already sets
  the extent and the smoothstep is the shape. Two controls for one effect
  is how a panel becomes unusable.
* **Automatic reshaping in-tree.** Not on principle — §5 says how it can
  arrive — but a first-party detector, a weight file and a third
  packaging burden are exactly what
  [ADR 0102](0102-paid-extensions-and-the-pixel-boundary.md) declined for
  a denoiser, and the argument does not weaken for a face.
* **Filling what a large displacement empties.** There is nothing to fill:
  §4's edge extension covers the frame border, and inside the frame the
  map is total.
