# ADR 0032 — Spot removal: cloning alone, a deterministic bilinear copy, early in the pipeline

**Status:** Accepted — 2026-07

## Context

`docs/v2-scope.md` §5 ("Spot removal / healing") notes that no clone or heal
tool exists today for removing sensor dust and blemishes. It is an
**intrinsically local** item: like masked adjustments (§2), it stores geometry
drawn on the displayed image and therefore shares the coordinate-frame lock
already settled.

Three cross-cutting decisions are **consumed here, not relitigated**:

* **ADR 0026** fixes the frame of all local-correction geometry: normalized
  `[0,1]` relative to the image after rotation and before crop — the same as
  `crop` — lifted back to the pre-rotation buffer by the same family of
  backward remapping as `rotate` (`process2.rs:446`) and lens correction
  (`process3.rs:180`). ADR 0026 explicitly notes that spot removal reuses that
  choice; this document **consumes** it, it does not re-derive it.
* **ADR 0028** freezes the versioning strategy: one process version per pixel
  feature, each in its own frozen `processN.rs` module, created by copying the
  previous module whole. This spot removal is a pixel operator: it therefore
  takes a new process version, without this ADR having to re-choose the
  convention.
* **ADR 0029** established, for masked local adjustments, the API extension
  pattern (extended `Param`/`Value` rather than new `EditSession` methods) and
  the principle "local geometry in `settings_json`, no dedicated table". This
  document **applies** those precedents to the spot, without redesigning them.

§5 left two questions open specific to the item (the determinism of a *heal*
correction, and automatic source selection); this document settles them at the
same time as it fixes the mode, the placement, the sampling and the storage.

## Decision

Spot removal is a new pixel operator, and therefore a **new process version**:
it takes **the next available process number at the time this feature ships**
(ADR 0028), in its own `processN.rs` module, a complete copy of the previous
module plus the cloning stage alone — exactly the per-module duplication
convention reaffirmed by ADR 0028. This ADR **does not freeze** a specific
process integer: the shipping order of items 3/4/5/6/8 belongs to the future
implementation plan.

### The central simplification — cloning alone, healing is cut from V2

**V2 ships the clone mode only**: a deterministic, softened copy from a source
point to a target point. The *heal* mode (seamless cloning, a Poisson-equation
style blend) is **removed from V2's scope — cut as a mode, not deferred as a
flag**.

The reasoning, in the same spirit as ADR 0016 cutting vignetting and TCA from
process 3 as "not trivial to validate without reference images at hand": a
*heal* requires **solving a Poisson blending equation per spot**, an
algorithmic commitment materially heavier and riskier than a deterministic
bilinear copy, whose quality and correctness cannot be validated responsibly
**without reference images available** — the exact bar ADR 0016 already set
for that kind of claim in this project. Cloning (a deterministic copy,
softened by a radial falloff, from a source point to a target point) covers
the use case — sensor dust, a point blemish — that `docs/v2-scope.md` §5's gap
analysis actually named.

A schema consequence: the `spot_removal[].mode` field sketched in §5
(`"clone"|"heal"`) **is abandoned**. There is only cloning in V2, so **no
`mode` field is needed** — simpler than keeping a `mode: "clone"` field with a
single legal value.

### Place in the pipeline — early, before the tonal block

A new **"Spot removal"** stage fits into the fixed order of
`docs/pipeline.md` §3.1 **immediately after Lens correction and before White
balance** — the very first stage adjacent to the tonal block. It thus operates
on the **least processed** data (close to the decoded/lens-corrected linear),
for the best possible clone quality — which the "Pipeline (§3.1)" row of
`docs/v2-scope.md` §5's table already fixes ("early in the chain… to operate
on data close to linear"). This ADR **confirms and consumes** that placement.

That point is **further upstream** than ADR 0029's "Local adjustments" stage,
which fits in after Vibrance/Saturation. The two V2 features land at
**different** points of the fixed order, and there is **nothing to reconcile**
between them: under ADR 0028's per-feature versioning, whichever ships first
adds its stage at its own position, and the second does likewise,
independently — no shared module, no schedule synchronization, no imposed
shipping order.

### Coordinate frame

Taken from **ADR 0026 unchanged**: each spot's target and source points are
stored in normalized `[0,1]` coordinates relative to the image after rotation
and before crop — the same frame as ADR 0029's mask geometry and as `crop`.
The stage runs over a buffer still in the decoded/lens-corrected orientation;
it lifts both points back to the pre-rotation buffer by applying the
**inverse** of the pending rotation, the same backward remapping technique as
`rotate`/`crop` (`process2.rs:446`, `:507`) and lens correction
(`process3.rs:180`). No new decision: ADR 0026 is cited, not re-derived.

