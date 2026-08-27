# ADR 0036 — The print module: "an export with a physical dimension and a destination profile", one photo per page, the OS hand-off left to Studio

**Status:** Accepted — 2026-07

## Context

`docs/v2-scope.md` §7 groups three gaps under one item — soft proofing,
watermark, print module — stressing that **it is the least "pipeline" item**:
none of the three modifies the stored revision's pixels. ADR 0034 settled the
first two thirds (view-only proofing, a text watermark at export) and
**explicitly deferred printing to "its own future ADR"**, sizing it L/XL and
"mostly UI plus a dedicated output path", refusing to stub it with filler
decisions. This document is that announced ADR.

Two cross-cutting decisions are **consumed here, not relitigated**:

* **ADR 0027** widened `leyline-color` into a general ICC transform library
  (loading a profile, building a `cmsTransform`, applying it), and explicitly
  provided that "the print module will be able to lean on that same
  output-transform primitive rather than inventing a third". That is exactly
  what this document does: the conversion to the printer/paper profile reuses
  the primitive ADR 0027 built and that ADR 0034 already reuses for proofing
  and non-sRGB export.
* **ADR 0025** (`docs/engine-api.md` §12) unified export behind a single
  `ExportRequest` (the versions to process), an `ExportRecipe`
  (`Adhoc`/`Preset`) and an `ExportPresetId` resolved at execution — the shape
  this document takes over as it stands for printing.

What remained to settle, item 7's last third, is the **concrete shape** of the
print module. That is this ADR's subject.

## Decision

### No process version, no `settings_json`, no pipeline stage

**Printing is not a process version, does not touch develop `settings_json`,
and inserts no stage into `docs/pipeline.md` §3.1's pipeline order.** It is
exactly the same category as ADR 0034's watermark and proofing: printing does
not change a revision's pixels, it is an **output concern**. ADR 0034's
reasoning for its two pieces applies word for word here — nothing in printing
modifies the pixels of a stored revision, so the reproducibility contract
(`docs/pipeline.md` §5, "the same revision → the same pixels") is not engaged,
and no process version is introduced (ADR 0028's numbering rule therefore does
**not** come into play here).

### V2's scope — one photo per page, contact sheets are cut

**V2 prints a single photo per page.** Contact sheets and N-up layouts (several
photos per page, arbitrary grid mathematics, mixed aspect ratios,
crop-to-fill rules) are **cut from V2** — a deliberate cut, not an oversight. A
contact sheet is a **layout engine** problem materially larger than "page +
margins + DPI" for a single image: one must settle the grid's geometry, the
handling of mixed orientations, crop-to-cell, and multi-page pagination. It is
another kind of undertaking, exactly as ADR 0032 cut seamless *heal*, ADR 0030
the parametric curve, ADR 0034 the image/logo watermark and ADR 0031/0033 the
regional effects: name **the smallest genuinely useful thing** — printing one
photo at a chosen size, paper and profile — and cut the rest cleanly rather
than half-designing it. Contact sheets will come back in their own future ADR
if they are ever wanted.

### Architecturally, "an export with a physical dimension and a destination profile"

Printing is **not a new subsystem**. The render path reuses `leyline-export`'s
existing machinery:

* `leyline-export`'s render-to-buffer and then encoding
  (`crates/leyline-export/src/lib.rs`) are reused as they are; the only
  difference is **how the target dimensions in pixels are computed**: instead
  of a `max_edge` in pixels (ADR 0025/0027), one computes
  `paper size × DPI` (say, 15 × 10 cm at 300 DPI). That is one more sizing
  mode, not one more render path.
* The conversion to the printer/paper profile reuses **ADR 0027's ICC
  primitive** (`leyline-color`) — the same one ADR 0034's proofing and
  non-sRGB export already use. Optionally, ADR 0034's proofing view (with its
  gamut warning) can be chained **before** confirming the print, so that the
  user previews the print's colorimetric behaviour before spending paper and
  ink.

**No new rendering algorithm is introduced anywhere in this ADR.**

### Persistence — a `print_presets` concept, parallel to `export_presets`

