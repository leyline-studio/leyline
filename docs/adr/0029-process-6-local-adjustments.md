# ADR 0029 — Process 6: masked local adjustments (brush, radial, gradient)

**Status:** Accepted — 2026-07
**Follow-up:** `local_adjustments::v1`, which this ADR creates, is no longer the
current version. [ADR 0048](0048-range-masks.md) adds range masks to it (`v2`),
then [ADR 0070](0070-stored-mask-coverage.md) **stored** coverages (`v3`) — a
mask is therefore no longer necessarily a geometry. The three families decided
here — brush, radial, gradient — and their coordinate frame
([ADR 0026](0026-mask-spot-coordinate-referential.md)) are unchanged.

## Context

`docs/v2-scope.md` §2 ("Local / masked adjustments") is V2's **founding** item:
it introduces the generic notion of a *mask* — a spatial coverage `[0,1]` per
pixel — over which a subset of `Settings` applies locally rather than globally.
§9 confirms it as the infrastructure on which spot removal (§5), masked dehaze
(§6) and regional colour grading (§3) rest. Today, every `Settings` applies
globally (`crates/leyline-core/src/settings.rs`); no spatially restricted
setting exists.

Three ADRs have already settled the cross-cutting locks that weighed on this
item, each explicitly "upstream" so as not to be re-derived here:

* **ADR 0026** fixes the coordinate frame of every mask geometry: normalized
  `[0,1]` relative to the image after rotation, before cropping — the same as
  `crop` — and requires the engine to carry it back to the pre-rotation buffer by
  the same backward-remapping family as `rotate`/lens correction. This document
  **does not reopen** that choice; it consumes it.
* **ADR 0028** fixes the versioning strategy: one process version per pixel
  feature, each in its own frozen `processN.rs` module, created by copying the
  previous module whole. This document **applies** that convention without
  re-litigating it.
* **ADR 0027** widens output colour management without touching the pipeline's
  internal working space — not directly relevant here, but it confirms that the
  internal rendering stays sRGB, the space where the tonal/colour operators (which
  the masks reuse) are defined.

What those three ADRs **left open** for item 2 itself (`docs/v2-scope.md` §2,
questions 2 and 3): the **storage** of masks (vector strokes in `settings_json`
vs a dedicated table), and **coalescing** (is a brush stroke an intent, or a
continuous gesture to be coalesced like a slider drag?). ADR 0026 explicitly
notes that those two points "remain to be settled by each feature's own ADR".
This document settles them, at the same time as it fixes the stage's place in
the pipeline, what a mask can adjust, and the crate placement.

The engine today numbers `CURRENT_PROCESS = 5`
(`crates/leyline-core/src/settings.rs`); this pixel addition therefore takes
**process 6**.

## Decision

Masked local adjustments are **process 6**, in a new module
`crates/leyline-engine/src/process6.rs`, a whole copy of `process5.rs`
augmented with the single new stage — exactly the per-module duplication
convention reaffirmed by ADR 0028. Three mask types are in scope: **brush**,
**radial** and **gradient (linear)**.

### Place in the pipeline

A new **"Local adjustments"** stage is inserted into the fixed order of
`docs/pipeline.md` §3.1 **immediately after Vibrance/Saturation and before
Noise reduction**. Local adjustments reuse exactly the same tonal/colour
operator mathematics as the global settings (exposure, contrast,
highlights/shadows, whites/blacks, white balance, vibrance/saturation), simply
re-parameterized per mask and blended by coverage. Running them as **one extra
masked pass, just after the equivalent global pass**, avoids propagating mask
awareness into every global operator's call site: it is the smallest correct
insertion, not a pipeline redesign.

> **Implementation note (not a spec edit here).** This ADR does **not** modify
> `docs/pipeline.md` §3.1's diagram. That diagram describes the pipeline **as
> implemented**; unlike lens correction (which was already a named but inert
> field since schema 1), this stage **does not yet exist** in the code. Per
> CLAUDE.md, the spec is updated in the same change as the actual
> implementation, not in this pre-decision ADR. The present document fixes only
> **where** the stage will land; §3.1's diagram and §3.3's process-version table
> will be amended by the PR that ships `process6.rs`.
>
> *(Done. This note describes the repository's state on the day of the decision:
> the stage has shipped, and `pipeline.md` was amended with it. The
> process-version table it announces no longer exists in that form — the revision
> now carries its stage map, [ADR 0042](0042-versioned-stage-pipeline.md).)*

### Coordinate frame

