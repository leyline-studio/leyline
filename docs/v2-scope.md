# V2 scope — candidate features

**Document:** `docs/v2-scope.md`
**Version:** 0.1
**Status:** Exploration — **scoping largely done since** (see the state below)

---

## The state of this document

> **Mind the tense.** This document was written when the seven features below were absent, and it speaks of them in the present tense ("no local adjustment today"). All of them have since been decided **and implemented**. Its analysis remains useful — it is the reasoning that led to the corresponding ADRs — but it no longer describes the state of the product. For that state, see [`specification.md`](specification.md).

| § | Feature | State |
|---|---|---|
| 2 | Local / masked adjustments | ✅ Delivered — process 8, [ADR 0029](adr/0029-process-6-local-adjustments.md) |
| 3 | Tone curve | ✅ Delivered — process 6, [ADR 0030](adr/0030-tone-curve.md) |
| 4 | Colour grading / HSL mixer | ✅ Delivered — process 9, [ADR 0031](adr/0031-hsl-color-grading.md) |
| 5 | Spot removal | ✅ Delivered — process 7, cloning only, [ADR 0032](adr/0032-spot-removal-clone.md) |
| 6 | Dehaze / texture / clarity | ✅ Delivered — process 10, [ADR 0033](adr/0033-clarity-texture-dehaze.md) |
| 7 | Soft proofing, watermark, printing | ✅ Delivered — printing ([ADR 0036](adr/0036-print-module.md)), then soft proofing and text watermark ([ADR 0034](adr/0034-softproofing-watermark-print.md), [ADR 0051](adr/0051-watermark-rasterization-and-soft-proof-surface.md)); the image watermark stays cut |
| 8 | Camera profiles beyond Lensfun | ✅ Delivered — process 11, **experimental**, [ADR 0035](adr/0035-camera-profile-dcp.md) / [ADR 0037](adr/0037-dcp-parsing-dependency.md) |

---

# 1. Purpose

The V1 scope (`docs/specification.md` §1) is **delivered and closed**.

This document does not reopen V1. It scopes, at the architectural level, seven features that were **missing** at the time it was written — without being excluded by design. V1's deliberate exclusions (`docs/specification.md` §4 (Deliberate exclusions): cloud, accounts, AI, HDR, panorama, face recognition, synchronisation) stay off topic here. The eight items below are of another nature: they are the classic expectations of a RAW developer, simply not built yet.

Each section describes what the item requires of the **render contract** (`docs/pipeline.md` §3.2/§3.3: the format `schema` vs. the render `process`), its place in the **pipeline order** (§3.1), the owning **crate** or crates, the **catalog** (`docs/catalog.md`) and **engine API** (`docs/engine-api.md`) implications, a complexity reading (S/M/L/XL) and the open questions.

This is **not** an implementation plan or a phase schedule: it is a context analysis parallel to the one an ADR lays out before deciding. No decision is taken here — so no ADR accompanies this document (`docs/adr/README.md`: an ADR records an **accepted** decision). §9 notes, for each item, whether it deserves an ADR of its own once settled.

The contract reminders used throughout below:

* a step that **changes the pixels** of a revision requires a new **process version** (`docs/pipeline.md` §3.3), including a simple insertion into the pipeline order (§3.1);
* adding an **optional parameter with a neutral value** is compatible and does **not** increment the `schema` (§3.4) — fields unknown to an old engine are already preserved verbatim (`Settings::extra`, `leyline-core/src/settings.rs`);
* most of the items below are therefore **process +1, schema unchanged**: additive structure is tolerated, only the render moves.

---

# 2. Local / masked adjustments

No local adjustment today: brush, radial filter, graduated filter, linear gradient are all absent. Every `Settings` applies globally.

This is the **founding** item: it introduces the generic notion of a *mask* (a per-pixel spatial coverage in `[0,1]`) on top of which a subset of settings applies locally. Items 4 (retouching), 5 (masked dehaze) and regional colour grading (item 3) rest on it naturally.

