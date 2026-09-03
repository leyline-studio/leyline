# Pipeline Development Specification

**Document:** `docs/pipeline.md`
**Version:** 1.0
**Status:** Draft

---

# 1. Purpose

The develop pipeline guarantees that every processing Leyline performs is **reproducible**, **versioned** and **non-destructive**.

Algorithms will evolve over time: fixes, optimisations, new parameters, new models.

That evolution must **never** invalidate results already computed.

A develop revision created today must produce exactly the same pixels ten years from now.

---

# 2. Principles

* every pipeline has a stable identity;
* every incompatible evolution creates a new version;
* every run keeps a complete copy of the parameters it used;
* a run's parameters are immutable;
* historical results are never modified;
* a recomputation always creates a new run.

This document covers two levels:

1. the **RAW develop pipeline** — the heart of Leyline (V1);
2. the **generic processing pipelines** — the framework that generalises it (thumbnails, histograms today; faces, OCR, AI tomorrow, see §38 of the catalog).

---

# 3. The RAW develop pipeline

## 3.1 Order of operations

Development applies a chain of operations **in a fixed order**, defined by the engine — never by the user.

```text
Decoded RAW

↓

Input (`input`: decoder configuration, buffer space)

↓

Camera profile (DCP)

↓

Lens correction

↓

Defringe

↓

Reshape

↓

Spot removal

↓

White balance

↓

Exposure

↓

Contrast

↓

Highlights / Shadows

↓

Whites / Blacks

↓

Tone curve

↓

Clarity / Texture / Dehaze

↓

Vibrance / Saturation

↓

HSL mixer

↓

Colour grading (shadows/midtones/highlights)

↓

Local adjustments (masked)

↓

Noise reduction

↓

Sharpening

↓

Rotation / Perspective / Crop

↓

Output rendering (`output_rendering`: from the working buffer to a display signal)

↓

Output (preview or export)
```

The user sets **values**, never the order.

**The working space.** Between stages, the buffer is in **Rec. 2020, linear light, D65, bounded below at 0 and unbounded above** ([ADR 0044](adr/0044-linear-wide-gamut-working-space.md)). Three consequences, one per default that ADR corrects:

* the sensor's gamut is no longer clipped before the first setting — the narrowing towards the output space happens once, right at the end;
* highlights above white travel through the pipeline: +1 EV then −1 EV gives the starting image back, and the *highlights* slider has material to recover;
* the operators that describe light (white balance, exposure, vignetting) and every geometric resampling are multiplications and weighted sums of real light.

The **tonal** operators, for their part, explicitly declare the display axis (`in_display`): a contrast slider is a statement about *perceived* lightness, and the same curve applied to linear light would crush the shadows. This is not a step backwards — nothing there is clipped at 1, the axis is simply the one on which those curves mean something.

The two framing stages, `input` and `output_rendering`, bracket that buffer. They have no neutral value — there is no render without an input and an output — and are therefore the only ones that **every** revision writes into its `stages` map. `input` carries the configuration asked of the decoder (native linear sensor, 16 bits, since ADR 0050 the fate of channels saturated at the sensor, since ADR 0061 the interpolation, and since [ADR 0066](adr/0066-sensor-white-level.md) the level the sensor calls white) and the matrix that brings its pixels into the working space; `output_rendering` brings the unbounded buffer back to a display signal, a highlight shoulder then a conversion to the output space.

The working space is a property declared by each stage version: two versions of different spaces do not compose, and a plan that mixes them **fails** (`MixedWorkingSpaces`) instead of being rendered as best it can. Migrating a revision from one space to the other is a reprocessing (§4.5), hence a new revision.

That order is part of the render contract: changing it changes the pixels produced, and therefore requires a new *stage version* declaring a different rank (§3.3).

**Non-RAW sources.** The catalog accepts JPEG, TIFF and PNG files at import (catalog §10). Those files enter the same chain: they are decoded by native codecs (EXIF orientation applied, samples read in the colour space the file **declares** — an embedded ICC profile, or HEIF's `nclx` box, [ADR 0115](adr/0115-tagged-source-colour.md) — and as sRGB when it declares nothing — at 8 bits through `input: 4`, at the file's own depth from `input: 5`, [ADR 0107](adr/0107-derived-assets-and-the-pixel-socket.md)) and take the place of "decoded RAW" at the head of the pipeline; `input` then decodes their transfer function and rotates their primaries into the working space. One kind of file skips that last step and says so in the revision: a **derived asset** ([ADR 0107](adr/0107-derived-assets-and-the-pixel-socket.md)) holds the develop buffer as it stood before rank 20 — linear Rec. 2020, white at 1.0 — and carries `source_encoding: "linear_workspace"`, which `input: 5` reads as "nothing to convert". Every earlier version of the stage **refuses** that setting rather than ignoring it. Having no headroom above white, they are imported with the output shoulder at 0, so that an unedited import comes back out **bit for bit** what it was. Decoding stays deterministic exactly as LibRaw does (§5). PSD is catalogued but has no decoder: asking for its pixels is an explicit error, not a LibRaw refusal. **HEIF is decoded** since [ADR 0114](adr/0114-heif-reading.md), by the libheif the platform provides — a build without that backend, which is how the packages published here are made, refuses a `.heic` by name instead.