### Clone sampling — a deterministic bilinear copy

For every spot, in table order, the engine copies the disc centred on the
**source** point onto the disc centred on the **target** point, resampling by
**bilinear interpolation** through the `bilinear` function **already present**
in the process module (`process2.rs:482`, `process3.rs:549`) — the `n + 0.5`
convention, Leyline's own, of `rotate`/`crop` (and not `lens_bilinear`,
reserved for Lensfun's integer convention). Three parameters modulate the
copy:

* **`radius`** — the radius of the copied disc (normalized coordinates, the
  same convention as the rest of the geometry);
* **`feather`** — a **radial falloff at the disc's edge**: the copied patch
  blends in smoothly instead of presenting a hard-edged circle, the copied
  fraction decreasing from centre to edge according to `feather`;
* **`opacity`** — the overall strength of the patch's blend onto the
  background.

Every target pixel receives `lerp(background, source_sample, coverage)`, where
the coverage combines the radial falloff and the opacity. The copy is **pure
and deterministic** (ADR 0012): the same points, the same parameters, the same
pixels. The exact shape of the falloff curve is a constant of the
implementation PR, at the same level of precision as ADR 0016/0030/0031; only
the **family** — a bilinear copy softened by a radial falloff — is frozen
here.

### Source selection — manual only, no engine suggestion

The source is placed **explicitly by the user**: they place the target point
**and** the source point themselves. **No engine feature for automatic source
suggestion exists in V2.**

The reasoning: §5's open question #2 already flagged that an engine-proposed
source would have to be **deterministic and recorded**, never recomputed at
render time (`docs/pipeline.md` §5). Cutting automatic suggestion from V2
avoids designing that determinism/recording mechanism prematurely.

> **Forward guidance (not a mechanism designed here).** If a source suggestion
> is ever added, the same rule will apply: whatever the engine proposes is
> **written into `spot_removal[].source`** like any other value, never left
> implicit nor recomputed on the fly. The suggestion would be nothing but a
> client-side input aid filling an existing field, not a separate render path.

### Storage — an additive schema, no dedicated table

`spot_removal` is an optional **list** in `settings_json`, each entry
`{ target: {x,y}, source: {x,y}, radius, feather, opacity }`. **Absent or an
empty list means neutral** (no spot), rendered **bit-for-bit identically** to
the previous process version — the invariant "*a parameter at its neutral
value skips its operator entirely, so the neutral rendering is bit-for-bit the
decoded image*" documented at the head of `process3.rs:25`. No schema bump is
required, consistent with the additive "process +1, schema unchanged" pattern
of most V2 items (`docs/v2-scope.md` §1) — fields unknown to an older engine
are preserved verbatim (`Settings::extra`,
`crates/leyline-core/src/settings.rs`).

