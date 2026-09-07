# Presets Specification

**Document:** `docs/presets.md`
**Version:** 1.0
**Status:** Draft

---

# 1. Purpose

A develop preset is a named, reusable set of settings, applicable in one gesture to a photo, to a selection, or to a whole library.

It is a baseline expectation of any RAW development tool: this document describes what Leyline offers, ahead of any implementation question.

Out of scope: **export presets** (`catalog.md` §27, `engine-api.md` §12) already exist and concern output encoding (format, quality, dimensions), not development. This document covers **develop presets** exclusively.

---

# 2. Principles

* A preset is **partial by nature**: it touches the setting categories its author chose to include, never the others.
* Applying a preset destroys nothing: as the pipeline requires (`pipeline.md` §2), each application produces a **new revision**, never an overwrite of the current settings.
* A preset applies just as well to a single photo as to an arbitrary batch (a selection in the grid, a search result, an entire collection, the whole library): batch processing is a nominal case (`engine-api.md` §8), not an option for presets either.
* A preset is independent of the photos it has already been applied to: editing it never affects past revisions (the same immutability contract as `pipeline.md` §2).
* A preset is portable: its format is self-contained JSON, with no reference to a particular library — a necessary condition for the export/import or the voluntary sharing mentioned in `readme.md`.

---

# 3. Model

## 3.1 Setting categories

A preset never captures the complete state of a development (`settings_json`, `pipeline.md` §3.2). A preset that captured everything would also overwrite crop and rotation on application — a "high-contrast black & white" preset applied to a whole series must not crop every photo the same way.

The settings of schema 1 (`pipeline.md` §3.2) fall into **categories**, at checkbox granularity — never at the individual field:

The rule that generates the list: **one category per block of the develop panel** ([ADR 0132](adr/0132-selective-copy-and-full-coverage.md) §1), because the panel is where a photographer already decides what a development is made of.

| Category (CLI name) | `groups` value | Fields covered |
|---|---|---|
| `white_balance` | `WhiteBalance` | `white_balance` (temperature, tint) |
| `tone` | `Tone` | `exposure`, `contrast`, `highlights`, `shadows`, `whites`, `blacks` |
| `presence` | `Presence` | `clarity`, `texture`, `dehaze`, `vibrance`, `saturation`, `monochrome` |
| `tone_curve` | `ToneCurve` | `tone_curve` ([ADR 0098](adr/0098-per-channel-tone-curves.md)) |
| `color_mixer` | `ColorMixer` | `hsl`, all eight bands together |
| `color_grading` | `ColorGrading` | `color_grading` |
| `camera_profile` | `CameraProfile` | `camera_profile` ([ADR 0035](adr/0035-camera-profile-dcp.md)) |
| `creative_lut` | `CreativeLut` | `lut` ([ADR 0053](adr/0053-creative-lut.md)) |
| `effects` | `Effects` | `vignette`, `grain` (ADR 0090 §5) |
| `lens_correction` | `LensCorrection` | `lens_correction`, `defringe` — the category is *what the lens did to this photograph*, not the `LensCorrection` struct ([ADR 0113](adr/0113-defringe.md) §5) |
| `detail` | `Detail` | `noise_reduction`, `sharpening` |
| `rendering` | `Rendering` | `highlight_reconstruction`, `demosaic`, `output_rendering` — how the photograph comes out of the file |
| `geometry` | `Geometry` | `rotation`, `crop`, `perspective` |
| `reshape` | `Reshape` | `reshape` ([ADR 0109](adr/0109-reshape-stage.md)) |
| `spot_removal` | `SpotRemoval` | `spot_removal` |
| `red_eye` | `RedEye` | `red_eye` ([ADR 0103](adr/0103-red-eye-correction.md)) |
| `local_adjustments` | `LocalAdjustments` | `local_adjustments` ([ADR 0029](adr/0029-process-6-local-adjustments.md)) |

Four settings are deliberately in **no** category, and each for a stated reason (ADR 0132 §3): `source_encoding` is a fact about the file rather than a decision about the photograph; `schema` and `stages` are the revision's own versioning, and a preset never fixes a rendering version ([ADR 0043](adr/0043-collapse-prerelease-render-history.md) §3); `extra` is the forward-compatibility passthrough, which belongs to the document that carried it.

A category is **atomic**: including it captures (or applies) all of its fields together. You cannot include `temperature` without `tint`. That granularity matches what the user actually chooses ("I want this colour rendering and this contrast, but not this crop"), with no superfluous complexity at the field level.

Six categories are **never included by default** — `geometry`, `rendering`, `reshape`, `spot_removal`, `red_eye`, `local_adjustments` — under one rule: *what a look is made of is included, what a place in this photograph is made of is not* (ADR 0132 §6). A crop, a mask, a spot repair, a pair of eyes and a reshape are statements about one frame; `rendering` is excluded from the other side, being about this *file*. The user can include any of them explicitly — a "centred square" preset for an Instagram series, or a graduated filter over a horizon that suits a whole shoot.

## 3.2 Format (`preset_json`)

```json
{
    "schema": 1,
    "groups": ["WhiteBalance", "Tone", "Presence"],

    "white_balance": { "temperature": 5400, "tint": 4 },
    "exposure": 0.35,
    "contrast": 12,
    "highlights": -40,
    "shadows": 25,
    "whites": 0,
    "blacks": -5,
    "vibrance": 18,
    "saturation": 0
}
```

