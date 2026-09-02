# ADR 0110 — Contact sheets: a page that holds a grid, and nothing else new

**Status:** Accepted — 2026-09

## Context

[ADR 0036](0036-print-module.md) settled printing as "an export with a
physical dimension and a destination profile" and, in the same breath,
**cut contact sheets from V2** — not as an oversight, but because the thing
it refused to half-design was a *layout engine*: "arbitrary grid geometry,
mixed aspect ratios, crop-to-cell, multi-page pagination". It closed with a
promise this document keeps: "contact sheets will come back in their own
future ADR if they are ever wanted".

They are wanted. The gap survey run against a current competitor ranked the
contact sheet as **the only real hole left in the print module** — everything
else in that survey is either shipped or refused in writing. And the cut's
own justification has aged: the pieces ADR 0036 named as the hard part are
each now decided elsewhere. `PrintSettings` already describes a page (paper,
orientation, margins, DPI, destination profile, intent) and already turns it
into pixels; [ADR 0051](0051-watermark-rasterization-and-soft-proof-surface.md)
already embedded a typeface and a glyph rasteriser so that text drawn into an
output is the same on every machine; [ADR 0027](0027-color-management-beyond-srgb.md)'s
ICC primitive already converts an output buffer into a destination profile.

What is left is arithmetic on a rectangle. This ADR does that arithmetic, and
takes care to add **nothing else**.

## Decision

### 1. A contact sheet is a print whose page holds a grid

`ContactSheetSettings` **contains a whole `PrintSettings`** as its `page`
field, rather than restating paper/margins/DPI/profile/intent:

```rust
pub struct ContactSheetSettings {
    pub page: PrintSettings,     // ADR 0036, unchanged and entire
    pub columns: u32,
    pub rows: u32,
    pub gutter_mm: f32,
    pub caption: CaptionSource,  // None | Filename
    pub caption_mm: f32,
}
```

Containment, not a fourth flavour of page description: the paper is described
in exactly one place in this codebase, `PrintSettings::page_mm` /
`printable_area_mm` / `target_pixels` are reused as they are, and a contact
sheet inherits every future page decision for free. It is the same move
[ADR 0025](0025-unified-export-request.md) made when it refused a second
export request type.

The nesting is **explicit in the JSON** (`{"page": {...}, "columns": 5}`)
rather than flattened, because `deny_unknown_fields` — the rule every
`settings_json` in this repository obeys, so that a preset written by a newer
engine is never applied by halves — cannot be combined with a flattened
field.

### 2. One document for the batch — which is why it is not a flag on `PrintRequest`

ADR 0036's print writes **one PDF per version**. A contact sheet writes **one
multi-page PDF for the whole request**. That difference in what a request
*produces* is the reason this is a separate call rather than a `rows`/`columns`
field on `PrintRequest` defaulting to 1×1: a 1×1 sheet is not today's print,
it is today's print collapsed into a single N-page file, and silently changing
what `leyline print` writes would break the one thing already shipped.

```rust
pub enum ContactSheetRecipe { Adhoc(ContactSheetSettings), Preset(ContactSheetPresetId) }

pub struct ContactSheetRequest {
    pub versions: Vec<VersionId>,   // job data, in reading order
    pub recipe: ContactSheetRecipe,
    pub destination: PathBuf,       // one file, refused if it exists
}
```

`copies` is **not** carried. ADR 0036 carried it as an inert passthrough for
the OS dialog's copy count; a sheet is a file, and repeating a file N times is
the print dialog's business, not the engine's. Nothing is lost that was ever
used.

### 3. The layout, stated exactly — and deliberately independent of the images

Inside the printable area (`PrintSettings::printable_area_mm`, i.e. paper
minus margins), `columns × rows` **equal** cells, separated by `gutter_mm`:

```
cell_w = (printable_w - (columns - 1) × gutter) / columns
cell_h = (printable_h - (rows    - 1) × gutter) / rows
```

Each cell reserves a caption band of `caption_mm × 1.5` at its bottom when
captions are on; the image box is what remains. Every image is scaled to
**fit** that box, preserving its aspect ratio, and centred in it. Images flow
left to right then top to bottom, `columns × rows` per page; the last page is
partially filled.

Two properties are the decision, and both are what makes this a *contact*
sheet rather than a collage:

