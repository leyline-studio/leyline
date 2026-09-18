# ADR 0034 — Soft proofing and watermark: two output surfaces, no process; the print module stays outside this decision

**Status:** Accepted — 2026-07
**Followed by:** the third it explicitly leaves unscoped — the print module —
got its own ADR as anticipated ([ADR 0036](0036-print-module.md)); and the
actual rasterization of the watermark, which this ADR describes at surface
level without choosing a text rendering engine, is settled by
[ADR 0051](0051-watermark-rasterization-and-soft-proof-surface.md). The title
is therefore to be read as of its date: printing is no longer outside the
decision.

## Context

`docs/v2-scope.md` §7 groups three gaps under one item — soft proofing,
watermark, print module — stressing that **it is the least "pipeline" item**:
none of the three is a pixel modification of the stored revision, but preview
/ export / colour management / UI work.

The cross-cutting lock that blocked all three — V1's sRGB freeze (ADR 0015) —
is already settled by **ADR 0027**: the render pipeline stays sRGB, but
`leyline-color` moves from "exposing a static profile" to "loading arbitrary
ICC profiles and building `cmsTransform`s between them" — a small transform
API (loading a profile, building a transform, applying it). ADR 0027
explicitly notes that soft proofing and non-sRGB export **share the same
underlying primitive** and that the print module will be able to lean on it
"rather than inventing a third". This document consumes that primitive; it
does not re-derive it.

What remained to settle, for item 7, is what ADR 0027 deliberately left open:
the **concrete shape** of soft proofing and of the watermark. That is this
ADR's subject.

**An explicit framing up front — this document does not design the print
module.** Printing (layout, paper formats, margins, contact sheets, output
through a printer profile) is a frankly separate and much wider undertaking —
"mostly UI plus a dedicated output path" according to `docs/v2-scope.md` §7
itself, sized **L/XL** — whose responsible design demands its own pass the day
that work is actually planned, exactly as §7 has already signalled. This ADR
**does not stub** printing with filler decisions: it settles the two pieces
genuinely treatable today — the watermark and soft proofing's engine/API
surface — and leaves printing to a future dedicated ADR.

## Decision