* `schema` references the **same** numbering as `settings_json.schema` (`pipeline.md` §3.2): a preset uses the field vocabulary of a given develop schema, it is not a separate versioning space. A schema 1 preset is understood with the field definitions of schema 1.
* `groups` is the **source of truth** for what the preset touches. That is necessary because a preset is entitled to want to return a category to its neutral value (e.g. "turn noise reduction off") — a `noise_reduction` absent from the JSON does *not* mean "leave it alone", contrary to the omitted-values rule of `settings_json` (`pipeline.md` §3.2). It is a deliberate divergence: `preset_json` answers "what to apply", `settings_json` answers "what the state is".
* Only the fields of the categories listed in `groups` appear in the JSON.
* No `process` field: a preset never fixes a process version. A revision created by applying a preset inherits the process of its parent revision, like any edited revision (`pipeline.md` §3.3).

## 3.3 Schema evolution

A preset follows the compatibility rules of `pipeline.md` §3.4, on the same schema as `settings_json`:

* an engine that knows the declared `schema` can read and apply the preset;
* an engine that meets a `schema` newer than what it knows refuses the application (the same error family as `NewerSettings`, `engine-api.md` §4) rather than applying a badly interpreted subset;
* renaming or removing a field in a new schema version leaves presets of the old schema still readable (no automatic migration), exactly as with historical revisions.

A preset never references a stage version: it stays valid across render evolutions, and only the `schema` of its fields concerns it. Stage versions come from the revision it is applied to (`pipeline.md` §3.3).

---

# 4. Catalog

```sql
CREATE TABLE develop_presets (

    id INTEGER PRIMARY KEY,

    uuid TEXT NOT NULL UNIQUE,

    name TEXT NOT NULL,

    preset_json TEXT NOT NULL,

    created_at INTEGER NOT NULL

);
```

The same shape as `export_presets` (`catalog.md` §27) but a distinct table: these are two independent domains (develop settings vs. output encoding), which have no reason to share a foreign key or common deletion rules (see ADR 0014).

Presets are independent of the revisions they produced: deleting a preset touches no existing revision (no `FOREIGN KEY` from `develop_revisions`). The history stays readable even if the preset that generated it has since been renamed or deleted — consistent with `export_history`, which keeps `preset_id` nullable with `ON DELETE SET NULL` (`catalog.md` §28): here we go further, `develop_revisions` does not even reference the originating preset, because a revision is a state of settings, not the trace of an action (`catalog.md` §17: "a revision represents a user intention, never an interface event").

---

# 5. Application

## 5.1 To a version

Applying a preset to a version merges the fields of the included categories over the head's current settings, then **commits** — exactly the gesture of a user who would adjust the sliders of the categories concerned and then release them (`pipeline.md`, "commit points").

Direct consequences of reusing the existing revision model, with no new invariant:

* a new revision is always created (never an amendment — "an explicit action requires it" already covers snapshots and export, and applying a preset falls in with them);
* the result is immediately undoable like any revision (`pipeline.md` §2, catalog §16);
* the process of the created revision is inherited from its parent (§3.2 above);
* the categories not included (typically `geometry`) stay strictly unchanged.

## 5.2 To a batch

A preset applies to a set of versions designated like any other batch processing in Leyline: a selection in the grid, a search result, a collection, or the entire library (the same selection as `GridQuery`, catalog §30, already used for smart collections).

* Every version in the batch receives its own revision, independent of the others — a consequence of "the version is the library unit" (ADR 0008): there is no single transaction covering the whole batch.
* A failure on one version (e.g. `NewerSettings` if the preset references a schema the engine no longer knows) does not prevent the others from being processed; the result is reported per version, like `export_batch` and import (`engine-api.md` §3.2, §12).
* The batch is long work (potentially a whole library): it follows the "jobs" category of `engine-api.md` §3.1, not "queries".

## 5.3 Non-destructiveness

No new rule: applying a preset, one by one or in batch, is writing revisions through the already specified mechanism (`pipeline.md` §2, §6). Nothing is lost, everything stays reproducible, and the complete history of each photo (including the pre-preset revisions) stays browsable and restorable.

---

# 6. A preset's life cycle

* **Create**: from the current state of an edit session (`engine-api.md` §10.1), choosing the categories to capture. A preset can also be created "blank" (the schema's neutral values) and then edited.
* **Rename**, **duplicate**, **delete**: simple operations on `develop_presets`, with no retroactive effect (§4).
* **Organise**: a flat list is enough for the V1 scope (no preset folders) — an extension held in reserve should the need be confirmed in use, following the hierarchical keyword model (catalog §22) if necessary.

---

# 7. Portability

`preset_json` references nothing specific to a library (no internal ID, no path): a preset can be serialised on its own (name + `preset_json`) into a file and reloaded into another library or shared, without translation. This is not a V1 feature (no preset-file export/import UI is planned here), but it is a free property of the chosen format, aligned with the intention already recorded in `readme.md` ("possibly sharing presets voluntarily").

---

# 8. What follows

This document fixes the product contract and the data model. Integrated into `engine-api.md` (the `Library` surface, query vs. job category, events) and into the engine (`leyline-engine`, `leyline-catalog`) — see `adr/0014-develop-presets.md`.
