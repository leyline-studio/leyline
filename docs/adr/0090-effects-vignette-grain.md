# ADR 0090 — The Effects panel: the vignette a photographer *wants*, and grain

**Status:** Accepted — 2026-08

## Context

Lightroom's `Effects` panel holds two tools: **Post-Crop Vignetting** and
**Grain**. Leyline has neither, and the gap is not cosmetic — a vignette is
how most photographers finish a portrait, and grain is how a digital file
stops looking like one.

The word *vignetting* already appears in this pipeline, and that is the trap
this decision exists to avoid. `lens` at rank 20 **removes** vignetting: a
radial gain read out of a Lensfun calibration for this body, this focal
length and this aperture, applied in the sensor's own frame, before a single
geometric stage has run (ADR 0017). What the `Effects` panel adds is the
opposite gesture — a darkening *nobody's lens produced*, centred on the frame
the photographer composed, drawn after every geometric stage. Same physics,
opposite intent, and therefore opposite ends of the pipeline.

Grain raises a different question, and a sharper one: it is an operator whose
whole purpose is to add **randomness** to pixels that
[`pipeline.md`](../pipeline.md) §5.1 promises will be identical in ten years.

## Decision

### 1. Two operators, both after the crop

Two stages, not one: `vignette` at rank **220** and `grain` at rank **230**,
between `crop` (210) and `output_rendering` (900). They share a panel in
Lightroom and nothing else; freezing them together, versioning them together
and invalidating one's cache with the other's setting would all be
consequences of a panel layout.

**"Post-crop" is not a name here, it is the rank.** The frame the vignette
centres on is the buffer `crop::v1` just produced, so re-framing a photo
moves its vignette with it and a 16:9 crop out of a 3:2 frame gets a 16:9
vignette. That is precisely the defect Lightroom had to add a second,
"post-crop" vignette control to fix, having shipped the first one in the
lens-correction frame. Leyline gets one control because it starts at the
right rank.

The distance from `lens` (20) to `vignette` (220) is the design, and both
ends are load-bearing: a correction is measured against a lens and belongs
before every resampling; an effect is drawn on a composition and belongs
after all of them.

### 2. The vignette is a multiplication in linear light

Four settings, neutral at `amount: 0`:

| Field | Range | Default | Meaning |
|---|---|---|---|
| `amount` | [-100, 100] | 0 | Negative darkens the corners, positive brightens them — Lightroom's sign |
| `midpoint` | [0, 100] | 50 | Where the falloff starts, 0 at the centre, 100 at the corners |
| `roundness` | [-100, 100] | 0 | The superellipse exponent: 0 an ellipse fitted to the frame, positive squarer, negative pointier |
| `feather` | [0, 100] | 50 | Width of the transition; 0 is a hard edge |

The shape is a superellipse `(|u|^n + |v|^n)^(1/n)` on frame-normalised
coordinates, divided by its own corner value so the corners sit at 1
whatever `n` is — without that division, moving `roundness` would move the
*brightness* of the corners as a side effect of moving the shape. The
transition is a smoothstep from `midpoint` to `midpoint + feather·(1 −
midpoint)`, and the gain is `2^(3·amount·t)`: **three stops at each end**.

The cap is deliberate. A slider whose last third drives a corner from
"black" to "blacker" is a slider that lies about having a range.

The gain multiplies the linear working buffer, and is not blended as a grey
in display space. `kernel::v1`'s own documentation already sorts operators
this way — white balance, exposure and *vignetting* describe light itself
and stay linear, tone controls declare the display axis — and the payoff is
concrete: a highlight two stops above white comes out of a −100 vignette
still above white, where `output_rendering` can still roll it off, instead
of being crushed onto a flat grey.

### 3. Grain is deterministic by construction, not by discipline

Three settings, neutral at `amount: 0`:

| Field | Range | Default | Meaning |
|---|---|---|---|
| `amount` | [0, 100] | 0 | Strength |
| `size` | [0, 100] | 25 | Lattice spacing, 1 to 16 **full-resolution** pixels |
| `roughness` | [0, 100] | 50 | Weight of a second octave at half the spacing |

The field is **value noise**: an integer hash of the lattice coordinates,
bilinear between lattice points, two octaves mixed by `roughness`. There is
no generator state, no seed, no clock and no dependence on how the rows were
scheduled — the value at a point is a pure function of that point. §5.1 holds
because the field is *recomputed* identically, never replayed.

**No seed is stored, and that is a decision.** A seed is a control whose only
possible meaning is "give me a different randomness"; to deliver it, it would
have to enter every revision, every preset, every fingerprint and the golden
manifest. Nobody has ever wanted the second roll of grain badly enough to pay
that.

The lattice is measured in **full-resolution pixels**: coordinates are divided
by ADR 0041's `scale` before hashing, so a display preview samples *the same*
field as the export rather than a different field at the same nominal size.
It samples it more coarsely, though, and no interpolation makes that go away
— grain is judged at 1:1, in Leyline as everywhere else.

The grain is monochromatic, added on the display axis
(`kernel::v1::in_display`), and its amplitude is weighted by `4·L·(1 − L)`,
clamped at zero. Grain that survives into a black or into a specular
highlight does not read as film; it reads as a sensor that was too hot.

### 4. What the panel deliberately does not get

* **Lightroom's three vignette styles** (Highlight Priority, Color Priority,
  Paint Overlay). They are three answers to a question a *bounded,
  gamma-encoded* buffer asks: what do you do when the darkening hits a
  channel that is already at 1? In unbounded linear Rec. 2020 (ADR 0044) a
  multiplication already *is* highlight priority, and the other two are the
  workarounds.
* **Coloured grain.** Chroma noise is what `noise_color` spends its time
  removing (ADR 0072).
* **A grain seed** (§3).

### 5. A `effects` preset category

`vignette` and `grain` are captured together by a new preset category,
`effects` ([`presets.md`](../presets.md) §3.1). A "film look" preset that
dropped its own vignette and grain would be a look preset missing the look.

## Consequences

* Two settings structs and two `Param` variants; **no schema change**. A
  stage absent from a revision's `stages` map is neutral, so every revision
  written before today renders exactly as it did — the same backward
  compatibility ADR 0088 §4 relied on. The golden manifest gains entries and
  moves none.
* Two golden cases. `vignette` is captured **over a crop**, so what is frozen
  is the §1 claim itself: move the stage before `crop::v1` and the digest
  moves. `grain`'s digest *is* the determinism assertion — it is the one
  operator whose test would fail if the field were ever seeded by anything.
* `crop/v1.rs` still calls itself "the last stage of the pipeline" in its
  header. It was already only the last *geometric* one — `output_rendering`
  has run after it since ADR 0044 — and the file is frozen, so it stays as it
  is rather than being edited for a comment.

## Alternatives rejected

* **One `effects` stage carrying both.** They would then freeze together,
  version together, and each would invalidate the other's cache checkpoint —
  three real costs bought by one panel heading.
* **The vignette in the `lens` frame**, sharing ADR 0017's radial code. That
  is the exact bug the word "post-crop" was invented to name.
* **Seeding the grain from the photo's identifier**, so two photos in a
  series do not carry the same grain. It would make the pixels a function of
  the *catalog*: the same file, imported into another library, would develop
  differently. §5.1 names the settings, the stage versions, the input
  checksum, the platform, the toolchain and the decoder — a row id is on none
  of those lists, and putting it there for a texture nobody can see at 1:3
  would be the largest promise ever broken for the smallest reason.
