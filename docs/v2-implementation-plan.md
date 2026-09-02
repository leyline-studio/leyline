# V2 implementation sequencing — a recommendation

**Document:** `docs/v2-implementation-plan.md`
**Version:** 0.1
**Status:** Recommendation (a planning input, not a decision) — **sequencing carried out**

---

## The state of this document

> This document recommended a build order for the seven items of [`v2-scope.md`](v2-scope.md). **That order was followed and the work is done**, item 7 included since [ADR 0051](adr/0051-watermark-rasterization-and-soft-proof-surface.md). It now reads as a planning archive, useful for understanding the trade-offs taken, and not as a task list. The actual state is in [`specification.md`](specification.md) and [`roadmap.md`](roadmap.md).
>
> One reading caveat: this document reasons on the guarantee laid down by [ADR 0028](adr/0028-process-version-per-feature.md) (one process version per feature, duplication by module). [ADR 0042](adr/0042-versioned-stage-pipeline.md) has since **replaced** ADR 0028. The underlying guarantee is unchanged — a frozen render stays frozen — but its implementation is not.

---

# 1. Subject

The whole V2 architecture is settled: `docs/v2-scope.md` scoped seven gaps
(§2 to §8) and ADRs **0026 to 0037** (twelve in all) fixed the design of
each item. **There is nothing left to design.**

This document is **not** an ADR (no decision to record here) and not a
schedule. It is an **analysis of dependencies, effort and risk** meant to feed
the implementation plan **the user** will establish. Every statement of order
below is a **recommendation**, never a "decided": the actual plan — what to
build first, in what order — belongs to the user, and this document is an
**input** to it, not a substitute.

Vocabulary: the items keep their numbers from `docs/v2-scope.md` (2 =
local adjustments, 3 = curve, 4 = colour grading/HSL, 5 = spot removal,
6 = dehaze/texture/clarity, 7 = proofing/watermark/printing, 8 = DCP profiles).

---

# 2. Summary of dependencies

The central guarantee comes from [ADR 0028](adr/0028-process-version-per-feature.md):
**one process per pixel feature**, each in its own frozen
`processN.rs` module, with no code shared between frozen modules. A direct
consequence: no V2 pixel feature has a **hard technical** delivery-order
dependency on another. Each inserts its stage at a fixed, distinct position in
the pipeline order, with no reconciliation.

Plainly:

* **Items 3 (curve, [ADR 0030](adr/0030-tone-curve.md)), 5 (spots,
  [ADR 0032](adr/0032-spot-removal-clone.md)), 8 (DCP,
  [ADR 0035](adr/0035-camera-profile-dcp.md)/[0037](adr/0037-dcp-parsing-dependency.md))
  and the _global_ forms of 4 (HSL/grading,
  [ADR 0031](adr/0031-hsl-color-grading.md)) and 6 (dehaze/texture/clarity,
  [ADR 0033](adr/0033-clarity-texture-dehaze.md)) have no hard dependency
  on each other** — they can ship in any order.
* **The only real dependency** concerns the **regional / masked variants**
  of 4 and 6 (and any future masked extension of 5): they are **deferred to
  a future ADR resting on the masking infrastructure of [ADR 0029](adr/0029-process-6-local-adjustments.md)**
  (item 2). Their **global** version, on the other hand, does **not** depend on masking at all.

Verified directly in the ADRs' text:

> [ADR 0031](adr/0031-hsl-color-grading.md) §"Regional colour grading — out of
> V2 scope": "Applying HSL or colour grading **under a mask** (combining them
> with the spatial infrastructure of ADR 0029) is **explicitly out of
> V2 scope** […] deferred to a future ADR — not designed here." The global
> version (by tonal zone) is "self-contained and deliverable without masking".

> [ADR 0033](adr/0033-clarity-texture-dehaze.md) §"Global only in V2 — the
> masked version is deferred": "The three sliders are delivered **globally**. The
> **masked or regional dehaze/clarity/texture** […] is **explicitly out of
> V2 scope** — the same one-line cut as ADR 0031 […] deferred to a
> future ADR resting on ADR 0029." The global sliders are "deliverable **without
> waiting for** item 2".

**The colour primitive of [ADR 0027](adr/0027-color-management-beyond-srgb.md)
is a shared foundation, not a feature.** ADR 0027 moves
`leyline-color` from "exposing a static profile" to "**loading arbitrary ICC
profiles and building `cmsTransform`s between them**" — a small transform API
(loading / building / applying). That primitive has no process version of its
own; it is a foundation piece. Three surfaces consume it:

* **soft proofing** and **non-sRGB export / watermark**
  ([ADR 0034](adr/0034-softproofing-watermark-print.md)) — the same output ICC
  transform;