| Aspect | Analysis |
|---|---|
| Contract | **Process +1** (it changes pixels). **Schema: additive** — an optional `local_adjustments` array, absent = neutral; no increment strictly required, though a deliberate increment is defensible for a structural addition of this size. |
| Pipeline (§3.1) | **Settled, [ADR 0029](adr/0029-process-6-local-adjustments.md): process 6**, a **Local adjustments** stage inserted immediately after Vibrance/Saturation and before Noise reduction — a masked pass reusing the global operator formulas. The coordinate frame is `crop`'s ([ADR 0026](adr/0026-mask-spot-coordinate-referential.md)). The §3.1 diagram will be amended by the implementation PR, not by the ADR. |
| Crate | **Settled, [ADR 0029](adr/0029-process-6-local-adjustments.md): no new crate.** Geometry and values in `leyline-core` (`Settings`); rasterisation and compositing in a `leyline-engine` module (e.g. `mask.rs`) consumed by `process6.rs`. No `leyline-mask`: masking wraps nothing external (unlike `leyline-lens`/Lensfun), it is tightly coupled to the internals of the render buffer. |
| Catalog | **Settled, [ADR 0029](adr/0029-process-6-local-adjustments.md): no dedicated table.** **Parametric** masks (radial/gradient): a few floats, compact inside `settings_json` — consistent with "a revision = a complete, self-contained state" (`docs/catalog.md` §17). **Brush** masks: a list of **vector strokes** (x/y/radius/flow/hardness), never a raster. A dedicated table remains a problem for a future ADR *with real data* should the volume ever demand it. |
| Engine API | **Settled, [ADR 0029](adr/0029-process-6-local-adjustments.md):** no new `EditSession` method — the life cycle of masks goes through `set`/`commit` plus two variants, `Param::LocalAdjustment(usize)` and `Value::LocalAdjustment(Option<LocalAdjustment>)`. **No** local `SettingsGroup` in V2 (§10.3): mask geometry is specific to the composition, not transferable in a preset (the same cut as `Geometry`). |
| Complexity | **XL** — this is infrastructure, not an isolated feature. The architecture is now settled ([ADR 0029](adr/0029-process-6-local-adjustments.md)); the remaining complexity is implementation (brush rasterisation, UI), plus one open architectural question. |

Open questions:

1. ~~The masks' coordinate frame~~ — resolved, [ADR 0026](adr/0026-mask-spot-coordinate-referential.md): the same frame as `crop` (after rotation, before cropping).
2. ~~**Brush storage**: vector strokes in `settings_json` vs. a dedicated table~~ — resolved, [ADR 0029](adr/0029-process-6-local-adjustments.md): vector strokes (x/y/radius/flow/hardness) in `settings_json`, never a raster; a dedicated table remains a problem for a future ADR *with real data* should the volume ever demand it.
3. ~~**Interaction with undo/redo and coalescing** (catalog §17): is a brush stroke an intention, or a continuous gesture to coalesce?~~ — resolved, [ADR 0029](adr/0029-process-6-local-adjustments.md): a complete stroke (press→release) is **one** commit point (§17, "end of drag"); no new mechanism — `Param::LocalAdjustment(usize)`/`Value::LocalAdjustment` reuse the existing coalescing and amendment window.

---

# 3. Tone curve

No parametric or point curve. Only the coarse sliders exist (exposure, contrast, highlights/shadows, whites/blacks).

| Aspect | Analysis |
|---|---|
| Contract | **Process +1**. **Schema: additive** — an optional `tone_curve`, absent = the identity curve (neutral); no increment required. |
| Pipeline (§3.1) | **Settled, [ADR 0030](adr/0030-tone-curve.md)**: a curve stage inserted in the tonal block, after Whites/Blacks and before Vibrance/Saturation. A new process version (the next number available at release time, [ADR 0028](adr/0028-process-version-per-feature.md)). The §3.1 diagram will be amended by the implementation PR, not by the ADR. |
| Crate | **Settled, [ADR 0030](adr/0030-tone-curve.md): no new crate.** `leyline-engine`: building a 1D LUT from the control points (the [ADR 0013](adr/0013-process-2-lut-transfer.md) convention). `leyline-core` carries the fields. |
| Catalog | None. Everything fits in `settings_json`. |
| Engine API | New `Param`s (curve points, parametric mode). Joins a widened `SettingsGroup::Tone` or a dedicated group. |
| Complexity | **S/M**. |