---

## 3.2 settings_json

Every revision (`develop_revisions.settings_json`, catalog §17) contains a **complete, self-contained state** of the development.

Never a delta.

```json
{
    "schema": 1,
    "stages": {
        "input": 2,
        "camera_profile": 1, "gains": 1, "contrast": 1, "crop": 1,
        "output_rendering": 1
    },

    "lut": {
        "enabled": true,
        "path": "Profiles/LUT/Kodachrome.cube",
        "checksum": "blake3:2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae",
        "strength": 75
    },
    "camera_profile": {
        "enabled": true,
        "path": "Profiles/Camera/Canon EOS 60D.dcp",
        "checksum": "blake3:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
    },
    "white_balance": { "temperature": 5400, "tint": 4 },
    "exposure": 0.35,
    "contrast": 12,
    "highlights": -40,
    "shadows": 25,
    "whites": 0,
    "blacks": -5,
    "clarity": 25,
    "texture": 15,
    "dehaze": 30,
    "vibrance": 18,
    "saturation": 0,

    "tone_curve": {
        "points": [
            { "x": 0.0,  "y": 0.0 },
            { "x": 0.25, "y": 0.30 },
            { "x": 0.75, "y": 0.70 },
            { "x": 1.0,  "y": 1.0 }
        ]
    },
    "spot_removal": [
        {
            "target": { "x": 0.62, "y": 0.31 },
            "source": { "x": 0.55, "y": 0.29 },
            "radius": 0.03,
            "feather": 0.40,
            "opacity": 1.0
        }
    ],
    "hsl": [
        { "hue": 0,  "saturation": -20, "luminance": 0 },
        { "hue": 5,  "saturation": 0,   "luminance": 0 },
        { "hue": 0,  "saturation": 0,   "luminance": 0 },
        { "hue": -10, "saturation": 15, "luminance": 8 },
        { "hue": 0,  "saturation": 0,   "luminance": 0 },
        { "hue": 8,  "saturation": 25,  "luminance": 0 },
        { "hue": 0,  "saturation": 0,   "luminance": 0 },
        { "hue": 0,  "saturation": 0,   "luminance": 0 }
    ],
    "color_grading": {
        "shadows":    { "hue": 220, "saturation": 15, "luminance": 0 },
        "midtones":   { "hue": 0,   "saturation": 0,  "luminance": 0 },
        "highlights": { "hue": 45,  "saturation": 10, "luminance": 0 },
        "balance": 0,
        "blending": 50
    },
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
            "mask": { "type": "everything" },
            "range": {
                "luminance": { "min": 0.25, "max": 0.75, "softness": 0.15 },
                "color": { "center": 210.0, "width": 40.0, "softness": 20.0 }
            },
            "opacity": 0.85,
            "adjustments": { "exposure": -0.4, "texture": -35, "noise_luminance": 20 }
        },
        {
            "mask": {
                "type": "coverage",
                "path": "Masks/9f2c….png",
                "checksum": "blake3:9f2c…"
            },
            "opacity": 1.0,
            "adjustments": { "exposure": 0.3, "clarity": 12 }
        }
    ],

    "lens_correction": { "enabled": true, "profile": "auto", "tca_red": 0.034, "tca_blue": -0.012 },
    "defringe": { "purple": 40, "green": 0 },
    "reshape": [ { "from": { "x": 0.41, "y": 0.38 }, "to": { "x": 0.44, "y": 0.4 }, "radius": 0.12, "strength": 0.8 } ],
    "noise_reduction": { "luminance": 15, "color": 25 },
    "sharpening": { "amount": 40, "radius": 1.0, "masking": 0 },
    "output_rendering": { "highlight_rolloff": 50 },
    "highlight_reconstruction": "rebuild",
    "demosaic": "dcb",

    "rotation": 0.0,
    "perspective": { "vertical": 35, "horizontal": 0 },
    "crop": { "x": 0.1, "y": 0.2, "width": 0.8, "height": 0.7 }
}
```

### Reserved fields

| Field | Role |
|---|---|
| `schema` | Version of the parameters' **format** (the JSON's structure) |
| `stages` | Version of the **render**, stage by stage (the algorithms producing the pixels) |

The two evolve independently: a field can be renamed without changing the render, and an algorithm fixed without changing the structure.