* the **print module** ([ADR 0036](adr/0036-print-module.md)) — it "will be able
  to rest on the same output transform primitive rather than inventing a third"
  (ADR 0027's Consequences, taken word for word by ADR 0036).

**A verified nuance (item 8):** DCP ([ADR 0035](adr/0035-camera-profile-dcp.md))
**extends the same `leyline-color` crate** that ADR 0027 turned into a general
colour library — but it **applies its matrices/LUTs directly, _not_ through
`cmsTransform`** ("DCP is not ICC"). It therefore shares the **home**
(`leyline-color`) and benefits from the maturity the ICC primitive brings there,
without literally consuming the ICC transform itself. The ICC primitive proper
is consumed by item 7 (its three sub-pieces); DCP lives alongside it in the same
crate as a second colour path (ADR 0035's Consequences:
"`leyline-color` becomes the home of two colour paths").

Building that foundation **once** is therefore cheaper than letting each
feature reinvent a partial version of it.

---

# 3. Suggested tiers

Grouped by **effort + risk + dependency** — these are **waves**, not
a schedule: no dates, no notion of "week" or "sprint"
(that cadence exists nowhere in the project's documents). The order **between**
tiers is a recommendation, not a technical constraint (§2).

## Tier A — self-contained, low risk, no dependency

Good candidates for an early or parallel start: self-contained, complexity
S/M, nothing to unblock first.

| Item | ADR | Complexity | Note |
|---|---|---|---|
| Tone curve | [0030](adr/0030-tone-curve.md) | S/M | A point curve only, a frozen monotone cubic spline, luminance only, LUT precomputation. |
| Spot removal | [0032](adr/0032-spot-removal-clone.md) | M | Cloning only (heal cut), a deterministic bilinear copy, a stage early in the pipeline. |

## Tier B — a shared foundation, cheap, unblocks three surfaces

The ICC extension of `leyline-color` from [ADR 0027](adr/0027-color-management-beyond-srgb.md):
small, with no process version, and the thing proofing/watermark
([0034](adr/0034-softproofing-watermark-print.md)), printing
([0036](adr/0036-print-module.md)) and, in the same crate, DCP
([0035](adr/0035-camera-profile-dcp.md)) all rest on. To be laid **once** rather
than letting each feature invent a partial version of it. Laying it early
de-risks all of Tier D on the colour side.

## Tier C — self-contained, medium-sized features

Global only, with no dependency, but heavier than Tier A.

| Item | ADR | Complexity | Note |
|---|---|---|---|
| Colour grading / HSL (global) | [0031](adr/0031-hsl-color-grading.md) | M | HSL derived from RGB (8 bands + falloff), zones weighted by luminance. Regional deferred. |
| Clarity / texture / dehaze (global) | [0033](adr/0033-clarity-texture-dehaze.md) | M to L | A unified local contrast at two radii; dehaze by dark channel prior in closed form. Masked deferred. |

## Tier D — the big infrastructure bet

| Item | ADR | Complexity | Note |
|---|---|---|---|
| Masked local adjustments | [0029](adr/0029-process-6-local-adjustments.md) | **XL** | The largest outlay of the whole set. |

This is the item **where a bad estimate has the widest knock-on effect**: three
other features — regional colour grading (4), masked dehaze/texture/clarity (6)
and a possible masked extension of spot removal (5) —
are **behind it**, **not yet designed** (each would require its own future
ADR). Underestimating 0029 means delaying everything that could later rest on it.
To be treated as effort risk number one.

## Tier E — items with non-engineering risk (not merely effort)

These call for a **small research / validation effort _before_**
committing to a full implementation — distinct from "it is just engineering
time". The ADR says so itself in each case:

* **DCP camera profiles** ([0035](adr/0035-camera-profile-dcp.md) /
  [0037](adr/0037-dcp-parsing-dependency.md)) — **colorimetric correctness**
  "must be **validated against real Adobe-generated DCP files and their
  reference renders before any release**" (the bar of [ADR 0016](adr/0016-process-3-lens-correction.md)).
  Parsing the container is, for its part, resolved and low risk (a minimal
  in-house reader on top of the already linked `tiff` crate, [ADR 0037](adr/0037-dcp-parsing-dependency.md)) —
  but the item **cannot ship responsibly without Adobe reference material in
  hand**, not merely engineering time.
* **The print module** ([0036](adr/0036-print-module.md)) — the **OS hand-off
  mechanism** (portable PDF vs. raster + platform API vs. a Slint surface) is
  **explicitly left to the PR**: an **unresolved feasibility question**,
  to be investigated/prototyped before an effort estimate means anything. The
  rendering (`paper × DPI` sizing + a destination profile, reusing export
  and the ICC primitive of ADR 0027) is, for its part, scoped.

Recommendation: for each, a **small research/validation pass** (obtain
the reference Adobe DCP files; prototype the hand-off path) before committing to
the full implementation.

## Tier F — deliberately not designed (accepted cuts)

To be **listed explicitly** so that they are not silently forgotten when
planning — but they are **not** part of the current scope. Each will come back
in its own future ADR if it is ever wanted:

| Cut | Source |
|---|---|
| Regional / masked variants of colour grading | [ADR 0031](adr/0031-hsl-color-grading.md) (resting on 0029) |
| Regional / masked variants of dehaze/texture/clarity | [ADR 0033](adr/0033-clarity-texture-dehaze.md) (resting on 0029) |
| Seamless _heal_ (spot removal) | [ADR 0032](adr/0032-spot-removal-clone.md) |
| Per-channel RGB curves + a parametric curve UI | [ADR 0030](adr/0030-tone-curve.md) |
| Image / logo watermark | [ADR 0034](adr/0034-softproofing-watermark-print.md) |
| Contact sheets / N-up layouts (printing) | [ADR 0036](adr/0036-print-module.md), reopened and delivered by [ADR 0110](adr/0110-contact-sheets.md) |
| An embedded DCP profile database | [ADR 0035](adr/0035-camera-profile-dcp.md) |

---

# 4. Caveat

This tiering is informed by **effort, risk and dependency** — it says
**nothing about value**. It expresses no judgement about which features
matter most to real users; that trade-off belongs to the user alone. The
present document is an **input** to the implementation plan,
not a substitute for that priority decision.