Anticipated fields (`settings_json`, schema unchanged):

| Field | Role | Neutral value |
|---|---|---|
| `tone_curve.points` | A point curve, a list of `{x, y}` normalised to `[0,1]` | absent — identity |
| ~~`tone_curve.parametric`~~ | ~~Highlights/lights/darks/shadows regions~~ — **cut, [ADR 0030](adr/0030-tone-curve.md)**: a parametric curve is only a UI generating points; Studio computes the point list client-side if needed, and the engine has one curve path only. | — |
| ~~`tone_curve.channel`~~ | ~~Target `rgb`/`r`/`g`/`b`~~ — **luminance only in V2, [ADR 0030](adr/0030-tone-curve.md)**: per-channel curves cut out as a separate, heavier feature. | luminance |

Open questions:

1. ~~**Frozen interpolation**: the spline (monotone cubic recommended) is part of the render contract~~ — resolved, [ADR 0030](adr/0030-tone-curve.md): a **monotone cubic spline** (Fritsch–Carlson or equivalent), a model choice frozen with the process version to avoid the overshoot of a naive cubic; the exact numerical constants left to the PR. Precomputed into a LUT (the [ADR 0013](adr/0013-process-2-lut-transfer.md) convention), no per-pixel evaluation.
2. ~~**Per-channel RGB curves** from V2 on, or luminance only first~~ — resolved, [ADR 0030](adr/0030-tone-curve.md): **luminance only** in V2; per-channel curves are a separate, heavier feature, to be designed later in an ADR of their own if wanted.

---

# 4. Colour grading / HSL mixer

Vibrance and saturation exist (globally). What is missing is the **per-hue HSL** mixer (8 hue/saturation/luminance bands) and the shadows/midtones/highlights **colour grading wheels** (colour + luminance), as found in every established developer.

| Aspect | Analysis |
|---|---|
| Contract | **Process +1**. **Schema: additive** — optional `hsl` and `color_grading`, absent = neutral. |
| Pipeline (§3.1) | **Settled, [ADR 0031](adr/0031-hsl-color-grading.md)**: the colour block, after Vibrance/Saturation. A new process version (the next number available at release time, [ADR 0028](adr/0028-process-version-per-feature.md)); HSL and colour grading designed together but deliverable separately. The §3.1 diagram will be amended by the implementation PR. |
| Crate | **Settled, [ADR 0031](adr/0031-hsl-color-grading.md): no new crate.** `leyline-engine`. |
| Catalog | None. Fits in `settings_json`. |
| Engine API | New `Param`s; a colour `SettingsGroup` for presets. |
| Complexity | **M**. |

Anticipated fields:

| Field | Role | Neutral value |
|---|---|---|
| `hsl` | 8 hue bands × `{hue, saturation, luminance}` | absent — 0 everywhere |
| `color_grading.shadows` / `.midtones` / `.highlights` | `{hue, saturation, luminance}` per zone | absent — neutral |
| `color_grading.blending` / `.balance` | Overlap of the zones, shadows↔highlights tilt | 0 |

Open questions:

1. ~~**A frozen hue model**: the exact definition of hue and the computation space~~ — resolved, [ADR 0031](adr/0031-hsl-color-grading.md): **HSL derived from the working RGB** (no perceptual/CIE space), 8 bands at fixed centres with falloff between adjacent bands; colour grading zones separated by **luminance weighting** (smoothstep + `balance`/`blending`), orthogonal to the spatial mask of [ADR 0029](adr/0029-process-6-local-adjustments.md). The exact numerical constants left to the PR.
2. ~~**Regional colour grading**: applying the wheels under a mask (item 2)~~ — resolved (out of V2 scope), [ADR 0031](adr/0031-hsl-color-grading.md): regional colour grading is deferred to a future ADR resting on the masking infrastructure of [ADR 0029](adr/0029-process-6-local-adjustments.md); the global version (by tonal zone) is self-contained and delivered first.