Taken from **ADR 0026 without modification**: each mask's geometry is stored in
normalized `[0,1]` coordinates relative to the image after rotation, before
cropping. The process 6 stage runs on a buffer still in the
decoded/lens-corrected orientation; it carries the geometry back to the
pre-rotation buffer by applying the **inverse** of the pending rotation, the
same backward-remapping technique as `rotate`/`crop` (`process2.rs:446`,
`:507`) and lens correction (`process3.rs`). No new decision here: ADR 0026 is
cited, not re-derived.

### What a mask can adjust

A restricted subset of `Settings`, reusing **exactly the same operator fields
and formulas** as their global equivalents: white balance (temperature/tint),
exposure, contrast, highlights, shadows, whites, blacks, vibrance, saturation.

**Explicitly out of scope** for a masked setting in V2: lens correction, noise
reduction, sharpening, rotation/cropping. They stay **global only** (see
*Consequences* and *Alternatives rejected* for the reasoning — the same spirit
as ADR 0016 cutting vignetting/TCA from process 3). They are either settings
that make no sense spatially restricted (rotation/cropping **define** the frame
itself), or settings that open far wider questions (spatially varying
sharpening/denoising kernels, lens correction interacting with a per-pixel
backward remapping that already happens at another stage) — none of which needs
solving to ship the basic masking infrastructure.

### Crate placement

**No new crate.**

* The masks' **geometry and value types** live in `leyline-core::Settings` —
  the same crate as `Crop`, `NoiseReduction`, `Sharpening`.
* **Rasterization** (mask → `[0,1]` coverage per pixel) and **compositing**
  live in `leyline-engine`, in a new module (e.g. `mask.rs`) consumed by
  `process6.rs` — the same pattern as `rotate`/`crop`, which live directly in
  the process modules.

An explicit contrast with `leyline-lens`: that crate is separate because it
wraps an external dependency (Lensfun) and an external profile database.
Masking wraps **nothing** external: it is pure geometry, tightly coupled to the
render buffer and its sampling. A crate boundary would separate two things that
need to share buffer/sampling internals, for no benefit to any consumer outside
the engine — Studio, the CLI and the SDK never call mask rasterization directly,
only `EditSession`.

### Storage — resolves `docs/v2-scope.md` §2 question 2

Everything lives in `settings_json`, **no dedicated table**.

* **Parametric** masks (radial, gradient) are a few floats each — trivially
  compact.
* **Brush** masks store the **stroke list** (ordered points, each with
  x/y/radius/flow/hardness), **never a rasterized bitmap** — reproducible and
  portable, consistent with `docs/catalog.md` §17 ("one revision = a complete,
  self-contained state").

As ADR 0028 reasons about process-module proliferation: if stroke lists ever
became a real volume problem, that will be a future ADR's problem **with real
data**, not something to solve speculatively today.

### API extension — resolves `docs/v2-scope.md` §2 question 3

**No new mechanism.** A mask's whole life cycle (create/edit/delete) is
expressed by the existing `set`/`commit` plus two variants added to the enums
already in place (`session.rs`):

* `Param` gains **`LocalAdjustment(usize)`**, where the index addresses a mask's
  position in the current `local_adjustments` array. That variant groups the
  mask's geometry **and** its adjustment sliders as a single coalescing unit —
  exactly the pattern already used by `Param::WhiteBalance`, documented as
  "*White balance override (temperature + tint together: one tool)*". A mask is
  "one tool" in the same sense.
* `Value` gains **`LocalAdjustment(Option<LocalAdjustment>)`**: `Some` replaces
  the mask's whole definition (whole-struct replacement, the same pattern as
  `Value::Crop(Option<Crop>)`/`NoiseReduction`/`Sharpening` — no partial field
  patch exists anywhere in this API); `None` **deletes** that mask (mirroring
  `Crop`, whose `None` = full frame, and `WhiteBalance`, whose `None` = back to
  "as shot").

Coalescing reuses **the existing rule unchanged**:

