# ADR 0084 — Assisted culling: the assistant types, it does not develop

**Status:** Accepted — 2026-08 (the shape; the model is deferred, as in
[ADR 0073](0073-external-mask-detectors.md))

## Context

The request is explicit: something of the shape of Aftershoot — a local model
that goes through a shoot and says which frames are keepers. It is worth being
precise about what that product actually does, because the four things it sells
under one name do not have the same cost, the same risk, or the same place in
this architecture:

1. **technical rejects** — motion blur, missed focus, blown or crushed frames;
2. **faces** — closed eyes, a turned head, the sharper of two portraits;
3. **near-duplicates** — a burst of eleven frames of the same scene, of which
   one is kept;
4. **an editing style learned from the photographer's past edits.**

The corpus this project is tested against says why the request is the right one.
Enumerated on 2026-08-28: **52,099 image files**, of which ~15,000 CR2 across
two bodies, spread over fourteen years and 25,346 distinct filename stems.
Culling is not a nice-to-have on a library that size — it is the only part of
the workflow whose cost grows linearly with the shutter count and which no
amount of engine performance reduces. Everything this project has optimised so
far ([ADR 0081](0081-grid-page-cost.md)–[0083](0083-scaled-jpeg-thumbnail-decode.md))
makes the photos *appear* faster. None of it makes the choosing faster.

### Why this feature and not AI denoising

[ADR 0073](0073-external-mask-detectors.md) §7 records that C1 — AI denoising —
has no way out, and states the reason in one line: **a denoiser produces
pixels.** It can therefore neither be materialised once like a mask, nor cross
[ADR 0069](0069-closed-extension-boundary.md)'s boundary, nor enter the render
path without dragging the reproducibility promise ([`pipeline.md`](../pipeline.md)
§5.1) with it.

Culling is the exact opposite, and that is the whole reason it fits. **It
produces no pixels.** It produces a rating, a flag, a colour label — state that
[`catalog.md`](../catalog.md) §18 already carries on the develop version, that
the grid already filters on, that the history already undoes. An assistant that
culls proposes *the keystrokes the photographer would have typed*. There is no
new kind of state to invent, no stage to version, no promise to renegotiate.

That is a stronger claim than "it fits behind the extension boundary". It fits
behind it **without the boundary having to do any work**: ADR 0069's rule is
that an extension produces settings and never pixels, and classification is the
one output that could not be pixels even if the extension tried.

## Decision

### 1. The assistant produces classification, and nothing else

The output of a culling run, per photo, is a subset of what a human can already
express: `rating`, `pick`/`reject`, `color_label`, and a free-text reason for
display. It is written through an **ordinary edit session**, the same call the
CLI's `leyline rate` makes.

Consequences of that sentence, all of them deliberate:

* **no schema change.** No migration, no column, no table. A verdict that
  needed one would be a verdict this application does not already know how to
  show, filter and undo;
* **nothing in the engine.** No stage, no stage version, no entry in
  [`pipeline.md`](../pipeline.md) §3.3. `§5.1` is not implicated, in the strong
  sense: not "we checked and it still holds", but "no pixel-producing code was
  added";
* **undo works because it is the same undo.** Accepting a run of 400 verdicts
  is 400 ordinary commits, reversible one by one or in bulk, by machinery that
  already exists and is already tested.

### 2. It proposes; the photographer applies

A run produces a **proposal**, held in memory and shown in the grid. Nothing is
written to the catalog until the photographer accepts — all of it, a filtered
subset, or one photo at a time.

This is the product rule of the ADR and it is not negotiable, for a reason that
has nothing to do with modesty about models: **a culling mistake is invisible
after the fact.** A wrongly flagged photo does not look wrong, it looks absent.
Unlike a bad denoise or a crooked horizon, the photographer has no way to
notice the error by looking at the result — the evidence of the mistake is
precisely what was removed from view. A tool that silently applies verdicts of
that kind is not offering assistance, it is offering a shoot the photographer
can no longer audit.

A proposal is therefore ephemeral by construction. Losing it costs a re-run,
which is cheap; persisting it would create a second, unversioned source of
truth about a photo's status, which is exactly the thing
[`catalog.md`](../catalog.md) §18 exists to prevent.

### 3. A set in, a set out — the socket of ADR 0073 is the wrong arity

[ADR 0073](0073-external-mask-detectors.md) established that a detector is an
**executable**, discovered through a manifest in the user's configuration,
speaking files rather than a linked API. That decision is reused wholesale: the
same reasoning (no runtime imposed, no model in this repository, no ABI to keep
stable) applies unchanged.

What is **not** reused is the arity. A mask detector maps one image to one mask.
The two judgements that matter most here are **comparative**: "the sharpest of
this burst" and "these eleven frames are the same scene" cannot be computed one
photo at a time. So the culling socket takes a **set** of images and returns a
set of verdicts plus a grouping, and it is a distinct socket rather than a
generalisation of the mask one — a detector that must see the whole shoot has
different memory behaviour, different failure modes and a different progress
story than one that sees a single frame.

### 4. Half of it needs no model at all, and that half ships first

Of the four capabilities in the Context, **one requires no weights**:
technical rejects. Focus and motion blur are a gradient-energy measurement;
clipping is a histogram; near-duplicate grouping over a burst is achievable
from capture timestamps and a perceptual hash long before an embedding is
involved.

That part is ordinary image processing, GPL-3.0-only, in the open repository,
with no licence question and no external process. It is also the part that acts
on the largest number of photos: a shoot's rejects are mostly rejects for banal
reasons.