`stages` associates with each **active** stage the version of that stage which renders this revision. A stage at its neutral value does not run, therefore has no behaviour to pin, and **does not appear** in it — the map is proportional to the actual editing, not to the number of stages the engine has. A neutral revision therefore records only the two framing stages (§3.1), which have no neutral value.

### Omitted values

A missing parameter takes its **neutral value**, defined by the schema.

Neutral values are **frozen per schema version**: they never change retroactively.

`{}` with `schema: 1` will always produce the neutral render of schema 1.

Neutral values of schema 1:

| Parameter | Neutral value |
|---|---|
| `camera_profile` | absent — the decoder's own sRGB conversion, no profile applied |
| `white_balance` | absent — the camera's "as shot" white balance |
| `exposure` | 0.0 EV |
| `contrast`, `highlights`, `shadows`, `whites`, `blacks`, `vibrance`, `saturation` | 0 |
| `clarity`, `texture`, `dehaze` | 0 |
| `hsl` | absent — 8 bands at `{ "hue": 0, "saturation": 0, "luminance": 0 }` |
| `monochrome` | absent — `false` |
| `color_grading` | absent — each zone at `{ "hue": 0, "saturation": 0, "luminance": 0 }`, `balance`/`blending` at 0 |
| `lens_correction` | `{ "enabled": false, "profile": "auto", "tca_red": 0.0, "tca_blue": 0.0 }` |
| `defringe` | `{ "purple": 0, "green": 0 }` |
| `reshape` | `[]` |
| `noise_reduction` | `{ "luminance": 0, "color": 0 }` |
| `sharpening` | `{ "amount": 0, "radius": 1.0, "masking": 0 }` |
| `output_rendering` | `{ "highlight_rolloff": 50 }` — the only field whose default value is not "do nothing": there is no render without an output, so it is a rendering choice, frozen with the stage version that reads it (ADR 0044 §3). Importing a JPEG/PNG/TIFF opens it at 0, there being no headroom to recover |
| `highlight_reconstruction` | absent — `"clip"`, clipping at white; the other two values (`"blend"`, `"rebuild"`) require `input` at version 2 ([ADR 0050](adr/0050-highlight-reconstruction.md)) |
| `demosaic` | absent — `"ahd"`, LibRaw's default and ours; `"vng"`, `"dcb"` and `"dht"` require `input` at version 3 ([ADR 0061](adr/0061-demosaic-algorithm.md)) |
| `rotation` | 0.0 |
| `perspective` | absent — no correction ([ADR 0052](adr/0052-perspective-correction.md)) |
| `lut` | absent — no look applied ([ADR 0053](adr/0053-creative-lut.md)) |
| `crop` | absent — the whole image |

### Units and conventions

* `temperature`: kelvin;
* `exposure`: EV;
* `rotation`: degrees, clockwise;
* `crop`: normalised coordinates in [0, 1] relative to the image **after** rotation and perspective correction;
* `lut.strength`: an integer in [0, 100] — 100 = the LUT as its author wrote it; the blend happens on the display axis, where the LUT is defined (ADR 0053 §2–3);
* `perspective.vertical` / `perspective.horizontal`: integers in [-100, +100], 0 = neutral; they move the corners of the frame in fractions of it, hence independently of the render's size (ADR 0052 §5);
* `output_rendering.highlight_rolloff`: 0 = a hard clip at white, 100 = the longest shoulder; recovering headroom costs a little white, which the slider lets the user arbitrate;
* `highlight_reconstruction`: `"clip"` (neutral), `"blend"` or `"rebuild"` — what the **decoder** does with a channel saturated at the sensor, before demosaicing, not to be confused with the shoulder above, which decides what becomes of the headroom at the output (ADR 0050);
* sliders with no physical unit (`contrast`, `vibrance`…): integers in [-100, +100], 0 = neutral;
* `hsl[].hue`, `color_grading.{shadows,midtones,highlights}.luminance`, `color_grading.balance`: integers in [-100, +100], 0 = neutral;
* `color_grading.{shadows,midtones,highlights}.hue`: degrees, an integer in [0, 360);
* `color_grading.{shadows,midtones,highlights}.saturation`, `color_grading.blending`: integers in [0, 100], 0 = neutral;
* `camera_profile.path`: a path relative to the library root, `/` separator, by convention under `Profiles/Camera/`;
* `camera_profile.checksum`: `"blake3:"` followed by the 64 hexadecimal digits of the `.dcp` file's BLAKE3 hash (ADR 0006, applied here to a referenced input rather than to a photo). A checksum that no longer matches the file on disk **fails the render** (`CameraProfileFailed`) instead of silently rendering other colours — the same posture as `NewerSettings` (§3.4).

---

## 3.3 Stage versions

