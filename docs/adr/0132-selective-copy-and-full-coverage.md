# ADR 0132 — Choosing what a copy carries, and giving it something to carry

**Status:** Accepted — 2026-09

## Context

Copying a development onto other photographs is the second gesture of a
working day, after culling. Studio has it — `Ctrl+C` / `Ctrl+V`, applying to
the whole selection — and two things are wrong with it, one of which was
invisible until counted.

**The visible one.** What is copied is a constant: `CLIPBOARD_GROUPS`, six
categories, always the same six. There is no way to say *only the white
balance of this one onto those*, which Lightroom has had since version 1 and
darktable offers as selective copy.

**The one that had to be counted.** `PresetSettings::capture` has seven arms.
`Settings` has thirty-five fields that are settings. Counted on the source
rather than remembered:

> 16 settings belong to **no category at all**: `camera_profile`, `lut`,
> `clarity`, `texture`, `dehaze`, `tone_curve`, `hsl`, `color_grading`,
> `spot_removal`, `reshape`, `red_eye`, `local_adjustments`,
> `output_rendering`, `highlight_reconstruction`, `demosaic`, `perspective`.

Half the develop panel. A photographer who grades a photograph, mixes its
colours, curves its tones and copies the result gets the exposure and the
vignette — and is told nothing. The same hole is in every **saved preset**,
which captures by the same categories: ADR 0058's preset shelf cannot hold a
look that is made of a tone curve.

So a checkbox dialog over the seven existing categories would advertise the
hole rather than close it. This ADR closes it first, and then hands the
choice over.

## Decision

### 1. A category for every block of the develop panel

Seventeen categories, and the rule that generates them: **one category per
block of the develop panel**, because the panel is where the photographer
already decides what a photograph's development is made of, and a dialog
that names the same things needs no learning.

| Category | Fields |
|---|---|
| `WhiteBalance` | `white_balance` |
| `Tone` | `exposure`, `contrast`, `highlights`, `shadows`, `whites`, `blacks` |
| `Presence` | `clarity`, `texture`, `dehaze`, `vibrance`, `saturation`, `monochrome` |
| `ToneCurve` | `tone_curve` |
| `ColorMixer` | `hsl` |
| `ColorGrading` | `color_grading` |
| `CameraProfile` | `camera_profile` |
| `CreativeLut` | `lut` |
| `Effects` | `vignette`, `grain` |
| `LensCorrection` | `lens_correction`, `defringe` |
| `Detail` | `noise_reduction`, `sharpening` |
| `Rendering` | `highlight_reconstruction`, `demosaic`, `output_rendering` |
| `Geometry` | `rotation`, `crop`, `perspective` |
| `Reshape` | `reshape` |
| `SpotRemoval` | `spot_removal` |
| `RedEye` | `red_eye` |
| `LocalAdjustments` | `local_adjustments` |

Three of them widen a category that already existed rather than adding a new
one, and each for the same reason — the field was already in that block of
the panel: `Presence` takes clarity, texture and dehaze (they sit under
Vibrance in Basic, and ADR 0108 put them there); `Geometry` takes
`perspective`, since a keystone correction is geometry by every reading;
`LensCorrection` already had that shape from ADR 0113 §5.

`Rendering` is the one category named after nothing in the panel's headings:
it is Basic's Full-mode block — what the decoder does with clipped channels,
which interpolation it uses, and how the working buffer becomes a display
signal. Grouped together because they are the three settings that are about
*getting the photograph out of the file*, and a photographer who changes one
on a difficult frame usually means it for the frames beside it.

### 2. Atomicity stays, and the panel remains the granularity

`docs/presets.md` §3.1's rule is unchanged: a category is all-or-nothing, and
there is no per-field checkbox. Seventeen boxes is already at the edge of what
a dialog can ask; thirty-five would be a form, and the answer to "which of
these six numbers is my contrast" is not one a photographer should have to
give.

### 3. Four fields are refused a category, by name

* **`source_encoding`** — what the file's samples already *are* (ADR 0115).
  Not a decision about the photograph but a fact about the file; carrying it
  across would relabel one file's colour with its neighbour's, and the render
  would be wrong in a way nothing on screen would explain.
* **`schema`** and **`stages`** — the revision's own versioning
  (ADR 0042, ADR 0043). A preset never fixes a rendering version; the stages
  come from the revision it lands on, which is ADR 0043 §3 and stays true.
* **`extra`** — the forward-compatibility passthrough of unknown fields. It
  belongs to the document that carried it.

This list is enforced, not merely written: §7.

### 4. A whole-list parameter for local adjustments

`Param::LocalAdjustment(index)` edits one layer — it adds at `len`, replaces
below it and removes. It cannot express *make this photograph's layers be
these*, because that needs the target's current length, and a captured set of
settings is built once and applied to many photographs whose lengths differ.

So `Param::LocalAdjustments` (plural) is added: one value, the whole list,
replacing whatever was there. The indexed parameter stays — it is what the
mask tools use, one gesture at a time.

Nothing about the stored document changes: `settings_json` has held
`local_adjustments` as a list since ADR 0029. This is an edit verb, not a
format.

### 5. The choice is made at copy, and `Ctrl+C` never asks

Two reasons, and the first is not a preference:

