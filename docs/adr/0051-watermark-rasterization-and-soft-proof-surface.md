# ADR 0051 — Watermark rasterization (`ab_glyph` plus an embedded font) and the soft-proofing surface

**Status:** Accepted — 2026-07

## Context

[ADR 0034](0034-softproofing-watermark-print.md) settled the *what*: a **text**
watermark held in `ExportSettings`, composited as the very last step before
encoding; a **view-only** soft proof, an optional parameter of a preview call,
never persisted. The print module, the item's third part, has since had its own
[ADR 0036](0036-print-module.md).

What ADR 0034 explicitly left "to the PR" and which proves to be a structural
decision rather than a detail:

1. **what to draw text with.** No brick in the repository rasterizes glyphs.
   `leyline-export` depends neither on Slint nor on a text engine, and it must
   not: it is an image encoder with no interface;
2. **which font**, and where it lives. A system font would make a watermark's
   rendering depend on the machine — hence on the workstation, hence on
   reproducibility — and would contradict Local First: two exports of the same
   preset on two machines would not look alike;
3. **which way soft proofing goes** on the API side, ADR 0034 having given only
   a sketch of a structure.

## Decision

### 1. `ab_glyph` for rasterization, and nothing more

`leyline-export` gains one dependency: **`ab_glyph`** — pure Rust, with no
system dependency, deterministic, and already present in the build tree through
Slint's dependencies (hence not one more download for whoever builds Studio).

It does exactly one thing: turn a glyph outline into pixel coverage. Layout —
the text's width, the anchor position, alpha compositing — stays Leyline code,
some thirty lines, because it is rectangle placement and not typography. No
shaping engine (HarfBuzz, `rustybuzz`, `cosmic-text`): a watermark is a line of
text with no ligature and no bidi to negotiate.

### 2. An embedded font: DejaVu Sans

The file `crates/leyline-export/assets/DejaVuSans.ttf` is **embedded in the
binary** (`include_bytes!`), with its licence beside it.

* **Embedded**, because a watermark must draw identically everywhere: a system
  font would make the rendering a property of the workstation.
* **DejaVu Sans**, because its licence (Bitstream Vera + DejaVu) is permissive
  and therefore compatible with the project's GPL-3.0-only — which the
  Liberation fonts installed on most distributions, under GPLv2 with a font
  exception, are not.
* **The cost is accepted**: 757 kB in every binary. That is the price of an
  identical watermark on two machines, and this decision's only expenditure.

`ExportSettings.watermark.font` is an enumeration, today with a single value
(`"sans"`). A second face will be added as one more value, without changing the
document's shape.

### 3. The watermark is drawn in `encode`, on a copy

ADR 0034 §Watermark places the composite "immediately before encoding".
Concretely, it is `leyline_export::encode` that applies it, on a **copy** of
the buffer received: the function takes `&[u8]` and must not burn the watermark
into the caller's buffer, which is the revision's rendering.

The useful side effect: every output path going through `encode` — a simple
export, a batch, a preset — gets it without knowing, and none of them can
forget it.

Units, settled here because ADR 0034 gave only an example:

| Field | Unit |
| :--- | :--- |
| `text` | the string, non-empty |
| `font` | `"sans"` |
| `size` | a percentage of the image's **height**, within `(0, 50]` — a watermark follows the export's size, it is not measured in pixels |
| `color` | `"#RRGGBB"` |
| `opacity` | `[0, 1]` |
| `anchor` | `bottom-right` (default), `bottom-left`, `top-right`, `top-left`, `center` |

The margin between the text and the edge is half of `size`, never adjustable:
that is placement, not a decision for the user.

### 4. Soft proofing is a library method returning an image, not a file

```rust
Library::preview_soft_proofed(asset, kind, &SoftProof) -> Result<Rgb8>
```

Three properties, which are the direct translation of ADR 0034's "view only":

* it renders **in memory** and writes nothing — no preview cache, no catalog.
  That is exactly the shape of `preview_before` (the before/after comparison),
  for the same reason: a display buffer is not a deliverable;
* the `SoftProof` (a destination ICC profile, an intent, a gamut warning) is a
  call argument, never a field of a revision or a preset;
* the transform reuses [ADR 0027](0027-color-management-beyond-srgb.md)'s ICC
  primitive (`leyline_color::OutputTransform`), as ADR 0034 required.

**The gamut warning** uses LittleCMS's own proofing — a *proofing* transform
with `gamut check` and an alarm colour — and not an in-house round trip
compared against the original: it is the same library that decides what is out
of gamut and that flags it, hence a single definition of "out of gamut" in the
project.

### 5. What does not enter

* **Soft proofing in the CLI.** Proofing is a display mode; a command with no
  screen has no use for it, and exposing it would invite writing its result to
  a file — that is, exactly the export to a destination profile, which is
  another function (ADR 0027).
* **The image/logo watermark**, cut by ADR 0034 and for the reason it gave: the
  resource-reference problem is not settled.
* **A watermark on the print output.** Printing has its own output path (ADR
  0036) and its own settings; carrying the watermark there is a separate
  change, not a side effect of this one.
* **A font per language, or the choice of a system font.** §2.

## Consequences

* **Two of ADR 0034's three gaps move from "decided" to "delivered"**, five
  months after the decision, and the last (the logo) stays cut for the original
  reason.
* **`leyline-export` gains a dependency and a binary asset.** It is the
  repository's first font and the first `include_bytes!` of a data file;
  `architecture.md` keeps the list.
* **Every export path inherits the watermark** (§3), batches and presets
  included, with no extra wiring.
* **Proofing cannot pollute the cache**: it renders in memory, like the
  before/after comparison. A user who proofs and then exports gets an
  unproofed export, which is the correct behaviour — proofing shows what a
  destination *would* give, it does not produce it.
* **A single definition of "out of gamut"** in the project (§4), LittleCMS's.

## Alternatives rejected

* **Using a system font.** The watermark would become a property of the
  workstation: the photographer's name rendered in Helvetica here, in Arial
  there, absent elsewhere. Unacceptable for a decoration destined for published
  files.
* **Writing our own glyph rasterizer**, on the model of
  [ADR 0037](0037-dcp-parsing-dependency.md)'s in-house DCP reader. The
  parallel does not hold: reading a few TIFF tags is bounded and verifiable,
  rasterizing TrueType outlines correctly (hinting, anti-aliasing, kerning) is
  not, and the result would be visibly worse for no gain.
* **An in-house bitmap font**, with no dependency and no asset. An aliased
  watermark on a 6000 px export: the function would lose its reason for being.
* **Drawing the watermark in the engine rather than in the encoder.** It would
  have to be done in every output path, and a forgotten path would show nothing
  without erroring. `encode` is the compulsory passage.
* **An in-house ICC round trip for the gamut warning** (transform, transform
  back, compare). Two definitions of "out of gamut" in the project, one of them
  ours, for information LittleCMS already gives.
* **Persisting the chosen proof in the revision.** Rejected by ADR 0034;
  nothing has changed.