The choice of "a list in `settings_json`, **no dedicated table**" is not
reopened here: the "Catalog" row of `docs/v2-scope.md` §5's table already
fixed it (a compact list, consistent with "a revision is a complete,
self-contained state", `docs/catalog.md` §17). This document merely confirms
it. As ADR 0029 and ADR 0028 put it: should the volume of those lists one day
become a real problem, it will be a future ADR's problem **with real data**,
not a speculative optimization today.

### API extension — ADR 0029's `Param`/`Value` pattern

**No new mechanism.** A spot's complete life cycle (add/move/delete) is
expressed through the existing `set`/`commit` plus the same enum-extension
pattern ADR 0029 established for masks (`session.rs`): a `Param` variant
indexing a spot's position in the `spot_removal` array as the **coalescing
unit**, and a `Value` variant replacing or removing the complete entry at that
index (`Some` = a complete placement, `None` = removal, the same pattern as
`Value::Crop(Option<Crop>)`). Placing a spot in full — from placing the point
to releasing it — is **one commit point** under the existing rule of
`docs/catalog.md` §17; successive edits of the **same index** within the
existing amendment window amend the head revision, exactly as ADR 0029's
`Param::LocalAdjustment(usize)` does. This document does not re-derive that
mechanism: it points at ADR 0029 as the precedent and applies it.

### A JSON sketch

The style follows `docs/pipeline.md` §3.2. One spot, then the neutral case:

```json
{
    "schema": 1,
    "process": 6,

    "exposure": 0.2,
    "spot_removal": [
        {
            "target": { "x": 0.62, "y": 0.31 },
            "source": { "x": 0.55, "y": 0.29 },
            "radius": 0.03,
            "feather": 0.40,
            "opacity": 1.0
        }
    ]
}
```

The neutral case — the field absent (or `"spot_removal": []`), rendered
bit-for-bit identically to the previous process version:

```json
{
    "schema": 1,
    "process": 6,
    "exposure": 0.2
}
```

> *The `process: 6` above is purely illustrative: the real number is whatever
> is next available at shipping time (ADR 0028), not fixed by this ADR.*

> **An implementation note (not a spec edit here).** This ADR does **not**
> modify `docs/pipeline.md` §3.1's diagram nor §3.3's process-version table. As
> for ADR 0029/0030/0031, the spec is updated in the same change as the actual
> implementation, in keeping with CLAUDE.md. The present document fixes only
> **where** the stage lands and **which** mathematics it freezes; the §3.1
> diagram and the §3.3 table will be amended by the PR that ships the process
> module.

## Consequences

* **The `process` field keeps its semantic legibility** (ADR 0028): the new
  number will mean exactly "clone spot removal active", a single readable
  fact, as `process: 3` means "distortion correction active".
* **A frozen neutral output**: with no spot, the stage is bit-for-bit the
  previous process version. The invariant "a neutral value → the operator
  entirely skipped" (`process3.rs:25`) stays true for the whole stage.
* **Healing stays open for a future ADR** with reference images available: the
  cut does not close the door, it only refuses to commit to a Poisson blend
  the project cannot validate today. The item's complexity therefore falls
  from "**M** (clone) to **L** (heal)" (`docs/v2-scope.md` §5) to **M** alone.
* **Automatic source suggestion stays open** for a future ADR; if it arrives,
  it writes into `spot_removal[].source` like any other value, never
  recomputed at render time (`docs/pipeline.md` §5).
* **No new session surface**: spots are created, moved and deleted entirely
  through `set`/`commit` plus ADR 0029's `Param`/`Value` extension. The rest of
  the engine (jobs, events, coalescing, amendment) is untouched.
* **Two local V2 features at distinct points of the pipeline**: the spot early
  (before White balance), masked adjustments late (after Vibrance/Saturation,
  ADR 0029). Neither depends on the other, neither waits for the other —
  per-feature versioning (ADR 0028) guarantees it.
* **The reproducibility contract** (`docs/pipeline.md` §5) is respected: every
  parameter (target, source, radius, feather, opacity) is explicit and
  recorded, the sampling is pure and deterministic (ADR 0012), and there is no
  source of randomness or guessing on the engine side anywhere in V2's clone
  path — everything serializes into `settings_json`, "the same revision → the
  same pixels".
* **One more `processN.rs` module** (ADR 0028): a bounded and known cost; no
  earlier version's module is touched, and the "same pixels in ten years"
  freeze stays mechanically unfalsifiable (`docs/pipeline.md` §3.3).
* **The `docs/pipeline.md` spec (§3.1, §3.3) is not edited by this ADR**: it
  will be by the implementation PR, in keeping with CLAUDE.md.

## Alternatives rejected

* **Shipping *heal* in V2 alongside cloning.** Rejected — it is this ADR's
  central cut, in the spirit of ADR 0016 cutting vignetting and TCA. A *heal*
  requires solving a Poisson blending equation per spot: an algorithmic
  commitment materially heavier than a deterministic bilinear copy, whose
  quality cannot be validated responsibly without reference images available
  — ADR 0016's exact bar. Cloning covers the use case actually named (sensor
  dust); *heal* will be added in its own ADR the day the project can validate
  its rendering. Cut as a **mode**, not deferred as a flag: the schema does
  not drag along a `mode` field with a single value.
* **Automatic source suggestion on the engine side.** Rejected for V2: it
  would require designing right now the determinism and recording mechanism
  §5's open question #2 demands (a proposed source must be deterministic and
  written into the parameters, never recomputed at render time). Manual
  selection sidesteps that work; if suggestion is added later, it will simply
  fill `spot_removal[].source` as an input aid, with no separate render path
  and no implicit value.
* **Placing the stage after ADR 0029's local adjustments, instead of early in
  the pipeline.** Rejected: cloning gives its best quality on data close to
  the decoded/lens-corrected linear, before the tonal block stretches the
  values (`docs/v2-scope.md` §5). Placing it late would make it copy
  already-toned pixels, changing which pixels also feed noise reduction and
  sharpening. The two stages have no need to be adjacent: ADR 0028 allows them
  distinct fixed positions with no reconciliation.
* **New dedicated `EditSession` methods (`add_spot`/`remove_spot`/…) instead
  of extending `Param`/`Value`.** Rejected for the same reason ADR 0029
  rejected it for masks: it would duplicate the coalescing/amendment
  mechanism, each method having to re-decide amendment versus new revision.
  Indexing the `spot_removal` array through a `Param` variant inherits the
  whole existing `Pending::One`/amendment-window policy for free
  (`session.rs`). The session surface stays minimal
  (`set`/`commit`/`undo`/`redo`) and the coalescing behaviour uniform across
  every parameter.