`PresetSettings.groups` **is** the source of truth for what a captured set
touches (`docs/presets.md` §3.2) — that is what lets a preset return a
category to neutral, an absent field meaning "not included" rather than
"leave alone". Filtering again at paste would put a second answer next to
that one, and the two would eventually disagree.

And it is the model a photographer arrives with. Lightroom asks at copy;
darktable asks at paste, and darktable's is the one users describe as needing
a manual.

So: **`Ctrl+C` copies the default set with no dialog** — the fast path stays
one keystroke — and **`Ctrl+Shift+C` opens *Copy Settings…***, which asks and
then copies. `Ctrl+V` pastes what was copied, onto the whole selection, as it
already does.

### 6. Defaults: the look, never the place

Checked when the dialog first opens: `WhiteBalance`, `Tone`, `Presence`,
`ToneCurve`, `ColorMixer`, `ColorGrading`, `CameraProfile`, `CreativeLut`,
`Effects`, `LensCorrection`, `Detail`.

Unchecked: `Geometry`, `Rendering`, `Reshape`, `SpotRemoval`, `RedEye`,
`LocalAdjustments`.

One rule generates that split: **what a look is made of is checked; what a
place in this photograph is made of is not.** A crop, a mask, a spot repair, a
pair of eyes and a reshape are all statements about *this* frame, and
`docs/presets.md` §3.1 had already reached that conclusion for geometry alone.
`Rendering` joins them from the other side: it is about this *file*, and a
photographer copying a look does not mean to change a demosaic.

This changes the default copy set, which until now was six categories: a
plain `Ctrl+C` now also carries the tone curve, the colour mixer, the colour
grading, the camera profile and the LUT. That is the correction, not a side
effect — those are the look, and their absence was the defect.

The last set chosen in the dialog is remembered across launches, in
`preferences.json` — and stored as the **category names**, not as the
seventeen-bit number the dialog composes. That is where this parts company
with ADR 0128 §3's arrangement: those numbers are opaque to Rust, and this one
cannot be, since it has to become a `Vec<SettingsGroup>`. A stored bitfield
would also silently become a different set the day a category is inserted
rather than appended, and a preferences file is exactly the thing that
outlives such a change.

The bit order is therefore a contract between the widget and `crate::groups`,
written down on both sides. It is not arbitrary — it is the order of the table
in §1, which is the order of the panel — and appending is the only permitted
change.

### 7. The hole cannot reopen in silence

A test in the engine builds a `Settings` with every field moved off its
default, captures it with **every** category, overlays the result onto a
default `Settings`, and asserts the two agree — field by field, except the
four §3 names.

Behavioural rather than a list of field names, so it is not a second place to
forget something: adding a field to `Settings` without giving it a category
fails it, and so does giving it a category whose `param_values` arm was never
written.

### 8. Out of scope

* **Selective paste.** §5. If it is ever wanted, it is a filter on
  `PresetSettings`, and the ADR that adds it has to answer what `groups` then
  means.
* **Per-field checkboxes.** §2.
* **A "copy everything" button.** Every box has a checkbox; a button that
  ticks them all is one row of chrome for a gesture that is already there.
* **New CLI verbs.** `--groups` gains the ten new names and nothing else
  moves.

## Consequences

* `preset_json` gains eleven optional fields. Stored presets are untouched and
  keep working: an absent field already means "not included", which is
  `docs/presets.md` §3.2's rule and not a new tolerance.
* Saved presets can now hold a tone curve, an HSL mix and a colour grade —
  ADR 0058's shelf becomes able to carry the looks it was built for. That is a
  larger consequence than the copy dialog, and it comes for free.
* `docs/presets.md` §3.2's example writes `"groups": ["white_balance", …]`.
  The enum has no `rename_all`, so the real document holds `"WhiteBalance"`.
  The spec is corrected to what the code writes rather than the reverse:
  renaming would make every stored preset unreadable, and the wire form of an
  enum is not worth a migration.
* Found on the way, and fixed here because §1 walked straight into it:
  `Option<Option<T>>` is how "included, and cleared" is told apart from "not
  included", and serde folds `null` and *key absent* into the same `None`. So
  a Geometry preset captured from an **uncropped** photograph stored
  `"crop": null` and read back as *not including* the crop — applying it left
  the target's crop alone instead of clearing it. Silently wrong since presets
  shipped; caught by the round-trip test the moment `perspective` was given
  the same shape. `crop`, `perspective`, `camera_profile` and `lut` now
  round-trip through a `double_option` helper.
* Seventeen categories is a long dialog. It is a two-column list, and the
  order is the panel's own order — a photographer looking for *Colour Grading*
  finds it where it is on the right of the screen.

## Alternatives rejected

* **The dialog alone, over the seven existing categories.** It was the
  request. It would have shipped a checkbox list from which half the develop
  panel was missing, and the missing half is exactly the part a look is made
  of.
* **Renaming the enum to snake_case to match the spec.** Every stored preset
  in every existing library holds the PascalCase form. A migration to make a
  document prettier is a migration for nothing.
* **A category per field, hidden behind an "advanced" disclosure.** Two
  granularities for one question.
* **Making `Ctrl+C` open the dialog.** The fast path is the common one: copy
  the look, paste it on forty frames. A dialog on the common path to serve the
  rare one is the trade backwards.