---

# 5. Spot removal / healing

No cloning or healing tool for sensor dust or blemishes.

Intrinsically local: it shares item 2's **coordinate frame** problem (geometry drawn on the displayed image).

| Aspect | Analysis |
|---|---|
| Contract | **Process +1**. **Schema: additive** — an optional `spot_removal` (a list), absent = empty. |
| Pipeline (§3.1) | **Settled, [ADR 0032](adr/0032-spot-removal-clone.md)**: a **Spot removal** stage inserted immediately after Lens correction and before White balance — earlier than the masked stage of [ADR 0029](adr/0029-process-6-local-adjustments.md), so as to operate on data close to linear. A new process version (the next number available at release time, [ADR 0028](adr/0028-process-version-per-feature.md)). `crop`'s frame ([ADR 0026](adr/0026-mask-spot-coordinate-referential.md)). The §3.1 diagram will be amended by the implementation PR. |
| Crate | **Settled, [ADR 0032](adr/0032-spot-removal-clone.md): no new crate.** `leyline-engine` — cloning only, a deterministic bilinear copy reusing the process module's existing `bilinear` function. *Seamless heal* is **cut from V2**. |
| Catalog | **Settled, [ADR 0032](adr/0032-spot-removal-clone.md): no dedicated table.** A compact `spot_removal` list in `settings_json`, consistent with "a revision = a complete, self-contained state" (`docs/catalog.md` §17). A per-revision table remains a problem for a future ADR *with real data* should the volume ever demand it. |
| Engine API | **Settled, [ADR 0032](adr/0032-spot-removal-clone.md):** no new `EditSession` method — the life cycle of spots goes through `set`/`commit` plus the `Param`/`Value` extension (an index into the `spot_removal` array), the same scheme as [ADR 0029](adr/0029-process-6-local-adjustments.md). |
| Complexity | **M** — cloning only ([ADR 0032](adr/0032-spot-removal-clone.md)); *heal* (**L**) is cut from V2 and no longer in the range. |

Anticipated fields: `spot_removal: [ { target:{x,y}, source:{x,y}, radius, feather, opacity } ]` in normalised coordinates — **the `mode` field abandoned, [ADR 0032](adr/0032-spot-removal-clone.md)**: cloning being the only V2 mode, no mode field is necessary.

Open questions:

1. ~~**Determinism of healing**: *heal* (seamless, Poisson-style cloning) must produce the same pixels on every render~~ — resolved, [ADR 0032](adr/0032-spot-removal-clone.md): *heal* is **cut from V2** (a mode, not a flag) — a Poisson blend cannot be validated without reference images, in the spirit of [ADR 0016](adr/0016-process-3-lens-correction.md); only cloning (a deterministic bilinear copy) is delivered, and it is reproducible by construction.
2. ~~**Automatic source selection**: if the engine proposes a source, the proposal must be deterministic and recorded~~ — resolved, [ADR 0032](adr/0032-spot-removal-clone.md): **manual source only** in V2, no engine suggestion; if one is added later, it is written into `spot_removal[].source` like any other value, never recomputed at render time.
3. ~~The coordinate frame~~ — resolved, [ADR 0026](adr/0026-mask-spot-coordinate-referential.md), shared with item 2.

---

# 6. Dehaze / texture / clarity

Only "detail" exists (noise reduction + sharpening). No separate clarity, texture or dehaze controls — three distinct local/frequency contrast treatments.