* **The geometry never depends on the images.** Cell boxes are computed from
  the page alone, before a single photo is decoded. A landscape and a portrait
  frame get the same cell and simply letterbox differently inside it — mixed
  orientations, the case ADR 0036 named as hard, cost exactly nothing once the
  grid refuses to negotiate with its content.
* **Nothing is ever cropped.** Fit, never fill. A contact sheet exists to show
  what a frame *is*; a crop-to-cell mode would show something the photograph
  is not, and would have to invent a crop rule the develop pipeline already
  owns ([ADR 0026](0026-mask-spot-coordinate-referential.md)'s frame, the
  `crop` stage). Refused, not deferred with a placeholder field.

### 4. The page is composed as one raster, and then it is a print

The layout runs in **pixel space at the page's DPI**: a white canvas of
`page_mm × dpi`, each rendered cell blitted at its computed origin, captions
drawn into it, and then the finished page handed to the same PDF path
`encode_print` uses (`encode_contact_sheet`, N pages of identical pixel size,
never overwriting an existing file).

This is what keeps the promise of adding nothing: the destination ICC
transform applies once per page, exactly as ADR 0036 applies it once per
print, and **no new rendering algorithm appears anywhere** — the composer
only copies pixels it was handed.

The cost is measured rather than guessed: a page is one image stream in the
PDF, losslessly compressed (Flate). Six real CR2 frames on an A4 give
**0.7 MB per page at 200 DPI and 2.8 MB at 300**; a page dense with detail
costs more, since Flate on photographic pixels is the only thing paying. The
knob is `page.dpi`, and a contact sheet's default is **200 DPI** rather than a
print's 300 — a sheet is an index read at arm's length, not a fine print. A
second, lossy image encoder is **not** brought into `leyline-export` for this:
the crate deliberately carries one narrow codec per format
([ADR 0067](0067-avif-encode-speed.md)'s tree), and a JPEG-in-PDF path would be
a second JPEG encoder beside `jpeg-encoder`.

### 5. Captions are drawn with the typeface the binary already carries

One optional line per cell, centred **under the photograph itself** — not
at the foot of the cell, where a label sitting a third of a page below a
letterboxed landscape frame would belong to nothing; the band reserved at the
cell's bottom is what guarantees there is room for it wherever the image ends.
Drawn with
**ADR 0051's embedded DejaVu Sans and its glyph rasteriser** — the same
reasoning as the watermark: a system font lookup would make the same sheet
different on another machine. `CaptionSource` has two values, `None` and
`Filename`, and the default is **`Filename`**: an unlabelled contact sheet
cannot answer the only question it exists to answer, which frame is which.

A caption too wide for its cell is **truncated with an ellipsis**, never
scaled down and never wrapped — a shrinking caption makes a grid of
different-sized text, and a wrapping one steals the height of the cell below.

Rating, capture date, and the rest of what a caption *could* say are refused
here, with the reason stated: each one is a formatting decision (locale, time
zone, the two capture-date conventions this repository has already been bitten
by) that belongs to a document about metadata rendering, not to this one.

### 6. A failed photo leaves a hole, and the sheet is still produced

A version that fails to decode or render leaves its cell **empty** — the cell
is not reclaimed and the following photos do not shift up — and is reported in
`ContactSheetReport.failed` beside the versions that made it. Position is
information on a contact sheet: shifting the grid to hide a failure would
silently renumber every frame after it. This mirrors `PrintReport`'s
per-version failure list ([`FailedPrint`](0036-print-module.md) is reused
rather than cloned), with one difference that follows from §2: a batch print
loses only the photo that failed, whereas a contact sheet would lose the whole
sheet, so refusing to produce it is the worse answer.

### 7. Cells are decoded at half size when they are small

A cell is small by construction: five columns on an A4 at 200 DPI is a 300-pixel
box, against a 5472-pixel source. When the cell's box fits inside half the
decoder's full output, the RAW is decoded **half size** — the same
`decode_params(settings, half_size)` switch `PreviewKind::Thumbnail` and
`Small` already flip, for the same reason.

This is explicitly permitted rather than sneaked in: a print is outside
[`pipeline.md`](../pipeline.md) §5's contract (ADR 0036 — printing modifies no
revision and reproduces no pixel), so the choice is a cost/quality one, not a
correctness one. It is also the difference between a forty-photo sheet taking
half a minute and taking two.

### 8. Persistence — a `contact_sheet_presets` table

