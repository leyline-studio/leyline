# ADR 0035 — Camera profile (DCP): a new colorimetric stage at the head of the pipeline, user-supplied files, referenced and checksummed

**Status:** Accepted — 2026-07
**Follow-up:** `camera_profile::v1`, which this ADR creates, is no longer the
current version. [ADR 0062](0062-dcp-illuminant-interpolation.md) replaces
§Decision's simplification with a real interpolation of the two calibration
illuminants (`v2`), and [ADR 0063](0063-dcp-tables.md) finally applies the
profile's tables — `HueSatMap` and `LookTable` (`v3`). The model decided here —
a colorimetric stage at the head of the pipeline, user-supplied files,
referenced and checksummed — is unchanged.

## Context

`docs/v2-scope.md` §8 notes that no DCP-style camera colour calibration exists
today. The V1 lens correction (Lensfun, ADR 0016–0018, process 3–5) covers
**geometric** distortion/vignetting/TCA, not the sensor's **colour**. A DCP
profile calibrates the sensor's colour rendering (colorimetric matrices, HSL
tables, tone curve, *look table*) — hence very early in the pipeline, at the
sensor RGB → working space conversion.

Two cross-cutting decisions are **consumed, not re-litigated, here**:

* **ADR 0027** widened `leyline-color` from "expose a static profile"
  (ADR 0015) to "load arbitrary ICC profiles and build `cmsTransform`s". It
  explicitly placed DCP authoring **outside its own decision**, noting that it
  "acts on the sensor → working space conversion, at the **start** of the
  pipeline (a genuine process-version event)", but that this item would from
  then on be able to "assume that `leyline-color` will already be a general ICC
  transformation library". This document is the ADR announced by ADR 0027.
* **ADR 0028** fixes the versioning strategy: one process version per pixel
  feature, each in its own frozen `processN.rs` module, created by copying the
  previous module whole. The camera profile is a pixel operator: it therefore
  takes a new process version without this ADR having to re-choose the
  convention.

`docs/v2-scope.md` §8 leaves three questions open that are specific to the item:
the DCP parsing dependency, the interaction with the sRGB freeze (partly
resolved by ADR 0027), and the reproducibility of an entirely new colour path.
This document settles the **pipeline placement, the owning crate, the source of
the profiles and their reproducibility**; it explicitly leaves the **parsing
dependency** open.

## Decision

### A new "Camera profile" stage, the very first of the pipeline — before even lens correction

A new **"Camera profile"** stage is inserted into the fixed order of
`docs/pipeline.md` §3.1 **between decoded RAW and Lens correction** — it is the
**very first** stage, before even the geometric lens correction.

That **makes precise** §8's sketch ("between decoded RAW and White balance") by
also fixing the order **relative to lens correction**: a DCP calibrates the
sensor's **colour** response (matrices, HSL tables, tone curve, look table);
lens correction handles **geometry** (distortion, vignetting, TCA). The two
**do not interact** — one remaps colours, the other remaps positions. Placing
the colorimetric calibration first makes it operate on the **least-processed
data possible** (the just-decoded sensor RGB), consistent with this ADR series'
other "operate on the least-processed data" placement decisions — ADR 0032's
spot removal, placed early "to operate on data close to linear". A DCP applied
after a geometric remapping would have no additional colorimetric meaning, and
would introduce a needless ordering dependency between two operators that have
none.

It is a **genuine reordering event** — a new stage inserted before the
currently first stage. `docs/pipeline.md` §3.1/§3.3 requires such an insertion
(like any step that changes the pixels a revision produces) to take a **new
process version** just like any insertion — **process N, the next number
available when this feature ships** (ADR 0028), in its own `processN.rs` module,
a whole copy of the previous module augmented with the camera-profile stage
alone. This ADR **does not freeze** a specific process integer: the shipping
order of the V2 items belongs to the future implementation plan.

### Crate placement — no new crate, an extension of `leyline-color`