| Aspect | Analysis |
|---|---|
| Contract | **Process +1**. **Schema: additive** — three optional sliders, neutral at 0. |
| Pipeline (§3.1) | **Settled, [ADR 0033](adr/0033-clarity-texture-dehaze.md)**: the order **clarity → texture → dehaze**, all of them **before Vibrance/Saturation** (hence before the masked stage of [ADR 0029](adr/0029-process-6-local-adjustments.md), whose position stays intact). A new process version (the next number available at release time, [ADR 0028](adr/0028-process-version-per-feature.md)). The §3.1 diagram will be amended by the implementation PR. |
| Crate | **Settled, [ADR 0033](adr/0033-clarity-texture-dehaze.md): no new crate.** `leyline-engine`. Clarity/texture: **one single** local-contrast function through a blurred mask, called at two radii (the blur approximated by subsampling, no large full-resolution kernel). Dehaze: *dark channel prior*, atmospheric light by percentile in **closed form**, determinism fixed. |
| Catalog | None. |
| Engine API | New `Param`s; they join a widened presence/detail group for presets. |
| Complexity | **M** (clarity/texture) to **L** (dehaze). |

Anticipated fields: `clarity`, `texture`, `dehaze`, sliders in `[-100, +100]`, neutral 0 ([ADR 0033](adr/0033-clarity-texture-dehaze.md)).

Open questions:

1. ~~**Determinism of dehaze**: the atmospheric estimate freezes with the process version~~ — resolved, [ADR 0033](adr/0033-clarity-texture-dehaze.md): atmospheric light by a **fixed percentile of the dark channel**, selected in **closed form** (no iterative optimisation), frozen with the process version.
2. ~~**Global first, masked later**~~ — resolved, [ADR 0033](adr/0033-clarity-texture-dehaze.md): the three sliders are delivered **globally** in V2; their masked/regional version is deferred to a future ADR resting on [ADR 0029](adr/0029-process-6-local-adjustments.md) (the same cut as [ADR 0031](adr/0031-hsl-color-grading.md) for regional colour grading).
3. ~~**CPU cost** of multi-scale local contrast on large previews~~ — resolved, [ADR 0033](adr/0033-clarity-texture-dehaze.md): the large-radius blur is **approximated by subsampling** (pyramid/box-filter style), never a full-resolution Gaussian kernel; the exact factor left to the PR.

---

# 7. Soft proofing, watermark, print module

None of the three is implemented. **Important: this is the least "pipeline" item** — the essential part is not a pixel modification of the stored revision, but preview / export / colour management / UI work.

| Sub-item | Actual nature | Render contract |
|---|---|---|
| Soft proofing | A simulation **at display time** through a destination ICC profile, with a gamut warning. Modifies **neither** `settings_json` **nor** the revision's pixels — it is a view mode. | **Settled, [ADR 0034](adr/0034-softproofing-watermark-print.md)**: **no** process/schema, an unpersisted transform. An **optional, view-only** proofing parameter on a preview call (`docs/engine-api.md` §11) — destination profile + intent + gamut warning — never written to the catalog. Reuses the ICC primitive of [ADR 0027](adr/0027-color-management-beyond-srgb.md) in `leyline-color`. |
| Watermark | An overlay **at export time**, an output-stage decoration on the same footing as format/quality. | **Settled, [ADR 0034](adr/0034-softproofing-watermark-print.md)**: **no** develop process. **Text only in V2** (image/logo cut); an additive `watermark` field in `ExportSettings`/`ExportRecipe` ([ADR 0025](adr/0025-unified-export-request.md), `docs/engine-api.md` §12), hence in `export_presets.settings_json` (`docs/catalog.md` §27). Composited last of all, **after** the destination ICC transform of [ADR 0027](adr/0027-color-management-beyond-srgb.md), before encoding. |
| Print module | A layout + output subsystem (margins, printer profile, proofing). Mostly Studio plus a print rendering path. | **Settled, [ADR 0036](adr/0036-print-module.md)**: **no** develop process/schema — "an export with a physical dimension (`paper × DPI` instead of `max_edge`) and a destination profile", reusing `leyline-export`'s plumbing and the ICC primitive of [ADR 0027](adr/0027-color-management-beyond-srgb.md). **One photo per page** (contact sheets cut from V2). A `print_presets` preset parallel to `export_presets` (`docs/catalog.md` §27); `PrintRequest`/`PrintRecipe` on the shape of [ADR 0025](adr/0025-unified-export-request.md). The **rendering** is the engine's (an extension of `leyline-export`/`leyline-color`, no crate); the **hand-off to the printer** (the OS dialog) lives in `leyline-studio` (`docs/engine-api.md` §14, the pattern of [ADR 0020](adr/0020-menu-bar.md)/[0021](adr/0021-context-menus.md)). The only **open risk left to the PR**: the exact OS hand-off mechanism (portable PDF vs. raster + platform API vs. a Slint surface). |

