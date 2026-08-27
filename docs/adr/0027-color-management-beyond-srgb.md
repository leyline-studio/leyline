# ADR 0027 — Widening colour management beyond sRGB: an output profile decoupled from rendering

**Status:** Accepted — 2026-07
**Note:** this ADR stays valid for everything concerning **output**. Its
premise that "the internal working space does not change" no longer holds
since [ADR 0044](0044-linear-wide-gamut-working-space.md), which moves the
pipeline's buffer to unbounded linear Rec. 2020; the transforms described
here then apply after the `output_rendering` stage.

## Context

ADR 0015 freezes V1 on a single space, from decode to export: sRGB. It
explicitly documents that as a provisional decision, not a closure: "A
future, wider working space (ProPhoto or Adobe RGB internally) would remain a
structural change in its own right — this ADR neither prepares it nor rules
it out."

`docs/v2-scope.md` §7 (soft proofing, watermark, print module) and §8 (DCP
camera profiles) both run into that freeze, identified as §9's second
cross-cutting lock. Rather than let every future feature ADR re-derive
independently how far colour management extends, this document settles once
where the pipeline stops and where flexibility begins.

`leyline-color` today (ADR 0015) exposes a single function:
`srgb_icc_profile()`, a canonical ICC profile generated once through
`lcms2::Profile::new_srgb`. `leyline-export` embeds it as it is in
JPEG/PNG/TIFF (`crates/leyline-export/src/lib.rs`). No `cmsTransform` exists
in the code yet.

## Decision

**The render pipeline's internal working space does not change.** The
sRGB gamma-encoded buffer between operators, and the internal linear light
for white balance and exposure (an invariant documented in `process3.rs`),
stay as they are: this decision **reopens no process version**. What widens
sits strictly at the **output**, carried entirely by
`leyline-color`/`leyline-export`, decoupled from `leyline-engine`:

1. **Export to a destination ICC profile.** `leyline-export` produces sRGB
   pixels as it does today; when a non-default output profile is asked for
   (Adobe RGB, ProPhoto, a printer profile…), `leyline-color` applies an
   sRGB → destination `cmsTransform` as the very last step before encoding —
   a pixel transform at export, never persisted in `settings_json`, never a
   process version, in the same way that format and quality are already not
   process concerns (ADR 0025, `ExportRequest`).
2. **Soft proofing, view only.** The preview renders normally (sRGB,
   unchanged), then `leyline-color` applies a proofing transform
   (destination profile + rendering intent + an optional gamut warning) for
   display alone — never written to the catalog, in keeping with
   `docs/v2-scope.md` §7's observation: proofing touches no persistent state.
3. **`leyline-color` moves from "exposing a static profile" (ADR 0015) to
   "loading arbitrary ICC profiles and building `cmsTransform`s between
   them"** — an extension of the existing `lcms2` dependency, not a new
   dependency.
4. **Authoring DCP camera profiles (`docs/v2-scope.md` §8) is explicitly
   outside this decision.** A DCP acts on the sensor → working-space
   conversion, at the **start** of the pipeline (a genuine process-version
   event), whereas this decision widens only the **output**. Item 8 keeps its
   own ADR when the time comes, but can now assume that `leyline-color` will
   already be a general ICC transform library by then, not a mere emitter of
   a static profile.

## Consequences

* ADR 0015 stays valid for the pipeline's internal working space — nothing
  here reopens `process1`..`process5`.
* `leyline-color`'s public surface goes from one function to a small
  transform API (loading a profile, building a transform, applying it),
  tested with the same deterministic rigour as `srgb_icc_profile()` today.
* Soft proofing and non-sRGB export share the same underlying primitive (an
  ICC transform): building one substantially de-risks the other, even though
  `docs/v2-scope.md` treats them as two separate features.
* The print module (`docs/v2-scope.md` §7) will be able to lean on that same
  output-transform primitive rather than inventing a third.
* The reproducibility contract (`docs/pipeline.md` §5) is not engaged for
  develop revisions: the transform lives entirely outside `settings_json` and
  outside `process` — it therefore cannot break "the same revision → the same
  pixels forever". It affects only the export encoding and the view-only
  preview, two surfaces already outside that contract's scope.

## Alternatives rejected

* **Widening the internal working space (Adobe RGB/ProPhoto) directly in the
  render pipeline**: it would impose a new process version (every operator
  assumes sRGB and its transfer function, an invariant documented in
  `process3.rs`), plus a reprocessing and performance cost on every asset,
  for a benefit — wide-gamut editing headroom — that neither V1 nor the items
  of §7/§8 ask for. Rejected, as disproportionate to the real gap (items 7
  and 8 need flexibility at the **output**, not a wider working capacity).
* **Treating proofing and the export profile as two separate, unrelated
  pieces of work**: it would have duplicated the ICC transform plumbing in
  `leyline-color`/`leyline-export` twice — rejected in favour of a common
  primitive.
* **Deferring this decision and letting the future ADRs of items 7 and 8 each
  choose their own approach**: that was the status quo (ADR 0015 left it
  explicitly open), but `docs/v2-scope.md` shows the two items converging
  independently on the same prerequisite — settling it once now avoids two
  ADRs re-deriving the same answer.