Printing is stored as a **preset**, on the exact model of `export_presets`
(`docs/catalog.md` §27) and `develop_presets`. A print preset captures: paper
size, margins/orientation, target DPI, a reference to the destination ICC
profile, and the rendering intent — all in a `settings_json` blob, **the same
mechanism** already established for `export_presets`/`develop_presets`, with no
new mechanism invented.

The **per-job data** (which photos, how many copies) stays an input at request
time, **not** part of the preset — exactly as `ExportRequest.versions` is
separate from the `ExportRecipe`/the stored preset (`docs/engine-api.md` §12,
ADR 0025). That shape is followed directly:

```rust
pub enum PrintRecipe {
    /// Settings supplied by the caller, not stored.
    Adhoc(PrintSettings),
    /// A stored preset, resolved when the request runs.
    Preset(PrintPresetId),
}

pub struct PrintRequest {
    pub versions: Vec<VersionId>,   // which photos — a job input, never in the preset
    pub recipe: PrintRecipe,        // Adhoc(...) or Preset(id), like ExportRecipe
    pub copies: u32,                // a job input, never in the preset
}
// the exact shape left to the PR, as for every ADR in this series.
```

A `print_presets` table, parallel to `export_presets` (`docs/catalog.md` §27,
with the id/uuid/name/settings_json/created_at columns taken over identically):

```sql
CREATE TABLE print_presets (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    settings_json TEXT NOT NULL,   -- paper, margins, orientation, DPI, ICC profile, intent
    created_at INTEGER NOT NULL
);
```

A sketch of a print preset's `settings_json`:

```json
{
    "paper": "A4",
    "orientation": "portrait",
    "margins_mm": { "top": 10, "right": 10, "bottom": 10, "left": 10 },
    "dpi": 300,
    "profile": "Profiles/Print/CansonBaryta.icc",
    "intent": "relative-colorimetric"
}
```

### The crate/ownership split — this ADR's underlying architectural decision

The division of responsibility is this ADR's real decision; it reads in two
halves.

**Rendering — producing a raster or file ready to print at the target physical
size and in the destination profile — is the engine's**, as an extension of
`leyline-export`/`leyline-color`, **with no new crate**. The same reasoning as
ADR 0035 for the placement of the DCP work: it is colour/output domain logic
(physical sizing plus a destination ICC transform), not buffer-coupled
infrastructure that would demand its own crate. `paper × DPI` sizing and
profile conversion are adjacent to the export work already housed there.