| Aspect | Analysis |
|---|---|
| Crate | `leyline-color` (proofing, profiles), `leyline-export` (watermark), `leyline-studio` + a dedicated output path (printing). No obvious new crate. |
| Catalog | Watermark: fields in `export_presets.settings_json`. Proofing/printing: nothing persistent on the develop side. |
| Engine API | `ExportSettings` gains the watermark. Preview gains a proof-profile option. |
| Complexity | Watermark **S/M**; proofing **M**; printing **L/XL**. |

~~The major open question: proofing, printing and non-sRGB export all run into V1's sRGB freeze~~ — resolved, [ADR 0027](adr/0027-color-management-beyond-srgb.md): the pipeline stays sRGB, and the output (export, proofing) gains an ICC transform to a destination profile through a widened `leyline-color`. A cross-cutting thread shared with item 8, see §8.

~~The concrete shape of proofing and of the watermark~~ — resolved, [ADR 0034](adr/0034-softproofing-watermark-print.md): a **text-only** watermark (image/logo cut) in `ExportSettings`, composited after the destination ICC transform; proofing = a **view-only** parameter on a preview call, never persisted; both reuse the ICC primitive of [ADR 0027](adr/0027-color-management-beyond-srgb.md). **The print module is now settled too, [ADR 0036](adr/0036-print-module.md)**: "an export with a physical dimension and a destination profile" (no process, no pipeline stage), **one photo per page** (sheets cut), a `print_presets` preset, engine rendering and the OS hand-off left to Studio — the only remaining point is the **open risk** of the exact OS hand-off mechanism (portable PDF vs. raster + platform API vs. a Slint surface), flagged for the PR. Item 7 is therefore entirely closed.

---

# 8. Camera/lens profile authoring beyond Lensfun

No custom DCP-style camera calibration. V1's lens correction (Lensfun, `docs/adr/0016`–`0018`, processes 3–5) covers geometric distortion/vignetting/TCA, not the sensor's **colour**.

A DCP profile calibrates the sensor's colour rendering (colorimetric matrices, HSL tables, tone curve, *look table*) — hence **very early** in the pipeline, at the sensor RGB → working space conversion.

| Aspect | Analysis |
|---|---|
| Contract | **Process +1** (the next number available at release time, [ADR 0028](adr/0028-process-version-per-feature.md)), and **the insertion of a new stage** for the input colour profile in §3.1 — reordering = a process event. **Schema: additive** (the profile reference). Settled, [ADR 0035](adr/0035-camera-profile-dcp.md). |
| Pipeline (§3.1) | **Settled, [ADR 0035](adr/0035-camera-profile-dcp.md)**: a new **camera profile** stage **first of all**, between the decoded RAW and **Lens correction** (a precision about the order: before geometric correction, since DCP calibrates colour and the lens calibrates geometry — no interaction, and we operate on the least processed data, in the spirit of the early placement of [ADR 0032](adr/0032-spot-removal-clone.md)). The §3.1 diagram will be amended by the implementation PR. |
| Crate | **Settled, [ADR 0035](adr/0035-camera-profile-dcp.md): no new crate — an extension of `leyline-color`.** ADR 0027 has already made it a general colour-transform library; applying a DCP (matrix + LUT applied directly, not through `cmsTransform`) is parallel colour-domain work, with no coupling to the engine's buffers (in contrast with the masking of [ADR 0029](adr/0029-process-6-local-adjustments.md), housed in `leyline-engine` *because* it is coupled to the buffer). The **DCP parser** (a minimal in-house one, or an existing crate) remains **an open dependency risk**, left to the PR. |
| Catalog | **Settled, [ADR 0035](adr/0035-camera-profile-dcp.md):** files **supplied by the user only** (no embedded database in V2, in contrast with Lensfun), dropped into a relative folder (`Profiles/Camera/`, `docs/catalog.md` §2.3), **referenced by an explicit relative path** (no EXIF auto-matching), with a **BLAKE3 checksum** ([ADR 0006](adr/0006-blake3.md)) of the `.dcp` file in `settings_json`. No dedicated table. |
| Engine API | Profile selection as a `Param`; possibly a surface to enumerate the available profiles. |
| Complexity | **L/XL**. |

