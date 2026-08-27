# ADR 0053 — Creative LUT: `.cube` files supplied by the user, applied on the display axis, with a strength control

**Status:** Accepted — 2026-07

## Context

Leyline knows how to recolour a photo through its own operators (curve, HSL
mixer, colour grading wheels, [ADR 0030](0030-tone-curve.md),
[ADR 0031](0031-hsl-color-grading.md)). It does not know how to apply a **3D
LUT**, that is, a look distributed as a file: film simulations, production-house
renderings, log-to-display conversions.

The three free competitors do it (darktable *lut 3D*, RawTherapee *film
simulation*, ART), Adobe/Iridas's `.cube` format is the de facto exchange, and
thousands of those files circulate — free or sold. It is the last of the five
functional gaps noted against them, and the cheapest to close: everything is
already in place but reading the file and interpolating.

**What is not at issue.** The existing colour operators, which stay the normal
route: a LUT is not a setting, it is a look chosen elsewhere.

## Decision

### 1. A file referenced like a DCP profile, never copied into the revision

[ADR 0035](0035-camera-profile-dcp.md)'s model applies word for word, and it
was designed for this case:

```rust
pub struct Lut {
    pub enabled: bool,
    /// A library-relative path, `Profiles/LUT/<name>.cube`.
    pub path: String,
    /// `blake3:` plus 64 hex digits of the imported bytes.
    pub checksum: String,
    /// Strength, a slider in [0, 100]. 100 = the LUT as it is.
    pub strength: i32,
}
```

* the file is **imported** into `Profiles/LUT/` by the engine, never referenced
  where the user found it — without which the library would cease to be
  movable ([ADR 0010](0010-relative-paths.md));
* the revision carries the **checksum** of the imported bytes, so a silent
  replacement of the file is detectable;
* an import that would overwrite a name already taken is **refused**, as for a
  `.dcp`.

No catalog table and no new mechanism: it is the same path, for the same
reason.

### 2. The strength is part of the setting

`strength` is not an ornament: a film-simulation LUT is almost always too
strong at 100 %, and every competitor exposes that slider. The blend is linear
between the LUT's input and output, on the display axis (§3) — the only place
where "50 % of that look" means what the user sees.

### 3. Applied on the display axis, not in linear light

A `.cube` is written for **display-referred** values in `[0, 1]`: its author
tuned it looking at an image, not at an unbounded linear buffer. Applying it to
our linear values would give a result bearing no relation to what the file
describes.

The stage therefore encodes every sample onto the display axis
(`kernel::v1::display`, ADR 0044), applies the LUT, and **comes back** to
linear. It is the same reasoning as range masks
([ADR 0048](0048-range-masks.md) §3), and the same function.

An accepted consequence: whatever exceeds white is **clipped to 1 before the
LUT**, since the LUT has no defined value beyond it. A LUT is an output look;
the headroom above white is `output_rendering`'s domain, which comes after.

### 4. Rank 165: after all the colour, before the detail

The LUT is the **last colour decision**, hence after the curve, the HSL mixer,
colour grading *and* the local adjustments — a look applies to the graded
image, not before it. And **before** denoising and sharpening: those work on
local structures, which a high-contrast LUT would amplify if it came after.

Rank 165, free between `local_adjustments` (160) and `noise_luminance` (170).

### 5. Trilinear interpolation, 1D and 3D `.cube`

* the reader accepts `LUT_3D_SIZE` (the common case) and `LUT_1D_SIZE`, the
  `DOMAIN_MIN`/`DOMAIN_MAX` directives, `#` comments and titles;
* the interpolation is **trilinear**. Tetrahedral is slightly more faithful at
  the cube's edges and significantly longer to write; it will be a `v2` if a
  visible difference presents itself, exactly like any other rendering
  correction;
* a size outside `[2, 128]`, a malformed line, a truncated file: a **named
  error**, never an approximate rendering. The reader lives in
  `leyline-color`, beside the DCP reader and for the same reason
  ([ADR 0037](0037-dcp-parsing-dependency.md)): reading a well-specified
  tabular data format is not a problem that deserves a dependency.

### 6. Out of scope

* **The `.3dl`, `.look`, link-type `.icc` formats, and HaldCLUTs as PNG.**
  `.cube` covers the real exchange; the others will be added as variants of the
  same stage if the need arises, with no new fundamental decision.
* **LUTs shipped with the application.** Leyline embeds no look: that would be
  an aesthetic choice by the publisher, and the project does not make those
  (`docs/vision.md`). The user brings their own.
* **Tetrahedral interpolation** (§5).
* **A LUT per local mask.** `LocalAdjustmentValues` re-parameterizes sliders,
  not file references (ADR 0029); fitting one in there is another decision.

## Consequences

* **The last of the five gaps against the free competitors closes.** The files
  the user already owns work.
* **No new mechanism**: ADR 0035's resource import and checksummed reference,
  ADR 0048 §3's display axis, ADR 0042's versioned stage.
* **A new stage neutral by default**, so no existing revision changes its
  rendering.
* **`leyline-color` gains a second format reader**, and the same argument as
  for DCP: no dependency, a read-only surface, named errors.
* **Two axis conversions per pixel when the LUT is active** (a linear ↔ display
  round trip), which is the cost of applying it where it means something.

## Alternatives rejected

* **Applying the LUT in linear light**, with no conversion. Less computation,
  and a rendering bearing no relation to what the file describes: a `.cube`'s
  content is defined over display values (§3).
* **Placing it after `output_rendering`**, in the output space. That would be
  the place most faithful to a colourist's intent, but it would put an operator
  *after* the conversion to the output space, hence outside the working buffer
  — and would make the result depend on the output profile chosen at export. A
  look must not change according to whether one exports in sRGB or in
  Adobe RGB.
* **Copying the LUT's bytes into `settings_json`.** A revision self-contained
  to the very end, and a `settings_json` of several megabytes per photo. ADR
  0035 already settled that trade-off the other way.
* **An absolute path to the user's file.** Forbidden by ADR 0010: the library
  would cease to be movable.
* **Embedding a set of film simulations.** §6.
