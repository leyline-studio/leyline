# ADR 0103 — Red-eye correction, without a model

**Status:** Accepted — 2026-08

## Context

Lightroom has a red-eye tool: click an eye, it finds the pupil and kills
the cast. It is the last Develop gap the parity survey left open that is
neither refused nor deferred.

Two things make it easy to get wrong. Lightroom's version *detects* the
pupil, and detection is the part this project does not do (ADR 0073 put
detectors behind a socket, and a red-eye detector would be a very small
model for a very narrow case). And the obvious cheap implementation —
darken everything inside the circle — is worse than nothing: placed a few
pixels off, it paints a grey disk on an eyelid, and the user cannot see
why.

## Decision

### 1. A new stage, not a version of `spot_removal`

Red-eye is a cousin of spot removal in *geometry* — a feathered disk
placed by hand, in the post-rotation referential of ADR 0026 — and
nothing else. Spot removal **clones**: it copies pixels from elsewhere.
Red-eye **corrects in place**: it desaturates and darkens what is already
there. Folding the second into the first would mean one list whose
entries mean two different things, decided by a mode field.

So: a `red_eye` stage at **rank 35**, its own list, its own predicate.
Placed right after `spot_removal` (30) and before every tonal stage,
because a red pupil is a defect of the capture — correcting it after the
tone sliders have stretched it means correcting a different red.

A *new stage* is backward-compatible **by construction**, and this is
worth naming because it is why no capability rule appears here: `pin`
records only **active** stages (ADR 0088), so every revision written
before this one has no `red_eye` entry, an empty list, and renders
exactly as it did. There is no setting an older pinned version cannot
express, because there is no older version.

### 2. The correction is a red-dominance test, not a disk

Inside the disk, a pixel is corrected according to **how red it actually
is**: the excess of red over the larger of green and blue, relative to
red. That measure passes through a smoothed threshold — below a floor,
nothing is touched at all; above a ceiling, the correction is full.

The threshold is not decoration. Scaling the correction by the measure
*directly* leaves a saturated pupil visibly red, because nine tenths of
the way out of a cast is still a cast; a hard cut-off instead draws a
visible contour through the gradient at an eye's rim. A neutral eyelash
or a skin tone falls under the floor and is left exactly as it was.

That is what makes an imprecisely placed circle harmless, and it is the
whole reason the tool is usable without detection. The circle says
*where to look*; the test says *what to fix*.

The correction itself: the red channel drops to the green/blue level
(killing the cast), and the pixel is darkened by `darken` — a pupil is
not merely grey, it is dark. Both happen in the working buffer's own
linear light, where a channel ratio and a multiplication are what they
say they are.

### 3. What it does not do

No detection, no snapping, no "find the other eye". The photographer
places a circle over a pupil, as they place a spot. If a red-eye detector
ever ships, it ships as a detector (ADR 0073's socket) proposing a
placement — and it will propose entries in *this* stage's list, which is
another reason for the stage to exist on its own.

## Consequences

* One new settings field (`red_eye`), one new stage module, one new
  golden case. Nothing existing moves, and the manifest shows the
  difference plainly: blessing added **exactly one** entry, where a new
  stage *version* added three or four in ADR 0096 and ADR 0098 (every
  case whose pinned map gained the new version). A new stage is invisible
  to old revisions; a new version is not.
* Delivered on all three clients: a tool in Studio, a `red-eye` parameter
  in the CLI, the type through the SDK.

## Rejected

* **A mode field on `SpotRemoval`** — §1: one struct meaning two
  operations.
* **Darkening the whole disk** — the cheap version, and the reason
  people distrust red-eye tools.
* **Detection** — §3, and ADR 0073 already decided where detection
  lives. A tool that needs a model to be usable is a tool this
  application cannot ship.
* **Correcting on the display axis** — the cast is a channel ratio; the
  axis where a ratio means what it says is the linear one the buffer
  already is (ADR 0044).