Open questions:

1. ~~**The DCP dependency**~~ — resolved, [ADR 0037](adr/0037-dcp-parsing-dependency.md): a minimal in-house tag reader on top of the already linked `tiff` crate (read-only, in `leyline-color`), no new crate to vet. **Colorimetric correctness**, however, remains the unchanged open risk posed by ADR 0035/0016: validation against real Adobe DCP files and their reference renders is required before release.
2. ~~The interaction with the sRGB freeze~~ — resolved, [ADR 0027](adr/0027-color-management-beyond-srgb.md) then [ADR 0035](adr/0035-camera-profile-dcp.md): `leyline-color` is already a general colour-transform library, and [ADR 0035](adr/0035-camera-profile-dcp.md) houses in it the input DCP stage (sensor → working space) that ADR 0027 did not cover, **first of all** in the pipeline.
3. ~~**Reproducibility** of an entirely new colour path, frozen by a process version~~ — resolved, [ADR 0035](adr/0035-camera-profile-dcp.md): a new process version (the next available, [ADR 0028](adr/0028-process-version-per-feature.md)); the referenced external `.dcp` file is **BLAKE3-checksummed** ([ADR 0006](adr/0006-blake3.md)), a genuinely new problem (the other ADRs store their geometry inline). A checksum that does not match triggers the existing §3.4 failure mode ("modify nothing, warn"), not a new category — an extension of the `docs/pipeline.md` §5 contract to an externally referenced input.

---

# 9. Cross-cutting threads and ADR eligibility

Three threads run across the eight items. Two were locks to settle **before** the features that depend on them — both are now resolved:

* **The post-crop coordinate contract — resolved, [ADR 0026](adr/0026-mask-spot-coordinate-referential.md).** Items 2, 4 and regional colour grading (3) draw geometry on the displayed image, whereas §3.1 applies Rotation/Crop at the end of the chain. Geometry is now stored in the same frame as `crop` (normalised, after rotation, before cropping); the engine carries it back to the pre-rotation buffer through the same family of backward remapping as `rotate`/lens correction.
* **A generic masking primitive — architecture settled, [ADR 0029](adr/0029-process-6-local-adjustments.md).** Item 2 is the infrastructure on which item 5 (spatial), masked dehaze (6) and regional colour grading (3) rest. Building masking first avoids coding the same spatial coverage three times. ADR 0029 fixes the foundation: process 6, a masked stage reusing the global operators, masks (brush/radial/gradient) stored in `settings_json`, driven by `Param::LocalAdjustment`. Colour grading (3), the curve (4) and clarity/texture/dehaze (6) remain deliverable **globally** without waiting for masks; their regional version does rest on that foundation.
* **Colour management beyond sRGB — resolved, [ADR 0027](adr/0027-color-management-beyond-srgb.md).** Items 7 (proofing, printing, non-sRGB export) and 8 (DCP) all ran into ADR 0015's sRGB freeze. The render pipeline stays sRGB (no process version reopened); `leyline-color` widens into a general ICC transform library to carry proofing and export towards a destination profile. Item 8 (DCP) stays distinct: it touches the **beginning** of the pipeline (sensor → working space), not the output — its own ADR remains to be written.