* A **complete brush stroke** — mouse down to release — is exactly **one commit
  point** under the existing rule of `docs/catalog.md` §17 ("*the user releases a
  control (end of drag)*"). No new rule is invented.
* Successive edits to the **same mask index** within the existing 2-second
  amendment window amend the head revision — exactly the per-`Param` amendment
  rule already realized by `session.rs`'s `Pending::One(Param)` mechanism,
  applied as it stands to the new variant.

**Explicit consequence: no new coalescing mechanism, no new `EditSession`
method** (no `add_mask`/`remove_mask`). The session surface stays as minimal
(`set`/`commit`/`undo`/`redo`) as it is today.

### Composition / rendering

Masks apply **sequentially in array order** — array order is the **only**
stacking order (no z-index field and no separate identifier: position in the
array is the sole ordering mechanism, in the image of the fixed pipeline itself,
which has no reordering concept beyond its declared structure).

For each mask, in order:

1. rasterize its coverage (`[0,1]` per pixel, carried back to the pre-rotation
   buffer by ADR 0026);
2. multiply by the mask's opacity;
3. blend:
   `output = lerp(buffer, apply_local_operators(buffer, mask.adjustments), coverage)`.

The next mask reads this mask's output. `apply_local_operators` reuses the
**same per-pixel operator formulas** as the global ones (re-parameterized per
mask), **not** a new algorithm: that is why process 6 introduces **no new tonal
mathematics**, only a masked application of existing mathematics.

An **absent or empty** `local_adjustments` array skips the whole stage, output
**bit-for-bit identical** to process 5's — which preserves the invariant "*a
parameter at its neutral value skips its operator entirely, so the neutral
rendering is bit-for-bit the decoded image*" documented at the head of
`process3.rs`.

### Schema

**Additive**: an optional `local_adjustments` array, absent/empty = neutral.
**No schema bump required**, consistent with the additive "process +1, schema
unchanged" scheme of most V2 items (`docs/v2-scope.md` §1) — fields unknown to
an older engine are already preserved verbatim (`Settings::extra`,
`leyline-core/src/settings.rs`).

### Presets — an explicit scope cut

`SettingsGroup` (`docs/engine-api.md` §10.3,
`crates/leyline-core/src/settings.rs`) **does not gain** a `LocalAdjustments`
variant in V2. Mask geometry is **composition-specific**: a radial filter
positioned for one photo's subject makes no sense applied verbatim to another
photo — unlike the purely numeric offsets of `Tone`/`Presence`, which really do
transfer from one photo to another. It is a deliberate cut, not an oversight
(the same spirit as `SettingsGroup::Geometry`, already excluded from presets by
default because "*geometry is a per-photo judgment, not a reproducible style*").

### JSON sketch

One instance of each type in `local_adjustments`, plus the neutral case. The
style follows `docs/pipeline.md` §3.2:

```json
{
    "schema": 1,
    "process": 6,

    "exposure": 0.35,
    "vibrance": 18,

    "local_adjustments": [
        {
            "mask": {
                "type": "radial",
                "cx": 0.5, "cy": 0.42,
                "rx": 0.30, "ry": 0.22,
                "angle": 0.0,
                "feather": 0.40,
                "inverted": false
            },
            "opacity": 1.0,
            "adjustments": { "exposure": 0.6, "contrast": 15, "highlights": -20 }
        },
        {
            "mask": {
                "type": "gradient",
                "x0": 0.5, "y0": 0.0,
                "x1": 0.5, "y1": 0.35
            },
            "opacity": 0.8,
            "adjustments": { "exposure": -0.8, "whites": -10, "temperature": 5200, "tint": 6 }
        },
        {
            "mask": {
                "type": "brush",
                "strokes": [
                    { "x": 0.20, "y": 0.60, "radius": 0.04, "flow": 1.0, "hardness": 0.5 },
                    { "x": 0.23, "y": 0.61, "radius": 0.04, "flow": 1.0, "hardness": 0.5 },
                    { "x": 0.26, "y": 0.62, "radius": 0.04, "flow": 1.0, "hardness": 0.5 }
                ]
            },
            "opacity": 1.0,
            "adjustments": { "saturation": -30, "shadows": 20 }
        }
    ]
}
```

Neutral case — array absent (or `"local_adjustments": []`), render bit-for-bit
identical to process 5:

```json
{
    "schema": 1,
    "process": 6,
    "exposure": 0.35
}
```

Each `mask` carries the white balance fields in `adjustments` in the same form
as the global `WhiteBalance` (`temperature`/`tint`), and the `[-100, +100]`
sliders in the same form as their global equivalents: vocabulary reuse, no new
units.

## Consequences

* **The `process` field keeps its semantic legibility** (ADR 0028):
  `process: 6` will mean exactly "masked local adjustments active", a single,
  readable fact, as `process: 3` means "distortion correction active".
* **Frozen neutral output**: with no mask, process 6 is bit-for-bit process 5.
  The invariant "neutral value → operator entirely skipped" (`process3.rs`) stays
  true, mechanically, for the whole stage.
* **No new session surface**: masks created/edited/deleted entirely through
  `set`/`commit` plus the two `Param`/`Value` variants. The rest of the engine
  (jobs, events, coalescing, amendment) is untouched.
* **The cut in maskable settings** (lens, denoising, sharpening,
  rotation/cropping stay global) leaves those four undertakings open for a future
  ADR, without blocking the basic infrastructure. A masked denoising/sharpening
  setting would require spatially varying kernels; a masked lens correction would
  interact with the backward remapping already under way at the lens correction
  stage — real questions, but not necessary to ship the core.
* **The preset cut** means styles stay transferable (numeric offsets) without
  dragging non-transferable geometry along; regional colour grading
  (`docs/v2-scope.md` §3, §4) will be able to reopen the question of regional
  presets when the time comes, with its own ADR.
* **One more `processN.rs` module** (ADR 0028): a bounded, known cost, the
  trajectory stays linear. No earlier version module is touched; the "same pixels
  in ten years" freeze stays mechanically unfalsifiable (`docs/pipeline.md` §3.3).
* **The reproducibility contract** (`docs/pipeline.md` §5) is respected: brush
  masks store deterministic vector strokes (never a render-dependent raster),
  rasterization and compositing are pure and deterministic like any operator
  (ADR 0012), and everything serializes into `settings_json` — "same revision →
  same pixels".
* **The `docs/pipeline.md` spec (§3.1, §3.3) is not edited by this ADR**: it
  will be by the implementation PR, per CLAUDE.md.

## Alternatives rejected

* **Propagating masking into every global operator instead of a post-pass
  stage.** One could have made every global operator (exposure, contrast…)
  mask-aware at its own call site, rather than adding a masked pass after
  Vibrance/Saturation. Rejected: it would scatter mask logic across a dozen call
  sites, each having to sample the coverage and blend, when the operator
  mathematics is already written and frozen; one extra pass, reusing those same
  re-parameterized formulas, is the smallest correct insertion. It would also
  multiply the points where a regression could alter the global rendering —
  exactly what per-module duplication (ADR 0028) exists to avoid.
* **A dedicated brush-stroke table instead of `settings_json`.** A blob or a
  per-revision table for the strokes would solve a hypothetical volume problem.
  Rejected: it breaks "one revision = a complete, self-contained state"
  (`docs/catalog.md` §17), the foundation of portability and reproducibility, in
  favour of an optimization no real data shows a need for. As ADR 0028 puts it
  for module proliferation: if volume becomes a real problem one day, that will
  be a future ADR's problem with real data.
* **New dedicated `EditSession` methods (`add_mask`/`remove_mask`/…) instead of
  extending `Param`/`Value`.** Rejected: it would duplicate the
  coalescing/amendment mechanism — every new method would have to re-decide
  amendment vs new revision. A `Param::LocalAdjustment(usize)` variant inherits
  the whole existing `Pending::One`/amendment-window policy (`session.rs`) for
  free, exactly as `Param::WhiteBalance` already groups two values (temperature +
  tint) into one tool. The session surface stays minimal and coalescing behaviour
  uniform across all parameters.
* **Including lens / denoising / sharpening / rotation-cropping in V2's
  maskable set.** Rejected as a deliberate cut (the spirit of ADR 0016 cutting
  vignetting/TCA from process 3). Rotation and cropping define the frame itself:
  "masked" has no meaning there. Masked denoising and sharpening would require
  spatially varying kernels — a substantial algorithmic problem. Lens correction
  would interact with the per-pixel backward remapping already applied at another
  stage. None of those four is required for the basic masking infrastructure;
  including them would swell process 6 with unresolved questions and delay the
  foundation items 3/5/6 depend on.
* **A new `leyline-mask` crate parallel to `leyline-lens`.** Rejected:
  `leyline-lens` is a separate crate because it wraps an external dependency
  (Lensfun) and an external profile database. Masking wraps nothing external — it
  is pure geometry, tightly coupled to the engine's buffer and sampling
  internals, consumed only by `process6.rs`. A crate boundary would separate two
  things that must share those internals, with no benefit to any consumer outside
  the engine (Studio/CLI/SDK only call `EditSession`). A `leyline-engine::mask`
  module is the right grain, as `rotate`/`crop` live directly in the process
  modules.
* **Treating "graduated filter" and "linear gradient" as two distinct types.**
  `docs/v2-scope.md` §2 lists them separately. Rejected as a fourth type:
  Lightroom uses both names for a single mechanism — a linear gradient feathered
  across the frame. The scope is therefore three types (brush, radial, gradient),
  not four; that is a scope clarification, not a type invented to match the
  letter of the wording.