The DCP parser and the logic that applies its matrices/tables live in
**`leyline-color`**, not in a new crate. Reasoning: ADR 0027 already established
`leyline-color` as growing from "one static profile" into a general colour
transformation library. Applying a DCP — a fixed matrix + LUT recipe,
**applied directly** (not through `lcms2::cmsTransform`, since DCP is not ICC,
see below) — is **adjacent colour pipeline** work, parallel to the ICC work
already housed there. It does **not** need coupling to the engine's buffers.

An explicit contrast with two placements decided differently elsewhere in this
series, for two different reasons:

* **Masking (ADR 0029) got a `leyline-engine` module, not a crate**: because
  mask rasterization is tightly **coupled to the render buffer's internals and
  its sampling** — a crate boundary would separate two things that must share
  those internals.
* **The camera profile gets a `leyline-color` extension, not a crate**: because
  it is **colour domain logic**, parallel to the ICC work already there
  (ADR 0027), and **not** something that needs coupling to the engine's
  buffers. The `processN.rs` module **calls** `leyline-color` to transform
  colour samples, just as it already calls `leyline-lens` for geometry.

Two features, two distinct placement arguments — stated together so the
contrast is legible.

### The DCP parsing dependency — an explicit open risk, not resolved here

DCP is Adobe's format, founded on **TIFF/EP tags**, **not** ICC: `lcms2` does
not parse it. Whether Leyline writes a **minimal in-house DCP parser** (only the
tags needed for application) or **integrates an existing Rust crate** (if a
suitable and licensable one exists when the time comes) is **left to the
implementation PR** — it depends on what is available and licensable at that
moment. This ADR fixes the **pipeline/architecture** decision, not the parsing
dependency choice.

In the same spirit as ADR 0016's caution ("non-trivial to validate without
reference images to hand" for vignetting/TCA), the **colorimetric correctness**
of DCP application must be **validated against real Adobe-generated DCP files
and their reference renders** before any release — that is exactly the bar this
project has already set for this kind of claim. This ADR does not claim that the
colour path is correct; it fixes where it lives and requires its validation.

### Source of the profiles — user-supplied files, no bundled database in V2

**No bundled DCP profile database in V2.** An explicit contrast with Lensfun
(ADR 0004/0016): lens correction rests on an **open, community, bundled profile
database**, where matching by EXIF string against thousands of profiles makes
sense. **DCP** profiles are of another nature: they are typically **generated by
the user, body by body**, from a calibration target (or downloaded individually
from a third party) — not an open, community-maintained database that Leyline
could bundle the way it bundles Lensfun's. Bundling such a database is a far
larger and separate undertaking (data rights/licences, hosting, maintenance),
frankly out of scope here.

V2 therefore lets the user **drop their `.dcp` files** into a profile folder
**relative to the library** (`docs/catalog.md` §2.3: relative paths, never
absolute, for portability), for example `Profiles/Camera/`, and **references
them from `settings_json` by relative path**. The profile is matched **by an
explicit stored path**, **not** auto-matched by the camera's EXIF model the way
`lens_correction.profile: "auto"` is. Reasoning: unlike Lensfun's community
database where fuzzy matching an EXIF string against thousands of bundled
profiles makes sense, the single DCP file a user produced for their own body
**needs no fuzzy matching** — an explicit reference is simpler and more
predictable.

### Reproducibility of a referenced external profile file — a genuinely new problem

This is a problem **neither ADR 0026, nor 0029, nor 0032 had to solve**: they
all store their geometry **inline** in `settings_json`, with no reference to an
external file. Here, `settings_json` **references a `.dcp` file outside
itself** — a new input whose reproducibility must be guaranteed.

**Decision: store a BLAKE3 checksum** (the same algorithm as ADR 0006, applied
to a **new kind of referenced file** rather than to a photo asset) of the `.dcp`
file's bytes, **next to its relative path** in `settings_json`. At render time,
if the current file's checksum **does not match** the stored checksum, the
engine **must not silently render with a modified profile**.

That **extends** `docs/pipeline.md` §5's reproducibility contract — "two runs
are identical if and only if … the input resource is identical (same
`checksum`)", "same revision → same pixels forever" — to this **new case of an
externally referenced input file**, exactly as the contract already applies to
the photo asset's own checksum. A DCP is, colorimetrically, a render input just
as much as the sensor pixels.