A fourth point, not blocking but structuring: **the proliferation of process versions — resolved, [ADR 0028](adr/0028-process-version-per-feature.md).** The house style duplicates a whole `processN.rs` module per version (ADR 0013, 0016) to guarantee the "same pixels ten years from now" freeze. Stacking several pixel features in V2 means either several bumps (several duplicated modules) or one grouped bump. The choice is settled: **one process per feature**, each pixel feature keeping its own frozen module and being able to ship independently. The grouped bump is rejected (it saves no duplication, since any later fix to one grouped feature forces a whole extra module anyway), as is a library of shared operators (it would reopen the risk of silently altering a frozen render that duplication exists to remove).

ADR eligibility once the decision is taken (none is decided today):

| Item | Its own ADR? |
|---|---|
| 2 — Local / masked adjustments | **[ADR 0029](adr/0029-process-6-local-adjustments.md) — the foundation is settled** (process 6, masked stage, coordinate frame, storage, API, presets). Further ADRs may follow for refinements per tool type, but the infrastructure no longer waits for them. |
| 3 — Tone curve | **[ADR 0030](adr/0030-tone-curve.md) — settled**: a point curve only (parametric mode cut), a frozen monotone cubic spline, luminance only, precomputed into a LUT. |
| 4 — Colour grading / HSL | **[ADR 0031](adr/0031-hsl-color-grading.md) — settled**: HSL derived from RGB (8 bands + falloff), colour grading zones weighted by luminance; regional colour grading deferred. |
| 5 — Spot removal | **[ADR 0032](adr/0032-spot-removal-clone.md) — settled**: cloning only (heal cut from V2), a deterministic bilinear copy, a stage early in the pipeline (after Lens correction), a manual source, the `Param`/`Value` of [ADR 0029](adr/0029-process-6-local-adjustments.md). |
| 6 — Dehaze / texture / clarity | **[ADR 0033](adr/0033-clarity-texture-dehaze.md) — settled** (one ADR grouping all three): clarity/texture as a unified local contrast at two radii, dehaze by dark channel prior in closed form; global in V2, masked deferred. |
| 7 — Proofing / watermark / printing | **The item is entirely settled.** Proofing + watermark, **[ADR 0034](adr/0034-softproofing-watermark-print.md)** — and, as anticipated, **not** a develop ADR (no process, no pipeline stage): a text watermark (export, `ExportSettings`), view-only proofing (preview), both on the colour primitive of [ADR 0027](adr/0027-color-management-beyond-srgb.md) that completed ADR 0015. **The print module is settled, [ADR 0036](adr/0036-print-module.md)**: "an export with a physical dimension and a destination profile" (no process, no stage), one photo per page (sheets cut), a `print_presets` preset, engine rendering, OS hand-off left to Studio (the pattern of [ADR 0020](adr/0020-menu-bar.md)/[0021](adr/0021-context-menus.md)). Only the **exact OS hand-off mechanism** remains a deliberately open risk, left to the PR. |
| 8 — Camera profiles (DCP) | **[ADR 0035](adr/0035-camera-profile-dcp.md) — settled**: a new process (the next available, [ADR 0028](adr/0028-process-version-per-feature.md)), a new colorimetric stage at the head of the pipeline (before lens correction), an extension of `leyline-color` (no new crate), user files referenced by relative path and BLAKE3-checksummed. The **DCP parsing dependency is now resolved, [ADR 0037](adr/0037-dcp-parsing-dependency.md)**: a minimal in-house tag reader on top of the already linked `tiff` crate (read-only, in `leyline-color`), no new crate to vet; **colorimetric correctness**, however, remains ADR 0035's unchanged open risk (validation against real Adobe DCP files, the bar set by [ADR 0016](adr/0016-process-3-lens-correction.md)). |