**The physical hand-off to a printer — the OS print dialog, driver
communication, spooling — is explicitly NOT engine work.**
`docs/engine-api.md` §14 is categorical: "No on-screen rendering: the engine
produces files and buffers, display belongs to the client", "No window
management, shortcuts or UI selection". The OS print dialog is precisely window
management and system integration. And it is **exactly the same pattern** ADR
0020 (the menu bar) and ADR 0021 (the context menus) already applied: OS/UI
integration lives **entirely in `leyline-studio`**, wired onto already-existing
engine calls, **without adding the slightest surface to the engine** ("No
change to the event model or to the engine API", ADR 0020; "No new engine
capability", ADR 0021). Here likewise: Studio calls the engine to **render a
print-ready file**, and then Studio — not the engine — invokes the platform's
native printing mechanism.

### An explicit open risk — the hand-off mechanism, left to the PR

**How** Studio precisely hands the rendered output to the OS's print flow is a
**genuinely unresolved risk**, left to the implementation PR — not a decision
this ADR arbitrarily dodges. The finalists are named, the choice deferred:

* **A self-generated PDF at the target physical page size, with an embedded
  profile** — probably the most portable choice across the Windows/macOS/Linux
  print dialogs, since almost every OS print flow accepts a PDF. But that is
  **not** stated here as an established fact.
* **A raw raster passed to a platform-specific printing API.**
* **The printing surface Slint may or may not expose** itself.

That choice depends on Slint's real capabilities and on each platform's native
print integration **when the time comes** — it is named with the same honesty
ADR 0035 used for the choice of DCP parser. This ADR does **not** invent a fake
resolution (it does not assert "Leyline generates a PDF" as a settled fact): it
states the finalist options and explicitly defers the choice.

## Consequences

* **This ADR entirely closes `docs/v2-scope.md`'s item 7**: its three
  sub-pieces are now settled — proofing and watermark by ADR 0034, the print
  module by this document. The only remaining point is the **named risk** of the
  OS hand-off mechanism, flagged for the implementation PR.
* **No process version, no pipeline stage**: like ADR 0034's watermark and
  proofing, printing lives outside the revisions' reproducibility contract
  (`docs/pipeline.md` §5) — it is an output surface, not a modification of a
  revision.
* **Print rendering inherits the export plumbing and ADR 0027's ICC primitive
  for free**: no new rendering algorithm and no third colour path — `paper ×
  DPI` sizing replaces `max_edge`, and the profile conversion is ADR 0027's,
  already shared with proofing and non-sRGB export.
* **The print preset inherits the preset pattern for free**: a `print_presets`
  table parallel to `export_presets`, a `settings_json` blob, no new mechanism;
  the job data (photos, copies) stays separate from the preset, as
  `ExportRequest.versions` is from `ExportRecipe`.
* **The engine gains no OS integration surface**: the printer hand-off lives
  entirely in `leyline-studio`, exactly like the menus (ADR 0020) and the
  context menus (ADR 0021) — the engine renders a file, Studio hands it to the
  OS.
* **Contact sheets stay open for a future ADR** with their real cost (a layout
  engine, the grid, mixed orientations, pagination): V2 only refuses to commit
  to them, it does not close the door.
* **The hand-off mechanism stays an accepted open risk** for the PR: a portable
  PDF, a raster plus a platform API, or a Slint surface — a decision that
  depends on the real capabilities at implementation time, not settled
  speculatively here.
* **It prejudges no future plugin/module system.** `docs/roadmap.md` lists
  "Plugins, a stable SDK" under Long term, outside V2. The engine/Studio split
  decided here (rendering = engine, OS hand-off = Studio) is an internal
  placement choice for V2 — it does not preclude a future plugin system
  grafting onto it, for instance to supply alternative print backends or
  contact-sheet layouts (cut today, see above) without touching the engine.
  Nothing here commits to the shape of that future mechanism.

## Alternatives rejected

* **Supporting contact sheets / multi-image page layouts from V2 on.**
  Rejected: a contact sheet is a layout engine (arbitrary grid geometry, mixed
  aspect ratios, crop-to-cell, multi-page pagination) — a problem materially
  larger than page/margins/DPI for a single image, and of another nature from
  the rest. Half-designing it here would invent an architecture nobody has
  framed ("No code before architecture"). We name the smallest useful thing
  (one photo per page) and cut the rest cleanly, as ADR 0032/0030/0034 did for
  their respective scopes. Contact sheets will have their own ADR if they are
  ever wanted.
* **Making printing a develop pipeline stage / a process version.** Rejected:
  printing does not modify a stored revision's pixels, it is an output concern
  — exactly the observation ADR 0034 made for the watermark and proofing.
  Writing it into `settings_json` or giving it a process version would
  introduce state affecting no revision pixel and would lie about what a
  revision is, while needlessly engaging the "same revision → same pixels"
  contract (`docs/pipeline.md` §5). Printing lives at the output, like export.
* **Having the engine own the OS print dialog's integration.** Rejected:
  `docs/engine-api.md` §14 explicitly excludes window management and on-screen
  rendering from the engine — "display belongs to the client". A system print
  dialog is OS/UI integration, precisely what ADR 0020 (the menu bar) and ADR
  0021 (the context menus) housed **entirely in `leyline-studio`** without
  adding the slightest surface to the engine. Making the engine a print-dialog
  manager would break that established pattern and make it platform-dependent —
  exactly what the engine/client boundary exists to prevent. The engine renders
  a file; Studio hands it to the OS.
* **Settling a precise hand-off mechanism right now (say, "always generate a
  PDF").** Rejected: that choice depends on Slint's real capabilities and on
  each platform's native print integration at implementation time — asserting
  it settled today would be inventing a fake resolution. The portable PDF is
  the likely finalist, but a raster plus a platform API and a possible Slint
  surface stay real candidates. This ADR names the finalists and honestly
  defers the choice to the PR, in the same spirit as ADR 0035 left the choice
  of DCP parser open — an accepted risk, not a dodged one.
