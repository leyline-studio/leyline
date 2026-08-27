# ADR 0037 — The DCP parsing dependency: a minimal in-house tag reader on top of the already-linked `tiff` crate, no new dependency

**Status:** Accepted — 2026-07

## Context

ADR 0035 settled the pipeline placement, the owning crate, the source of the
profiles and the reproducibility of the DCP camera profile — but left one
question **explicitly open**, deferred "to the implementation PR": the **DCP
parsing dependency**. It stated that Leyline would write "a **minimal in-house
DCP parser** (only the tags needed by the application)" or "**integrate an
existing Rust crate** (should a suitable and licensable one exist when the
time comes)", without settling it, "as that depends on what is available and
licensable at that point".

This document resolves that single point left blank by ADR 0035. It reopens
**nothing** else: not the `leyline-color` placement (ADR 0035), not the source
of the profiles, not reproducibility by BLAKE3 checksum, not the process
version. It fills in a blank; it does not relitigate the previous ADR.

Facts verified in the repository before deciding:

* The **`tiff` crate is already a workspace dependency**, pinned `tiff =
  "0.10"` (`Cargo.toml`, resolved to `0.10.3` in `Cargo.lock`), consumed by
  `leyline-export` (`crates/leyline-export/Cargo.toml: tiff.workspace =
  true`). It is used today for TIFF **writing**, with ICC profile embedding
  (`crates/leyline-export/src/lib.rs`: `tiff::encoder::TiffEncoder`,
  `tiff::tags::Tag::IccProfile`) — the adoption decided by ADR 0015. Its
  licence has therefore **already cleared the project's bar** when it was
  adopted for export.

## Decision

### Write a minimal in-house DCP tag reader on top of the already-linked `tiff` crate

Leyline **writes a minimal in-house DCP tag reader**, leaning on the IFD/tag
reading primitives of the **already-linked** `tiff` crate — rather than
depending on a new, unvetted DCP-specific external crate, and rather than
writing a TIFF/IFD reader from scratch. Three reasons:

**1. DCP is a container founded on TIFF/EP tags — like DNG itself.** ADR 0035
already established it: "DCP is Adobe's format, founded on **TIFF/EP tags**,
**not** ICC". Leyline **already** links `tiff` for TIFF **writing** (ICC
embedding, ADR 0015). Reusing its IFD/tag reading primitives for a DCP
**reading** path is a **natural extension of an already vetted and already
linked dependency**, not a novel dependency surface. It is the same container,
read instead of written.

**2. The set of tags genuinely needed is small and entirely documented.** DCP
rendering needs only a bounded subset of tag families, all published in
Adobe's **DNG Specification** — a document **publicly available and freely
distributed**, not a reverse-engineered or undocumented format:

* the **colour matrices** and **calibration matrices**,
* the **calibration illuminants** (the two reference illuminants),
* the **forward matrices** (the connection space),
* the **profile tone curve**,
* the **hue/sat map** tables (the profile's 3D HSV tables),
* the **look table** (the profile's aesthetic rendering table).

**Delivery status (process 11, 2026-07-25).** Only the first three families
are **actually read**: `ColorMatrix1/2`, `ForwardMatrix1/2`, the two
calibration illuminants, plus the profile's name. The **tone curve**, the
**hue/sat map** and the **look table** are **neither parsed nor applied** —
`DcpProfile` to this day carries only the name and the resolved
camera→XYZ(D50) matrix. That is not an abandonment of scope: the list above
remains this ADR's target, and a profile whose rendering rests mostly on those
tables will give a result different from Adobe's while they are missing. The
distinction is flagged here rather than left implicit, so that no reader infers
from this ADR a completeness the code does not yet have.

*(The tag families are named at the level of detail the DNG Specification
guarantees; each tag's exact numeric identifier is left to the PR, which will
read them from the spec rather than invent them here — the same caution
against inventing constants as the rest of this series of ADRs.)*

That is a situation **materially different** from an undocumented proprietary
format: writing a minimal reader against a published spec is a **bounded and
well-framed** task, not an open-ended reverse-engineering effort.

**3. The dependency and licence story stays clean.** No new external crate to
vet: ADR 0009's dependency licence constraint (code under GPL-3.0,
dependencies compatible) is not re-invoked, since `tiff`'s licence **already
cleared the bar** when it was adopted for export (ADR 0015). Reusing an
already-vetted crate avoids introducing — and having to re-vet — a third-party
DCP-specific dependency.

### The parser's scope — read-only, the small set of rendering tags

The parser is **read-only** and limited to the small set of tags needed for
rendering (above). **No DCP *authoring*/writing support**: V2 only **reads**
`.dcp` files supplied by the user (ADR 0035), it never writes one. That narrow
scope is stated explicitly: it **keeps the parser small** and **prevents any
scope creep** towards a general DCP toolkit (editing, re-encoding,
conversion) that nothing in V2 asks for.

### Colorimetric correctness is NOT resolved by this ADR

This ADR resolves **how bytes become structured data** — the container. It
**does not resolve** whether the matrices and tables thus read are **applied
correctly** in the colorimetric sense. Those are **two distinct risks**:
parsing the container correctly, and applying its colour science correctly.

ADR 0035's validation requirement **stays unchanged and is in no way
weakened**: the DCP application path must be **validated against real
Adobe-generated `.dcp` files and their reference renderings before any
release** — the same bar as ADR 0016 ("not trivial to validate without
reference images at hand"). This ADR touches **only** the first risk (the
container's parsing, documented and low-risk); the second (the correctness of
the colour mathematics against Adobe's rendering) remains exactly the risk ADR
0035 named, and this ADR does not claim to close it.

### Placement — unchanged, in `leyline-color`

The parser and its tag-interpretation logic live in **`leyline-color`**,
consistent with the placement decided by ADR 0035 (the DCP parser and the
application of its matrices and tables already live there). This ADR does not
reopen that choice; it fills in the single detail ADR 0035 had left blank.

## Consequences

* **The dependency question ADR 0035 opened is closed**: a minimal in-house
  tag reader on top of `tiff` (already linked, already vetted), no new
  DCP-specific crate, and no TIFF/IFD reader from scratch.
* **Zero new dependency to vet**: ADR 0009's licence constraint is not
  re-invoked, `tiff`'s licence having already cleared the bar for export (ADR
  0015). The dependency story stays clean.
* **The scope stays small and frozen**: read-only, a subset of rendering tags,
  no authoring. No creep towards a general DCP toolkit.
* **The parser shipped is a subset of that scope**: matrices, illuminants and
  the profile's name only; the tone curve, the hue/sat map and the look table
  remain to be done (see "Delivery status" above). Documenting them as read
  when they are not would make the implementation look more complete than it
  is.
* **Colorimetric correctness stays an open risk, unchanged since ADR 0035**:
  this ADR resolves the container's parsing (documented, bounded, low-risk),
  **not** the colour mathematics' fidelity to Adobe's rendering. Validation
  against real Adobe DCPs and their reference renderings before release (ADR
  0016's bar) remains required as it stands.
* **`leyline-color` stays the home** of DCP parsing and application (ADR
  0035), alongside the output ICC transform path (ADR 0027) — this document
  moves nothing.
* **It prejudges no future plugin system.** `docs/roadmap.md` lists "Plugins,
  a stable SDK" under Long term, outside V2. The in-house DCP reader decided
  here is an internal implementation choice (where the code lives, today) — it
  does not close the door on a future extension mechanism (a plugin supplying
  another profile parser, an additional format) that would be layered on top
  the day `docs/roadmap.md` takes up that work. Nothing here commits to the
  shape of that future system; it simply is not what this ADR closes.

## Alternatives rejected

* **Depending on a DCP-specific external crate.** Rejected: as of today, no
  established DCP-specific crate is known with the maturity and adoption level
  of the project's other dependencies (LibRaw, Lensfun, LittleCMS through
  `lcms2`, `tiff`) — without claiming to have conducted an exhaustive survey of
  the ecosystem, no candidate of that calibre stands out. Introducing such a
  crate would impose a new licence check (ADR 0009) and a new, untried
  dependency surface, in order to parse a container whose useful subset is
  small and documented — a cost disproportionate to the gain. Reusing `tiff`,
  already linked and already vetted, avoids both.
* **Writing a TIFF/IFD reader from scratch** rather than reusing the
  already-linked `tiff` crate. Rejected: it would rewrite exactly the IFD/tag
  reading primitives `tiff` already provides and that `leyline-export` already
  uses for TIFF writing (ADR 0015). That is redundant work on a component
  (IFD/tag parsing) where a vetted dependency already exists in the tree — no
  reason to redo it by hand. The in-house DCP reader is limited to the **DCP
  tag interpretation layer**, on top of the IFD reading `tiff` already
  carries.
* **Treating DCP as an entirely undocumented format demanding
  reverse-engineering caution equivalent to ADR 0016's deferral of vignetting
  and TCA.** Rejected — and the distinction must be made explicitly: DCP's
  **container format** is **documented** (TIFF/EP tags, Adobe's published DNG
  Specification) and **low-risk to parse**; it is not a reverse-engineered
  format. Only the **colorimetric correctness of its application** carries
  ADR 0016-level risk — and that risk is **not** what this ADR resolves: it
  stays open and validated against real Adobe renderings, exactly as ADR 0035
  requires. Conflating the two would lead to over-framing the parsing
  (treating a bounded task as an open-ended effort) while underestimating that
  the colour mathematics still has to be validated separately. The two risks
  are distinct; this ADR closes only one of them.