The order is therefore: measurable classical criteria first, in the open;
faces and learned judgement second, behind the socket. This is not a staging
convenience — it decides how much of the feature exists at all if the model
half never ships, and the answer must not be "none of it".

### 5. Weights: the licence is eliminating, and it is checked before any code

[ADR 0073](0073-external-mask-detectors.md) §5 applied this criterion once and
it eliminated the two most visible candidates on the first pass — SegFormer
ADE20K (non-commercial) and RMBG-1.4 (paid commercial). The same criterion
applies here, unweakened: **a model whose weights are not under a
GPL-3.0-compatible licence does not enter**, and the check happens before a
line is written, not after a prototype works.

No candidate is named in this ADR. Naming one here would repeat the mistake
ADR 0073 §5 was written to prevent: the licence of a specific checkpoint is a
fact to verify at the moment of choosing, from the checkpoint's own
distribution, not a thing to recall. What is settled is the criterion and its
position in the order of work.

The weights are **never in the open repository** and never in the free
AppImage, exactly as ADR 0073 §6 already requires of `leyline-assist`.

### 6. Offline, always

No network call, at any point, including for a licence check.
[`vision.md`](../vision.md)'s Local First is the project's most legible promise
and the one a photographer can verify with a firewall. ADR 0069 §5 already
recorded that a paid edition would have to verify **offline**; nothing here
relaxes it.

### 7. The dependency rule this raises, stated once

The question that brought this ADR about was licensing, so the rule is written
down rather than left to be re-derived. Leyline is **GPL-3.0-only** with the
section 7 additional permission of [`LICENSE-EXCEPTION.md`](../../LICENSE-EXCEPTION.md).
For anything linked into the open binaries — crates, C libraries, embedded data,
model weights:

* **admitted:** the permissive family (MIT, Apache-2.0, BSD, ISC, Zlib, 0BSD,
  Unlicense, CC0), and the copyleft that flows into GPL-3 (LGPL-2.1, LGPL-3,
  MPL-2.0, GPL-3);
* **excluded:** AGPL — compatible in the licence-lawyer sense, but it would
  make the combined work AGPL and silently change what Leyline is;
* **excluded:** anything non-commercial, source-available, evaluation-only, or
  requiring a separate commercial agreement. This is the category that catches
  interesting models, and it catches them late if nobody looks.

Audited on 2026-08-28 over the full dependency graph, all features enabled:
**779 external packages, 43 distinct licence expressions, zero violations.**
The tree is clean today. It was clean by luck and attention rather than by
enforcement, and §8 fixes that.

### 8. The rule is enforced by a test, not by a reviewer's memory

An allow-list check over `cargo metadata`, run by `make check` like everything
else. The precedent is `every_cascading_foreign_key_is_indexed`
([ADR 0083](0083-scaled-jpeg-thumbnail-decode.md)'s predecessor work): the
audit that asks the artefact itself found half of what a careful reader had
missed by hand. A licence policy that lives only in an ADR is a policy that
holds until the first hurried dependency bump.

## Consequences

* **The feature degrades honestly.** Without any model, the technical-reject
  half works, in the open repository, under GPL-3.0-only. The socket adds
  faces and comparative judgement; its absence removes capability, not
  correctness.
* **Nothing in the render path changes**, and this ADR adds no stage and no
  stage version. `pipeline.md` §5.1 is untouched by construction.
* **No migration.** The first feature in a long while to add no catalog state
  — because it deliberately reuses the classification that
  [`catalog.md`](../catalog.md) §18 already versions.
* **A new socket to specify**: set-in/set-out, with progress, cancellation and
  a memory budget over a shoot of thousands. That specification is the next
  step, and it is real work.
* **`specification.md` §4 will need correcting** the day a model ships, not
  working around — the same requirement ADR 0069 §5 set for the paid edition.
  Today's text ("no model is shipped, no inference takes place") stays exactly
  true for everything §4 above delivers.

## Alternatives rejected

**Applying verdicts automatically, with undo as the safety net.** Undo protects
against a mistake the user *notices*. §2 gives the reason this one is different:
a wrongly rejected photo is invisible precisely because it was rejected. The
safety net does not catch what nobody looks for.

**Persisting proposals in the catalog.** It would survive a restart, which is
the only argument for it. Against: a second source of truth about a photo's
status, unversioned, that every query filtering on rating would then have to
know about — and a re-run is cheap.

**Generalising ADR 0073's mask socket to cover culling.** One socket, fewer
concepts. But an executable that must hold a whole shoot in order to answer
has different memory, progress and cancellation behaviour than one answering
about a single frame, and folding them together would give the mask detectors
an interface shaped by a problem they do not have.

**Learning the photographer's editing style (capability 4).** Deliberately out
of scope here. It produces *settings*, so it would cross ADR 0069's boundary
legally — but it is a different problem (it learns from history rather than
judging an image), it needs the edit history of a library that has one, and
folding it in would make this ADR about two things. It is a candidate for a
later ADR, not an omission.

## Related

* [ADR 0069](0069-closed-extension-boundary.md) — the extension boundary, and
  why an extension produces settings and never pixels
* [ADR 0073](0073-external-mask-detectors.md) — the detector-as-executable
  pattern, and the licence criterion this ADR reuses verbatim
* [`catalog.md`](../catalog.md) §18 — where rating, pick and label live
* [`vision.md`](../vision.md) — Local First
* [`specification.md`](../specification.md) §4 — the standing AI exclusion