**Failure mode — not a new category.** A checksum that does not match is
handled **as the engine already handles a `schema`/`process` it does not
recognize** (`docs/pipeline.md` §3.4): it **never modifies the revision**,
**does not edit the asset** (read-only), and **displays the best available
preview with a warning**. No new failure category is invented: the existing
"do not destroy prior work, warn" policy is extended to this new trigger
(missing or modified profile file).

### Storage / schema — additive

`camera_profile` is an optional object of `settings_json`. **Absent =
neutral**: LibRaw's default path (sRGB output, ADR 0015) unchanged — exactly
ADR 0015's status quo when the field is absent. No schema bump required,
consistent with the additive "process +1, schema unchanged" scheme of most V2
items (`docs/v2-scope.md` §1) — fields unknown to an older engine are preserved
verbatim (`Settings::extra`, `crates/leyline-core/src/settings.rs`).

Sketch (the style follows `docs/pipeline.md` §3.2):

```json
{
    "schema": 1,
    "process": 7,

    "exposure": 0.2,
    "camera_profile": {
        "enabled": true,
        "path": "Profiles/Camera/EOS60D-D65.dcp",
        "checksum": "blake3:9f2b…"
    }
}
```

Neutral case — field absent, render bit-for-bit identical to the previous
process version (ADR 0015's default LibRaw-sRGB path):

```json
{ "schema": 1, "process": 7, "exposure": 0.2 }
```

> *The `process: 7` above is purely illustrative: the real number is the next
> available one when it ships (ADR 0028), not fixed by this ADR.*

> **Implementation note (not a spec edit here).** This ADR does **not** modify
> `docs/pipeline.md` §3.1's diagram or §3.3's process-version table. As for
> ADR 0029–0033, the spec is updated in the same change as the actual
> implementation, per CLAUDE.md. The present document fixes only **where** the
> stage lands (very first, before lens correction), **where** its code lives
> (`leyline-color`) and **how** its reproducibility is guaranteed (BLAKE3
> checksum, §3.4 failure mode); §3.1's diagram and §3.3's table will be amended
> by the PR that ships the process module.

> **Note of 2026-08-02, after confronting real profiles.** The first authentic
> `.dcp` tried revealed that **none** could be read: a profile is a bare IFD
> carrying version `0x4352`, where the reader expected a standard TIFF with an
> image. Fixed by an in-house IFD reader — which §Decision already described,
> but which the implementation had delegated to `tiff::Decoder`. Now verified:
> reading real files, and the preservation of a neutral grey through the
> matrix. Still unverified, and hence the "experimental" mention remains:
> agreement with Adobe's *rendering*, which requires Lightroom or ACR.

> **Note of 2026-08-03, after comparison with an independent render.** The full
> DCP path — container, matrices, interpolated illuminants
> ([ADR 0062](0062-dcp-illuminant-interpolation.md)) and tables
> ([ADR 0063](0063-dcp-tables.md)) — was confronted with a RawTherapee render of
> the same RAW with the same profile, neutral processing profile. **Once the
> level is normalized, the median deviation is 0.0027 out of 1.0**, i.e. less
> than one level in 255, and the channel ratios agree to within 0.007. The
> colorimetry is therefore no longer unvalidated: it agrees with an independent
> and mature implementation of the same specification.
>
> What remains, and what justifies keeping the "experimental" mention:
>
> * an overall **gain of ×1.083** (encoded space) persists, uniform across the
>   three channels — tonal, not chromatic. The investigation of 2026-08-03 found
>   a real white-level defect along the way
>   ([ADR 0066](0066-sensor-white-level.md)): the decoder let LibRaw choose that
>   level from the brightest pixel of each image. Fixing it **does not close
>   this gap** — a ~1.14 factor remains unattributed. What is established:
>   **it is not the colour**, which agrees to within 0.0027;
> * no comparison with Adobe itself, for want of an available converter.

## Consequences

* **The `process` field keeps its semantic legibility** (ADR 0028): the new
  number will mean exactly "DCP camera profile active", a single, readable fact,
  as `process: 3` means "distortion correction active".
* **Frozen neutral output**: without `camera_profile`, the stage is
  bit-for-bit the previous process version — ADR 0015's LibRaw-sRGB path,
  unchanged. The invariant "neutral value → operator entirely skipped"
  (`process3.rs`) stays true for the whole stage.
* **`leyline-color` becomes the home of two colour paths**: the output ICC
  transform (ADR 0027) and the input DCP application (here) — two pieces of
  colour domain logic, neither coupled to the engine's buffers, consistent with
  the role ADR 0027 gave it.
* **The DCP parsing dependency stays an open risk** for the PR: a minimal
  in-house parser or an existing crate, depending on what is available and
  licensable when the time comes. Colorimetric correctness must be validated
  against real Adobe DCPs and their reference renders before release
  (ADR 0016's bar).
* **The reproducibility of an external profile file is now covered** by the
  BLAKE3 checksum (ADR 0006) and §3.4's failure mode — a missing or modified
  profile never silently renders different pixels; it warns and writes nothing.
  The "same revision → same pixels" contract (`docs/pipeline.md` §5) holds for
  this new kind of referenced input.
* **The bundled profile database stays open for a future ADR** with its real
  costs (rights, hosting, maintenance): V2 only refuses to commit to it
  speculatively, it does not close the door.
* **One more `processN.rs` module** (ADR 0028): a bounded, known cost; no
  earlier version module is touched, the "same pixels in ten years" freeze stays
  mechanically unfalsifiable (`docs/pipeline.md` §3.3).

## Alternatives rejected

* **Bundling a DCP profile database Lensfun-style.** Rejected: DCP profiles are
  typically generated by the user body by body (calibration target) or
  downloaded individually, not an open community database that Leyline could
  bundle the way it bundles Lensfun's (ADR 0004/0016). Bundling such a database
  is a far larger and separate undertaking — data rights/licences, hosting,
  maintenance — frankly out of V2 scope. User-supplied files cover the real case
  ("my calibration for my body"); the bundled database will come back in its own
  ADR if it is ever wanted.
* **Auto-matching the profile by the camera's EXIF model** rather than an
  explicit stored reference. Rejected: fuzzy EXIF-string matching makes sense
  for Lensfun's **community database of thousands of profiles**
  (`lens_correction.profile: "auto"`), not for the **single** DCP file a user
  produced for their own body. An explicit reference (stored relative path) is
  simpler and more predictable than an EXIF guess when there is, in practice,
  one candidate per user's body.
* **Not checksumming the referenced profile file** (treating it as a
  configuration value, not as an input whose reproducibility matters). Rejected:
  a DCP is colorimetrically a render input just as much as the sensor pixels —
  `docs/pipeline.md` §5's contract requires an identical input resource (same
  checksum) to guarantee "same revision → same pixels". Without a checksum,
  replacing or editing the `.dcp` on disk would silently change the render of a
  revision believed frozen — exactly what the contract forbids. The BLAKE3
  checksum (ADR 0006) and §3.4's failure mode extend the existing guarantee to
  this new kind of referenced file, without inventing a new failure category.
* **Putting the camera-profile logic in a new dedicated crate** (e.g.
  `leyline-profile`) rather than extending `leyline-color`. Rejected: ADR 0027
  already made `leyline-color` a general colour transformation library; DCP
  application (matrix + LUT applied directly) is parallel colour domain work,
  with no coupling to the engine's buffers — unlike masking (ADR 0029), housed
  in `leyline-engine` precisely *because* it is coupled to the buffer's
  internals. Two features, two placement reasons: DCP goes where colour science
  already lives.
* **Placing the new stage after lens correction** rather than before. Rejected:
  the camera profile calibrates the sensor's **colour**, lens correction remaps
  **geometry** — no interaction. Putting it first makes it operate on the
  least-processed data (the just-decoded sensor RGB), the best point for a
  colorimetric calibration, and consistent with spot removal's early placement
  (ADR 0032). Putting it after the geometry would bring no colorimetric benefit
  and would introduce a needless ordering dependency between two operators that
  have none.
