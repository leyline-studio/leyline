# ADR 0049 — Exposing local adjustments to the clients: drawing tools in Studio, a JSON payload in the CLI

**Status:** Accepted — 2026-07

## Context

[ADR 0029](0029-process-6-local-adjustments.md) delivered the masks (brush,
radial, graduated) and [ADR 0048](0048-range-masks.md) their refinement by
range. Both are **entirely implemented in the engine**: `Mask`,
`LocalAdjustment`, `RangeMask`, `mask.rs`'s rasterization, the
`local_adjustments::v1` and `v2` stages, and the driving through
`Param::LocalAdjustment(index)`.

No client exposes them. Not Studio (no occurrence of `local_adjustments` under
`crates/leyline-studio/`), and not the CLI (likewise). ADR 0048 §6 wrote it in
so many words and deferred the question to "a piece of work in its own right":
this is it.

The gap is the project's most serious. Local retouching is the central function
of darktable and Lightroom, and here it is reachable only by writing JSON by
hand through the SDK. Code that is written, tested and frozen serves nobody.

**What is not at issue.** The data model, the coordinate frame
([ADR 0026](0026-mask-spot-coordinate-referential.md)), the stages' semantics,
the SDK boundary. This decision touches only the two clients: it adds no
setting, no stage and no version.

## Decision

### 1. A local adjustment is a *tool*, not a slider

The develop panel exposes a list of adjustments, not a set of sliders: each
entry of `Settings::local_adjustments` is a row, selectable, deletable, and
whose values are set below it. That is Lightroom's and darktable's shape, and
it is the one the model already imposes — a `Vec<LocalAdjustment>` driven by
index.

The geometry is **drawn on the image**, never typed: a tool active in the bar
above the preview, as *Crop* and *Spot Removal* already do (ADR 0032). Three
gestures, one per geometry:

| Tool | Gesture | What it writes |
| :--- | :--- | :--- |
| Radial | a drag | the ellipse inscribed in the dragged rectangle, `angle: 0` |
| Graduated | a drag | the axis: press = full coverage, release = zero coverage |
| Brush | clicks | one `BrushStroke` per click, added to the stroke |

`Mask::Everything` (ADR 0048 §1) has no geometry to draw: it is added from the
panel, by a button, which is exactly the intended use — a range with no
geometry.

### 2. The gesture creates the entry; there is no empty entry

`Settings::validate()` refuses a `Mask::Brush` with no dab. A brush therefore
cannot be created before its first stroke, and the interface does not pretend
otherwise: the Brush tool's first dab **creates** the adjustment, and the
following ones lengthen it. Radial and graduated, whose geometry is complete on
release, follow the same rule for consistency: the drag creates.

A selected entry is redrawn rather than duplicated — the same drag on an
already-selected radial adjustment replaces its geometry. Without that,
correcting a badly placed radial would require deleting it first.

### 3. Neutral value = *absent*, except white balance

`LocalAdjustmentValues` is a set of `Option`s: `None` means "no change here",
which is not the same as "0". For the eight fields whose neutral *is* zero
(exposure, contrast, highlights, shadows, whites, blacks, vibrance,
saturation), the distinction has no rendering consequence, and a slider brought
back to zero therefore writes `None`: the `settings_json` stays clean, and the
adjustment enumerates only what it changes.

`temperature` and `tint` do not have that property — `temperature: 0` is
refused by `validate()`, and 0 is not a neutral tint. The pair is therefore
driven by an explicit switch, which seeds it with the photo's global
temperature when turned on and sets it back to `None` when turned off. The same
reasoning for `RangeMask`'s two terms, for which `None` is the only way to say
"no term".

### 4. The CLI takes the stored JSON, not a grammar of its own invention

```
leyline develop <lib> <ver> local-adjustment <json|@file>
leyline develop <lib> <ver> local-adjustment rm <index>
leyline develop <lib> <ver> local-adjustment reset
```

The payload is **exactly** the serialized form of a `LocalAdjustment`, the one
`settings_json` contains and `docs/pipeline.md` §3.2 documents. A positional
grammar in the manner of `spot-removal` would demand seven to twelve fields in
an order to be remembered, plus a nested syntax for `range` — that is, a second
dialect to document, to validate and to age in parallel with the first.

The JSON is validated by `Settings::validate()` like any setting: a malformed
or out-of-range payload is a named error, not a silence. `@file` reads the
payload from disk, because a brush mask with thirty dabs does not fit on a
command line.

### 5. What the interface shows of the coverage

Studio draws the **outline** of the selected geometry on the preview — a
radial's ellipse, a graduated filter's axis, a brush's dabs — computed on the
UI side from the stored geometry, asking nothing of the engine.

The outline does not reflect an ellipse's rotation (`angle`): the tools never
write one — a two-corner drag has no rotation to report (§1) — and a tilted
ellipse written by the CLI or the SDK therefore shows its outline unrotated.

It does **not** draw the real coverage as an overlay (Lightroom's "red mask").
That coverage includes the range term, and therefore depends on the pixels:
producing it would demand a mask rendering from the engine, that is, a new
render output to specify, to cache and to scale like the preview (ADR 0041).
That is an engine decision, separable, and its absence does not block the
gesture: the outline is enough to know *where* one has drawn.

### 6. Out of scope

* **The engine-computed coverage overlay** (§5), including for the range term.
* **Range selection by eyedropper** on a designated pixel (DxO's control
  points), already named as feasible with no engine decision by ADR 0048 §6 —
  it requires an eyedropper, and hence its own slice.
* **Moving an already-drawn geometry by handles.** Redrawing replaces (§2);
  handles are pure ergonomics, addable afterwards.
* **Copying local adjustments between photos.** `SettingsGroup`
  (`docs/presets.md` §3.1) has no group for them, and adding one touches the
  presets, not the clients.

## Consequences

* **The engine stops having unreachable functions.** The two ADRs most costly
  in pixels (0029, 0048) become usable by a photographer, which was their
  point.
* **Develop's toolbar goes from three tools to six**, on the same `active-tool`
  mechanism: no new interaction path and no new coordinate convention —
  `letterbox_unit`'s letterbox serves the three new gestures as it serves crop
  and spot.
* **The decoding rules stay pure and tested** in `crate::masks` (Studio), free
  of any Slint type, as `crate::develop` already is.
* **The CLI gains a JSON payload, which no other command had.** That is
  accepted and bounded to this case: the justification (§4) is the structure's
  depth, not convenience.
* **`ui/state/mask.slint` is the eighth domain global** (ADR 0045 §1). It
  carries the list of adjustments, the brush's three fields **and the selected
  row**: the last crosses the boundary because a gesture that *creates* an
  adjustment must select it from Rust (§2). The active tool and the collapse
  states, which Rust never reads, stay private to the panel.

## Alternatives rejected

* **A positional grammar in the CLI**, by symmetry with `spot-removal`.
  Rejected in §4: a second dialect for the same structure.
* **Numeric fields in Studio for the geometry** (cx, cy, rx, ry…). Quick to
  write, unusable: nobody places a radial by typing percentages. Drawing is the
  gesture, which is precisely why the competitors offer nothing else.
* **A "disabled" toggle per field** rather than §3's "neutral = absent" rule.
  Twice as many controls to distinguish two states whose rendering is
  identical.
* **Waiting for the coverage overlay before shipping the interface.** That
  would keep the software's central function unreachable while waiting for an
  independent engine decision. The outline (§5) is what makes drawing usable;
  the rest is a refinement.
* **Exposing masks in the CLI only**, while waiting for an interface redesign.
  The CLI does not serve the gesture: drawing a brush in JSON is not
  retouching.