The `stages` field plays the role that a global *process version* plays elsewhere, at one granularity's difference: it is not the whole pipeline that carries a number, it is **each operator** ([ADR 0042](adr/0042-versioned-stage-pipeline.md)).

The fundamental rule:

> **No published version of the software — patch, minor or major — changes the render of an already published stage version.**

* A revision is always rendered with the stage versions it declares.
* An algorithm fix that changes the pixels produced = a new version of that stage, in a new module; the old one is never touched.
* An optimisation that produces identical pixels = no new version.
* A stage's rank in the pipeline belongs to the version: moving a stage is a new version declaring a different rank, never a modification of the existing one.
* The user can migrate a photo to the current versions (§4.5): that creates a **new revision** — the old one stays renderable identically.

**Pinning.** The map is written by the engine at the moment the revision is written, never inferred at read time:

* a stage already recorded **keeps** its version — editing a 2026 photo in 2036 does not re-render it through newer code;
* a stage that has just left its neutral value receives the engine's current version **within the working space the revision already declares** — never a version that would make it change space as a side effect ([ADR 0044](adr/0044-linear-wide-gamut-working-space.md) §4);
* a stage back at neutral **loses** its entry, since it renders nothing any more; `input` and `output_rendering` are the exception, having no neutral value.

A stage that is active but has no version recorded renders at the current version. That case concerns only settings built in memory (SDK, preset, test): every *stored* revision receives its entries when written.

The code of every stage version is kept in the engine forever: that is the price of the "same pixels ten years from now" promise, whose exact scope §5.1 states. It is now paid per operator actually fixed — a few dozen lines — and no longer by a full copy of the pipeline.

**Known stages, and the order in which they run.** The pre-publication render history was collapsed ([ADR 0043](adr/0043-collapse-prerelease-render-history.md)), since no revision in the world cited it: every stage therefore started again at version 1. Four have had a second version since: the two noise stages ([ADR 0046](adr/0046-edge-preserving-denoise.md), taken to version 3 by [ADR 0072](adr/0072-measured-noise-profile.md) — the first to change **rank** while changing version), local adjustments ([ADR 0048](adr/0048-range-masks.md), taken to version 3 by [ADR 0070](adr/0070-stored-mask-coverage.md)) and `input` ([ADR 0050](adr/0050-highlight-reconstruction.md)) — `input` having since risen to version 5 ([ADR 0061](adr/0061-demosaic-algorithm.md), then [ADR 0066](adr/0066-sensor-white-level.md), then [ADR 0107](adr/0107-derived-assets-and-the-pixel-socket.md)), and `camera_profile` to version 3 ([ADR 0062](adr/0062-dcp-illuminant-interpolation.md), [ADR 0063](adr/0063-dcp-tables.md)). The current version, the one a new revision pins, is the **last** one listed for each stage.

**A setting that a pinned version cannot express is refused.** ADR 0046 fixed a render; ADR 0048 **extends** an operator, and so brings out a case nothing had tested: a setting whose very existence depends on the stage version. Since a stage already recorded keeps its version, a revision pinned at `local_adjustments: 1` cannot carry a range mask — and `Settings::validate()` **refuses** it, naming the remedy (reprocess, §4.5) instead of letting the setting silently disappear. This is the general rule for any future feature added to an existing stage. ADR 0050 applies it a second time, one notch lower: the highlight reconstruction mode is a **decoder configuration**, which `input::v1` does not read — a revision pinned at `input: 1` therefore refuses it the same way. [ADR 0107](adr/0107-derived-assets-and-the-pixel-socket.md) applies it where the consequence is worst: an `input` below 5 would convert a buffer that is *already* in the working space, so the photograph would come back visibly wrong rather than merely unchanged. [ADR 0108](adr/0108-local-texture-clarity-sharpness-noise.md) applies it to the five neighbourhood values a local adjustment gained: a revision pinned below `local_adjustments: 4` refuses them, naming the version. [ADR 0111](adr/0111-adaptive-chromatic-aberration.md) applies it to the two measured chromatic aberration coefficients: a revision pinned at `lens: 1` refuses them, since v1 has no manual map to put them in.