**Neither the watermark nor soft proofing is a process version, and neither
touches develop `settings_json`.** `docs/v2-scope.md` §7's table already
states it for each: soft proofing is a transform "at display time" that
modifies **neither** `settings_json` **nor** the revision's pixels ("No
process/schema: a transform that is not persisted"); the watermark is a
decoration "at export time", in the same category as format and quality ("No
develop process"). Both therefore live where output configuration already
lives — at export, `ExportRecipe`/`ExportSettings` (ADR 0025,
`docs/engine-api.md` §12); at display, the preview request
(`docs/engine-api.md` §11) — never in a revision nor a `process`.

**Unlike ADR 0029–0033, this document introduces no process version and
inserts no stage into `docs/pipeline.md` §3.1's pipeline order.** That is the
direct consequence of the item's "non-pipeline" nature noted in §7: nothing
here changes the pixels of a stored revision.

### Watermark — text only in V2, image/logo is cut

The V2 watermark is **exclusively textual**: a string, its font, its size, its
colour, its opacity, and its anchor/position. Such a watermark is **entirely
self-contained** in `export_presets.settings_json` (`docs/catalog.md` §27) —
no new resource-reference problem.

The **image/logo watermark is cut from V2's scope** — a deliberate cut, not an
oversight. A logo would raise a genuine design question this ADR chooses not
to settle lightly: where does the logo file live and how is it referenced? A
path relative to the library (`docs/catalog.md` §2.3) — but the logo is not a
photo asset, and the relative-path rule does not obviously cover it? An
absolute path chosen by the user — which the library's portability (§2.3)
precisely forbids storing? A text watermark sidesteps that question entirely
(it has no external resource) and covers the use case named in typical gap
lists: the photographer's name, a copyright, a website. The logo will be added
in its own change the day that reference problem is settled.

**Placement in the export render path.** The text watermark is composited as
the **very last step of the export render**, *after* ADR 0027's optional ICC
transform to the destination profile and **immediately before encoding**
(`leyline-export::encode`, `crates/leyline-export/src/lib.rs`). The reasoning:
the watermark's text must be drawn directly in the destination RGB space
(whatever profile the export targets), **not** put back through a photographic
colour transform designed for image content. Drawing it after the profile
conversion avoids that mismatch — output decoration inherits the output space,
it does not travel through it.

**Storage — additive.** `ExportSettings` (`crates/leyline-export/src/lib.rs`,
which **doubles as** the `settings_json` of export presets, `docs/catalog.md`
§27) gains an optional `watermark` field. Absent means no watermark. No export
schema bump: it is the addition of an optional field with a neutral value,
just as `max_edge` was.

A sketch (an export recipe with a text watermark):

```json
{
    "format": "jpeg",
    "quality": 90,
    "max_edge": 2048,
    "watermark": {
        "text": "© Quentin Boulard 2026",
        "font": "sans",
        "size": 3.0,
        "color": "#FFFFFF",
        "opacity": 0.7,
        "anchor": "bottom-right"
    }
}
```

The neutral case — the field absent, the export unchanged from today:

```json
{ "format": "jpeg", "quality": 90 }
```

> **An implementation note (not a code edit here).** `ExportSettings` today
> refuses any unknown field (`#[serde(default, deny_unknown_fields)]`,
> `crates/leyline-export/src/lib.rs`), and a test uses precisely
> `"watermark": "logo.png"` as an example of an unrecognized field to reject
> (§3.4). The PR that ships the watermark makes `watermark` a **known** field
> (an object, not the test's string) and updates that test in the same change
> — in keeping with CLAUDE.md, spec and code move together, not in this
> pre-decision ADR.

### Soft proofing — an engine/API surface, view only

Soft proofing is **an extension of the preview request** (`docs/engine-api.md`
§11), not a revision and not a preset. An **optional** proofing parameter is
added to a preview call: a destination ICC profile (bytes or a reference), a
rendering intent, and an optional gamut-warning flag. That addition is **a
view option on a single preview call** — **never persisted**, never written to
any revision or preset, never in `settings_json`.

The transform itself **reuses ADR 0027's primitive** (`leyline-color`: loading
a profile, building a transform, applying it) — no new primitive, exactly the
sharing ADR 0027 anticipated between proofing and non-sRGB export. The render
pipeline's output (sRGB, `docs/adr/0015`) is **entirely unchanged**: the
proofing transform happens **strictly after** the normal render, for display
alone. The preview renders first in sRGB as it does today, then, when a
proofing parameter is supplied, `leyline-color` applies the sRGB → destination
profile transform (plus the gamut warning if asked for) to the display buffer
alone.

A conceptual sketch of the parameter (the exact shape left to the PR, as for
every ADR in this series):

```rust
struct SoftProof {
    profile: IccProfile,       // ICC bytes or a reference to a destination profile
    intent: RenderingIntent,   // perceptual / relative colorimetric / …
    gamut_warning: bool,       // highlight colours outside the destination gamut
}
// an optional parameter of a preview call — never serialized, never catalogued.
```

## Consequences

* **This ADR closes the two thirds of item 7 that are "proofing/watermark"**,
  which ADR 0027 had left open. The remaining third — the print module —
  stays **genuinely unscoped**, tracked as future work, **neither designed
  here nor stubbed** with filler decisions. That is an explicit deferral, not
  an oversight: printing will have its own ADR when the work is planned, in
  the same spirit as `docs/v2-scope.md` §7 sizing it L/XL and describing it as
  "mostly UI plus a dedicated output path".
* **No process version, no pipeline stage**: the revisions' reproducibility
  contract (`docs/pipeline.md` §5, "the same revision → the same pixels") is
  not engaged, exactly as ADR 0027 established — the watermark lives at export
  encoding, proofing in a display buffer, two surfaces already outside that
  contract's scope.
* **The watermark inherits the export-preset plumbing for free**: a field in
  `ExportSettings`, hence portable, storable and replayable like any recipe
  (`docs/catalog.md` §27), with no new table and no new mechanism.
* **Soft proofing and non-sRGB export (ADR 0027) share a single ICC
  primitive** in `leyline-color`: building one de-risks the other, as ADR 0027
  foresaw, even though `docs/v2-scope.md` lists them separately.
* **Cutting the image/logo watermark** leaves the real resource-reference
  problem open for a future ADR (a library-relative path vs. an absolute file
  chosen by the user) — without blocking the common use case text already
  covers.

## Alternatives rejected

* **Supporting an image/logo watermark from V2 on.** Rejected: it would
  require settling where the logo file lives and how it is referenced — a
  library-relative path (`docs/catalog.md` §2.3) when the logo is not a photo
  asset, or an absolute path that the portability rule (§2.3) precisely
  forbids storing. A genuine design question this ADR refuses to invent under
  pressure; the text watermark is entirely self-contained in `settings_json`
  and covers the named case (name/copyright/website). The logo will come back
  in its own change, once the reference problem is settled — not deferred as a
  flag, cut as a feature.
* **Compositing the watermark before the transform to the destination
  profile.** Rejected: the text would then be drawn in sRGB and put back
  through ADR 0027's photographic ICC transform, designed for image content
  and not for a decoration. Its colours (a white at 70 % opacity, a copyright
  tint) would drift with the destination profile. Drawing it **after** the
  conversion, directly in the destination space, guarantees that a white
  watermark stays the destination's white — the decoration inherits the output
  space, it does not travel through it.
* **Persisting the proofing state in `settings_json` or a preset** rather than
  keeping it a view-only request parameter. Rejected: proofing is a **view
  mode**, not a property of the revision (`docs/v2-scope.md` §7). Writing it
  to the catalog would contradict §7's own observation ("modifies neither
  `settings_json` nor the pixels") and would introduce state affecting no
  rendered or exported pixel — a field that lies about what a revision is.
  Keeping it on the preview call keeps it exactly where it acts: the display,
  and nothing else.
* **Scoping the print module in this same ADR.** Rejected: printing is a
  layout plus output subsystem (margins, contact sheets, printer profile),
  sized L/XL and "mostly UI plus a dedicated output path" by
  `docs/v2-scope.md` §7 — an undertaking frankly wider than the watermark and
  proofing, and of another nature (UI and output plumbing, not develop
  pipeline architecture). Designing it responsibly demands its own dedicated
  pass, the day that work is planned. Stubbing it here with filler decisions
  (paper formats, a margin model, contact-sheet handling) would be inventing
  an architecture nobody has actually framed yet — exactly what the house
  avoids ("No code before architecture"). Printing therefore keeps its own ADR
  to come; this ADR merely says so explicitly.
