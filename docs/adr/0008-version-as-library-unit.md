# ADR 0008 — The version as the library's unit

**Status:** Accepted — 2026-07

## Context

Under the Git model (ADR 0007), a virtual copy is a branch. What remained to decide was where rating, label, pick, collections and keywords live: on the file (the asset) or on the branch (the version)?

## Decision

The dividing line:

```text
A fact about the image     → asset      (keywords, EXIF, checksum)
A judgement on a rendering → version    (rating, label, pick, collections)
```

The grid enumerates versions. Rating an ordinary photo means rating its `Default` version.

## Consequences

* Every virtual copy is rated, labelled and classified independently (parity with Lightroom).
* Keywords stay shared: "find my herons" brings back every version, and the XMP export stays attached to the file.
* The grid costs one indexed 1:1 join (`develop_versions JOIN assets`) — measured as negligible at the target scale.
* An extension held in reserve should the need arise: an additive `version_keywords` (`catalog.md` §38).

## Alternatives rejected

* **Everything on the asset**: virtual copies share the rating — a real limitation, observed in culling workflows.
* **Everything on the version, keywords included**: tagging duplicated on every branch, silent divergences, ambiguous XMP.
* **Nullable overrides (asset → version inheritance)**: two sources of truth and a COALESCE in every query — permanent debt.