| Rank | Stage | Version | Role |
|---|---|---|---|
| 0 | `input` | 1 | The configuration asked of the decoder (native sensor, linear, 16 bits) and the matrix into the working space — DCP profile, camera matrix, or sRGB decoding for a JPEG/PNG/TIFF; always active (ADR 0044) |
| 0 | `input` | 2 | The same, plus the highlight reconstruction mode asked of the decoder ([ADR 0050](adr/0050-highlight-reconstruction.md)) |
| 0 | `input` | 3 | The same, plus the demosaic algorithm asked of the decoder ([ADR 0061](adr/0061-demosaic-algorithm.md)); identical to `v2` at neutral settings |
| 0 | `input` | 4 | The same, normalising by the white level the **camera** wrote rather than by the format's ceiling ([ADR 0066](adr/0066-sensor-white-level.md)) — the only version of `input` that does **not** render like its predecessor at neutral settings, which is the correction itself |
| 0 | `input` | 5 | The same, keeping a non-RAW source's own bit depth instead of truncating it to eight, and understanding a file that is **already** in the working space — what a derivation writes ([ADR 0107](adr/0107-derived-assets-and-the-pixel-socket.md)); on every source the earlier versions could decode, identical to `v4` |
| 0 | `input` | 6 | The same, plus **the colour space a non-RAW file declares** — an embedded ICC profile, or HEIF's `nclx` box — rotated straight into the working space instead of being read as sRGB ([ADR 0115](adr/0115-tagged-source-colour.md)). On a RAW, an untagged file, or a profile this engine cannot reduce to primaries and a curve, identical to `v5` |
| 5 | `noise_luminance` | 3 | Luminance noise, at a threshold derived from the camera's **measured profile** at that sensitivity: it therefore varies per pixel. Hence the rank — a model measured on sensor counts means nothing after exposure and the tone curve ([ADR 0072](adr/0072-measured-noise-profile.md)) |
| 6 | `noise_color` | 3 | The same on the chrominance planes, with a measured threshold too (ADR 0072) |
| 10 | `camera_profile` | 1 | DCP matrix from camera to linear Rec. 2020, before everything else: it then replaces `input`'s matrix (ADR 0035, container read per ADR 0037) |
| 10 | `camera_profile` | 2 | The same, **interpolating** the two calibration illuminants in mireds according to the scene temperature, instead of averaging them ([ADR 0062](adr/0062-dcp-illuminant-interpolation.md)) |
| 10 | `camera_profile` | 3 | The same, plus the profile's tables — `HueSatMap`, `LookTable`, `ProfileToneCurve` — in the order and the ProPhoto space of the DNG specification ([ADR 0063](adr/0063-dcp-tables.md)) |
| 20 | `lens` | 1 | Distortion, transverse chromatic aberration and vignetting through a Lensfun profile (ADR 0016–0018) |
| 20 | `lens` | 2 | The same, plus two **measured** chromatic aberration coefficients folded into the same per-channel resample — the correction for a lens Lensfun has never calibrated ([ADR 0111](adr/0111-adaptive-chromatic-aberration.md)). At zero coefficients, bit-identical to v1 |
| 22 | `defringe` | 1 | Takes the saturation out of the purple and green halos axial aberration and blooming leave **beside** a high-contrast edge, hue and luminance untouched ([ADR 0113](adr/0113-defringe.md)) |
| 25 | `reshape` | 1 | Moves content *within* the frame: handles that grab at one point and drop at another, the pixels around following through an **inverse** map — nothing invented, every output pixel comes from an input pixel ([ADR 0109](adr/0109-reshape-stage.md)) |
| 30 | `spot_removal` | 1 | Deterministic cloning by a softened bilinear copy, with no *heal* mode (ADR 0031) |
| 35 | `red_eye` | 1 | Red-eye correction: a hand-placed disk, corrected by red dominance (ADR 0103) |
| 40 | `gains` | 1 | White balance and exposure: a per-channel multiplication, the buffer being already in linear light (ADR 0044) |
| 50 | `contrast` | 1 | An S-curve around middle grey |
| 60 | `highlights_shadows` | 1 | Highlights and shadows, masked by luminance |
| 70 | `whites_blacks` | 1 | Remapping of the extremes |
| 80 | `tone_curve` | 1 | A point curve, a monotone cubic spline precomputed into a table (ADR 0030) |
| 80 | `tone_curve` | 2 | The same, plus one curve per channel (ADR 0098). With none set, bit-identical to v1 |
| 90 | `clarity` | 1 | Local contrast at a large radius (ADR 0033) |
| 100 | `texture` | 1 | The same operator at a small radius (ADR 0033) |
| 110 | `dehaze` | 1 | Haze removal by *dark channel prior* (ADR 0033) |
| 120 | `vibrance` | 1 | Saturation weighted by the existing chroma |
| 130 | `saturation` | 1 | Uniform saturation |
| 140 | `hsl` | 1 | An HSL mixer over 8 hue bands, blended between adjacent bands (ADR 0032) |
| 145 | `monochrome` | 1 | Collapses each pixel to its luma. Ranked here on purpose (ADR 0088 §3): after `hsl`, so that mixer's eight luminance sliders *are* the black-and-white mix; before `color_grading`, so a black and white can still be toned |
| 150 | `color_grading` | 1 | Three zones — shadows/midtones/highlights — weighted by luminance (ADR 0032) |
| 160 | `local_adjustments` | 1 | Masked local adjustments (brush/radial/gradient), reusing the global operators restricted to a coverage (ADR 0029) |
| 160 | `local_adjustments` | 2 | The same, plus range masks: the geometric coverage can be tightened by a luminance band and a hue band (ADR 0048) |
| 160 | `local_adjustments` | 3 | The same, plus **stored** masks: the coverage may be a referenced image rather than a geometry ([ADR 0070](adr/0070-stored-mask-coverage.md)); `v1` and `v2` **refuse** it instead of ignoring it |
| 160 | `local_adjustments` | 4 | The same, plus the five operators that read a **neighbourhood** — `clarity`, `texture`, `sharpness` and the two noise reductions, which existed globally and nowhere else ([ADR 0108](adr/0108-local-texture-clarity-sharpness-noise.md)). Their radii are constants bound to this version and scaled by the render scale, never read from the revision's global sharpening. `v1` to `v3` **refuse** them. An entry setting none of the five renders exactly as `v3` renders it |
| 160 | `local_adjustments` | 5 | The same, plus the defringe pair on a mask ([ADR 0116](adr/0116-local-defringe.md)) — the operator of rank 22 run here instead, so it reads a contrast the tone curve has stretched. With neither amount set, bit-identical to `v4` |
| 165 | `lut` | 1 | A creative `.cube` LUT applied on the display axis, dosed by `strength`; the last colour decision (ADR 0053) |
| 170 | `noise_luminance` | 1 | Luminance noise reduction: a blend towards a Gaussian blur of the luma plane |
| 170 | `noise_luminance` | 2 | The same, edge-preserving: à trous wavelets and soft per-scale thresholding (ADR 0046) |
| 180 | `noise_color` | 1 | Chroma noise reduction: a blend towards a Gaussian blur of the deviations from luma |
| 180 | `noise_color` | 2 | The same, edge-preserving, with a more aggressive threshold than luminance (ADR 0046) |
| 190 | `sharpen` | 1 | Unsharp mask on the luminance plane |
| 190 | `sharpen` | 2 | The same, confined to edges by `masking` (ADR 0096). At `masking = 0`, bit-identical to v1 |
| 200 | `rotate` | 1 | Rotation by an arbitrary angle, bilinear sampling |
| 205 | `perspective` | 1 | A two-slider homography straightening converging lines; the output is the bounding box of the transformed quadrilateral (ADR 0052) |
| 210 | `crop` | 1 | Cropping |
| 220 | `vignette` | 1 | The vignette the photographer *adds*, a multiplication in linear light. Ranked after `crop` on purpose (ADR 0090 §1): it is centred on the composed frame and follows a re-framing — the opposite end of the pipeline from `lens` (20), which *removes* the one the lens made |
| 230 | `grain` | 1 | Film grain: value noise hashed from the pixel's full-resolution coordinates, monochromatic, on the display axis, faded out of blacks and highlights. No seed is stored — the field is recomputed, never replayed (ADR 0090 §3) |
| 230 | `grain` | 2 | The same, plus `color`: how much the three layers disagree. A chroma-only term added on top of the shared field, so the grain's grey stays as loud at every setting ([ADR 0118](adr/0118-coloured-grain.md)). At `color = 0`, bit-identical to v1 |
| 900 | `output_rendering` | 1 | The highlight shoulder, then Rec. 2020 → sRGB and encoding; always active (ADR 0044 §3) |

