# ADR 0173 — A backdrop's subjects are detected before the backdrop

**Status:** Accepted — 2026-11

## Context

[ADR 0172](0172-extending-a-backdrop.md) §6 recorded that `_MG_0025`, one of
its four acceptance frames, failed "not because of the stage: the detector
protects the left dancer and only the legs of the right one". Replaying the
frame with a far stronger detector gave the same amputated dancer, which
ruled the detector out. Two defects of the *sequence* were found instead,
each reproduced through the CLI:

1. **The detector is shown the fill.** `detect … --backdrop` hands the
   detector the developed preview, as [ADR 0073](0073-external-mask-detectors.md)
   §2 specifies. Once a polygon is traced, that preview already contains the
   extension: whatever stands outside the cloth — a head, a raised arm — has
   been replaced by cloth *before* the detector looks. It cannot protect
   what it is not shown.
2. **Detecting first loses the answer, silently.** Run before any polygon,
   the detection stores its coverage, the CLI prints "committed", and the
   protection is gone: `Settings` serialises `backdrop` only when
   `Backdrop::is_neutral` is false, and a backdrop without a region is
   neutral. The one order that would have worked writes nothing.

Together they make the tool unusable for the case it exists for: a subject
overlapping the edge of the backdrop.

## Decision

### 1. The detector is shown the photograph without its backdrop

A detection made *for the backdrop* runs on a **view**: the head revision's
settings with `backdrop` at its default **and without the crop**, rendered
in memory and written nowhere (the rule of ADR 0074 and 0163).

* Without `backdrop`, because that stage is what the answer will feed.
  Every other setting stays: a detector sees the tones and colours the
  photographer chose, as ADR 0073 intends.
* Without the crop, because a coverage lives on the post-rotation,
  **pre-crop** canvas (ADR 0070 §3, ADR 0026), like the backdrop's
  polygons. A cropped image would be laid over the wrong pixels.

The engine owns this view, `Library::preview_before_backdrop`, so that two
clients cannot build it differently; `leyline-detect` gains
`detect_coverage_of`, which takes pixels instead of a file and keeps the
temporary-file plumbing where ADR 0105 §1 put it.

### 2. A protection is a decision, stored whether or not a region exists

`backdrop` is serialised unless it equals its **default**, the distinction
`ParametricCurve::is_default` already draws for a moved split that changes
no pixel yet. `is_neutral` keeps its meaning and its one remaining use:
whether the stage runs. A revision holding only protections renders exactly
as before (the stage is a no-op without a region) and is no longer thrown
away.

Both orders now work: polygon then detection (§1), detection then polygon
(§2).

## Consequences

* No stage version, no golden change: §1 changes what a detector is shown,
  §2 what a revision stores; no pixel of any existing revision moves.
* The CLI's `detect … --backdrop` and Studio's future *Protect the subject*
  both go through §1. ADR 0172 §5's sync rule ("Protect the subject on N
  frames") inherits it.
* A detection for a **local adjustment** still receives the developed,
  cropped preview (ADR 0073 §2). On a cropped photograph its coverage is
  therefore read on a canvas it was not computed on. That is outside this
  decision and is left open here on purpose: it deserves its own
  measurement before its own ADR.

## Rejected

* **Detecting on the unedited asset preview**: simpler, and wrong for a
  detector, which ADR 0073 feeds the developed image so that an
  underexposed RAW is not judged in the dark.
* **Refusing a detection before a polygon** instead of storing it: it
  would make the silence loud, and keep forbidding the order that needs no
  special view at all.
* **Excluding `backdrop` from every preview handed to a detector**: a
  local adjustment's detector is entitled to the picture as it will be
  printed, extension included.