Parallel to `print_presets` (`docs/catalog.md` §42), itself parallel to
`export_presets` (§27) — the new section is §45: `id`, `uuid`, `name`, `settings_json`, `created_at`,
the `settings_json` being exactly `ContactSheetSettings`. No history table, for
ADR 0036's reason unchanged — a sheet modifies no revision and is not a state
to journal.

A separate table rather than a row in `print_presets`: those rows are read
back as `PrintSettings`, which refuses unknown fields, so a contact-sheet
recipe stored among them would make an existing preset listing fail — the
mechanism that protects a preset from being half-applied is the same one that
forbids mixing two shapes in one table.

## Consequences

* **ADR 0036's cut is lifted, by the ADR it asked for.** The print module's
  last named gap closes; nothing else in that document changes, and no print
  written by it renders differently.
* **No process version, no stage, no `settings_json` in a revision.** A
  contact sheet is an output concern, exactly like a print, a watermark
  ([ADR 0034](0034-softproofing-watermark-print.md)) and a soft proof. The
  reproducibility contract is not engaged, and the golden manifest does not
  move.
* **`leyline-export` gains geometry and a composer; the engine gains
  orchestration.** The split is ADR 0036's, unchanged: pixel/geometry work in
  `leyline-export` next to `PrintSettings` and the watermark, catalog work and
  rendering in `leyline-engine`, and **nothing about an OS print dialog
  anywhere in the engine** (`docs/engine-api.md` §14, the pattern of
  [ADR 0020](0020-menu-bar.md)).
* **Three clients, one shape.** `leyline contact-sheet` / `contact-sheet-preset`
  / `contact-sheet-presets` in the CLI, a grid section in Studio's print
  dialog that prints the current selection, and the SDK façade re-exporting the
  new types — the façade rule of
  [ADR 0025](0025-unified-export-request.md)/`tests/surface.rs` applies, and a
  type reachable from a client that the façade does not name is a bug the
  surface test must catch.
* **One new migration** (schema v12) for one new table. A library that never
  makes a contact sheet pays one empty table.
* **A sheet's file size is a measured cost, not a surprise**: lossless page
  images, 0.7 MB per A4 page at 200 DPI and 2.8 MB at 300 on real frames, with
  `dpi` as the knob. The default of 200 DPI is a deliberate departure from
  `PrintSettings::default`. Seven real CR2 frames on two pages take five
  seconds end to end, half-size decode included.

## Alternatives rejected

* **A `rows`/`columns` field on `PrintSettings`, 1×1 by default.** Tempting —
  one type, one CLI verb, one dialog. Rejected because it changes what an
  existing request *writes*: today's `leyline print a b c` produces three
  PDFs, and a grid-aware print produces one three-page file. The shape of the
  output is not a setting.
* **Crop-to-fill cells (square cells with the frame cropped).** Rejected: a
  contact sheet's job is to show what the frame is. Filling a square cell means
  choosing a crop, and the choice of a crop belongs to the develop pipeline,
  where the user made it. Fit-and-letterbox is the only rule that never lies.
* **Placing every cell as its own image in the PDF instead of composing one
  page raster.** Rejected after arithmetic rather than by taste: twenty cells
  of 560×840 carry 9.4 Mpx, an A4 page at 300 DPI carries 8.7 Mpx — the same
  bytes, since the page's white ground compresses to nothing. Per-cell XObjects
  would buy no file size and would move the caption problem into PDF text
  layout, where the embedded typeface would have to be subset and embedded.
* **Enabling `printpdf`'s JPEG feature to shrink the file.** Rejected: it pulls
  the `image` crate's JPEG encoder into `leyline-export`, which deliberately
  depends on one narrow codec per format and already encodes JPEG with
  `jpeg-encoder`. Two JPEG encoders in one binary is a bad trade for a file
  that measured under a megabyte a page at the default DPI — and lowering
  `dpi` costs nothing at all.
* **Storing contact-sheet recipes in `print_presets`.** Rejected: `PrintSettings`
  refuses unknown fields, so one contact-sheet row would break every listing of
  print presets. The tables are cheap; the mixed shape is not.
* **Captions carrying rating, date or EXIF.** Rejected for now, and the reason
  is not "later": each is a formatting decision (locale, time zone — this
  repository has already paid for two capture-date conventions) that belongs to
  a document about rendering metadata. The filename is the one caption that is
  a fact.
* **Reclaiming the cell of a photo that failed to render.** Rejected: the grid
  position is how a person points at a frame on a printed sheet. A silent
  renumbering is worse than a visible hole beside a reported failure.