Ranks go up in tens: a future stage inserts itself between two existing ones without anyone renumbering anything.

An edited revision inherits its parent's stage versions; only new revisions by default (imports) pin the current versions.

---

## 3.4 Schema evolution

The `schema` field increments by the same rules as the generic pipelines (§4.4):

Compatible (no increment):

* adding an optional parameter with a neutral value;
* adding documentation or validation constraints.

Incompatible (increment mandatory):

* removing or renaming a parameter;
* changing type, unit or meaning;
* changing the range of values.

No migration of existing revisions is ever performed: the engine can **read** every past schema.

### Forward compatibility

An engine that meets a `schema` newer than what it knows, or a stage version it does not implement:

* never modifies the revision;
* does not edit the asset (read only);
* shows the best available preview (the last cached one) with a warning.

An old engine must never destroy the work of a recent engine. An unknown stage is never **skipped**: the render fails (`UnknownStage`), because rendering the photo without an operator its author saw would be showing them other pixels without saying so.

The same posture for a revision whose stage versions do not agree on a working space: the render fails (`MixedWorkingSpaces`), naming the two stages that disagree. An operator written for linear light, handed a gamma-encoded buffer, would produce plausible and wrong pixels — the only case worse than an error.

The `process` field, removed by [ADR 0043](adr/0043-collapse-prerelease-render-history.md), is an exception to the rule that unknown fields are preserved: a document that still carries it is **refused**. That rule protects the work of a *newer* engine; a *removed* field signals, on the contrary, a document older than the stage map, which it would be wrong to render as though it carried none.

---

# 4. Generic processing pipelines

The mechanism of RAW development generalises to any automatic processing that produces results from an asset.

Examples:

Today (V1):

* generating thumbnails and previews;
* computing histograms;
* extracting EXIF.

Tomorrow (§38 of the catalog, outside V1):

* face detection;
* OCR;
* classification by local AI;
* search vectors.

---

## 4.1 A pipeline's identity

Every pipeline has:

| Field | Description |
|---|---|
| `id` | A stable functional identifier (e.g. `face_detection`) |
| `version` | The pipeline's major version |
| `schema` | A JSON schema describing the allowed parameters |
| `description` | Optional documentation |

---

## 4.2 Runs

Every run records a **complete copy** of the parameters actually used.

```json
{
    "model": "yolov12-face",
    "confidence": 0.55,
    "min_size": 48,
    "gpu": true,
    "merge_distance": 12
}
```

Those parameters are:

* immutable;
* independent of the current schema;
* kept for the whole lifetime of the library.

The processing reads **exclusively** the parameters contained in that copy — never the application's current configuration.

### Sequence

1. load the pipeline;
2. select the version;
3. validate the parameters against the schema;
4. copy the parameters in full into `settings_json`;
5. start the processing;
6. record the results.

---

## 4.3 Validation schema

Every pipeline exposes a JSON schema:

```json
{
    "type": "object",
    "properties": {
        "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
        "gpu": { "type": "boolean" },
        "min_size": { "type": "integer", "minimum": 1 }
    },
    "required": ["confidence"]
}
```

The schema serves only to:

* validate parameters before a run;
* document the pipeline;
* generate configuration interfaces.

The schema is **never** used to reconstruct or reinterpret an old run.

---

## 4.4 Evolution and compatibility

Compatible evolutions:

* adding an optional parameter;
* adding validation constraints;
* adding documentation.

Incompatible evolutions — a new version is mandatory:

* removing or renaming a parameter;
* changing meaning or type;
* changing behaviour so that results differ.

A new version never replaces an old one. Several versions coexist:

```text
face_detection v1
face_detection v2
face_detection v3
```

Every run explicitly references the version it used.

An example of a rename between versions:

```json
v2 : { "confidence": 0.50 }

v3 : { "score_threshold": 0.50 }
```

Both formats stay valid for the runs that use them. No migration of historical parameters is permitted.

---

## 4.5 Reprocessing

When a pipeline evolves, assets can be reprocessed: reprocessing raises each pinned stage of a revision to its current version, keeping the values of the settings.

Reprocessing:

* creates a new run;
* keeps the old ones;
* never replaces historical results.

Old results stay available until the user explicitly deletes them.

---

# 5. Reproducibility

## 5.1 What is guaranteed

Two runs produce the **same result, bit for bit**, if and only if:

* the recorded parameters are strictly identical;
* the stage versions involved are identical (ADR 0042) — including those of the two framing stages, which pin the working space and the decoder configuration (ADR 0044); for a generic pipeline, the pipeline's identity and its version (§4.1);
* the input resource is identical (the same `checksum`);
* the platform and the build toolchain are the same (§5.2);
* the **decoder is the same, at the same version** ([ADR 0086](adr/0086-decoder-in-the-promise.md)) — LibRaw is linked dynamically (ADR 0004), so it is a property of the machine rather than of the build, and it is the component that turns a file into pixels. `leyline_raw::decoder_version()` reports the one that answered; a test pins it, and a change of decoder fails `make check` rather than passing quietly.

"The same result" is to be read in the strong sense: **the exported file**, not only the pixels. An export therefore carries no clock — in particular, the ICC profile embedded in every file has its creation date zeroed out, LittleCMS otherwise writing the current time into it, which made two otherwise identical exports differ by one byte (`leyline_color::srgb_icc_profile`).

That guarantee depends on **neither the application version nor the build profile**. Leyline 1.0.3 and Leyline 7.2.0 render `sharpen::v1` identically, because it is literally the same frozen code in both binaries; a `debug` build and a `release` build likewise, which the reference renders (`crates/leyline-engine/src/stages/golden.rs`, manifest in `tests/golden/renders.json`) verify in both profiles.

Those reference renders pin, with every checksum, **the `stages` map that produced it**, and replay each entry through that map. A new stage version therefore cannot move an existing checksum: it adds one. Three guards hold together — pinned entries always render the same pixels, what the engine pins *today* appears in the manifest, and no published `(stage, version)` pair escapes the manifest.

That is the right scale at which to state the promise: a user knows which stage versions their photo cites — they are written in its revision — whereas they have no idea which build produced its pixels.

Hence the publication rule, which is the operational form of the promise:

> **No published version — patch, minor or major — changes the render of an already published stage version.** If the render must change, it is a new stage version; existing revisions go on citing the old one.

A render that changes is therefore never an increment of the application's version: it is a new stage. And the rule is **mechanically verified** rather than promised — the reference render suite runs before publication, and a checksum that moves blocks the publication.

Keeping the old code in the tree is what makes that guarantee real, and a Git tag does not suffice: the 2036 binary is compiled from the 2036 tree, and a 2026 photo is correctly rendered only if `sharpen::v1` is still in it. The Git tag serves to *audit* that the stage never moved (an empty `git log` since its publication) — not to deliver it.

## 5.2 What is not guaranteed

**Changing platform or build toolchain.** The pipeline calls `powf`, `ln` and `exp`: those functions come from the system's maths library, whose results are not identical to the last bit from one platform, one libm version or one LLVM version to the next. Between two platforms, the render is therefore **visually identical, up to a last-bit drift** — not bit for bit.

Claiming otherwise would be promising what no comparable engine keeps — some run GPU and CPU paths that do not agree, others migrate the parameters of old modules onto current code rather than freezing that code. Leyline guarantees strictly more within the frame of §5.1, and stops exactly where floating point stops.

**The execution backend counts as part of the platform.** An operator ported to
another backend — a GPU among them — is a **new stage version** declaring that
backend, never a faster way to run a published one ([ADR 0080](adr/0080-the-promise-and-its-boundary.md) §3).
A machine that cannot provide the backend a revision cites refuses to render,
with a named error, rather than falling back silently onto another one: two
backends producing two images from one revision is exactly what §5.1 exists to
prevent. What remains — driver versions, vendor differences — sits in this
section for the same reason libm does. The preview path is unaffected, having
never been inside §5.1.

**The decoder is not covered by the last-bit clause above.** LibRaw is not libm: between its releases, AHD interpolation, highlight recovery and per-camera white levels have moved by amounts a photographer can see. So it sits in §5.1 as a *condition*, not here as an accepted drift ([ADR 0086](adr/0086-decoder-in-the-promise.md)) — and what §5.1 promises about it is not identical pixels across versions but that a change is **told**, never silent. Two things follow, and both are limits worth stating plainly. The reference renders are built on **synthetic images**, so the decode path has never been under a frozen reference: what guards it is a pinned version string, plus a decode manifest that anyone with RAW files of their own can fill (`crates/leyline-raw/tests/decodes.json`), and this repository ships that manifest empty. And a *pinned* decoder is not yet a *single* decoder across deliverables: the Windows leg cross-builds a pinned LibRaw while the Linux and macOS legs install what their package manager offers, so until those converge, one release of Leyline can carry more than one decoder.

A practical consequence: the toolchain is **pinned to an exact version** in `rust-toolchain.toml`. Changing it is a deliberate act, which requires replaying the reference renders and recording any drift observed — never the side effect of a bug fix.

A second consequence, about the reference renders themselves: their checksums were blessed on **one** platform, so the guard that compares them (`every_pinned_render_is_still_bit_identical`) runs only there — on the others it would be checking a binary identity that the paragraph above precisely refuses to promise, and it does fail on macOS, at the last bit, in the one case that calls `powf`. The manifest's two other guards compare no pixels and run everywhere. Measuring the drift stays possible anywhere: `cargo test -- --ignored`.

## 5.3 Determinism

Processing must be **deterministic**: any non-deterministic element (a random seed, a thread ordering that affects the result) must be fixed and recorded in the parameters. Parallelism stays permitted as long as it changes neither the formula nor the order of operations for a given sample (ADR 0012).

---

# 6. Non-destructiveness contract

The following rules are invariant:

* a pipeline may evolve;
* a schema may evolve;
* a run's parameters never change;
* historical results are never modified;
* a recomputation always produces a new run;
* every historical run stays reproducible;
* a recent engine reads every past format; an old engine never modifies a format it does not know.

---

# 7. Articulation with the catalog

| Pipeline concept | Catalog reality (`docs/catalog.md`) |
|---|---|
| A develop run | A row in `develop_revisions` |
| Immutable parameters | `settings_json` (the complete state) |
| Format version | The JSON's `schema` field |
| Render version | The JSON's `stages` field, one entry per active stage |
| Recomputation | A new revision in the graph |
| Materialised result | `previews`, invalidated by comparing `revision_id` (§20) |
| Coalescing | One revision = one intention (§17) — the granularity of runs follows the same rule |

The only exception to the immutability of revisions is the catalog's amendment window (§17), strictly bounded to unreferenced revisions.
