# ADR 0119 — Keystone by drawn lines: the photographer says what should be straight

**Status:** Accepted — 2026-09

## Context

[ADR 0052](0052-perspective-correction.md) gave the pipeline a perspective
correction — one homography, two sliders — and refused the automatic version
in as many words:

> **Automatic correction does not enter either**: detecting vanishing lines
> requires edge detection and a Hough vote, that is, an image-analysis
> algorithm whose result depends on the content.

That refusal stands, and this ADR does not touch it. What is missing is
something else, and the difference is the whole decision: **nothing detects
anything here**. The photographer draws a line along an edge that ought to be
vertical, and another, and the tool answers with the two slider values that
make them so. The input is a gesture, not a photograph; the answer is a pure
function of four points and an aspect ratio.

Two sliders is a poor interface for this particular question. A person
looking at a leaning building knows exactly which edge should be upright and
cannot say what number that is; they find it by dragging back and forth. The
gesture they already have — *pointing at the edge* — is the one this closes.

## Decision

### 1. A solver, not a stage, and it writes nothing itself

`leyline-engine` gains `keystone`, next to [`auto_tone`](0088-auto-tone-and-black-and-white.md)
and [`auto_tca`](0111-adaptive-chromatic-aberration.md) and for the same
reason: **it renders nothing that is kept**. Its whole output is two numbers
a client writes through an ordinary `EditSession`, so a guided correction
lands in the history as one revision like any other and
[`pipeline.md`](../pipeline.md) §5.1 is untouched by construction. No stage
calls it and none may.

Unlike those two it does not even look at the pixels. It takes lines and an
aspect ratio, and that is all.

### 2. It **enumerates**; it does not solve

The setting is two integers in `[-100, 100]`. There are 40 401 of them.
Evaluating one costs a 3×3 build and two point transforms per line. So the
tool does not run an optimiser — it walks the whole set and keeps the best.

That is not a shortcut, it is the better answer:

* the result is, **by construction, the best value the setting can express**;
* there is no initial guess, no convergence criterion, no step size, no local
  minimum and no iteration count to tune;
* it is deterministic and identical everywhere, which every other number this
  program writes into a revision also has to be.

The whole search is a few million floating-point operations — below the cost
of decoding one row of the photograph it is about.

The homography it evaluates is **the render's own**, `perspective::v1`'s,
reused rather than restated. A solver that agreed with the renderer only
approximately would hand back numbers that do not do what they promised, and
the arrangement is [ADR 0098](0098-per-channel-tone-curves.md)'s: the surest
way to agree with code is to run it.

### 3. What the gesture says, and what the tool refuses

```rust
pub struct GuideLine { pub a: Point, pub b: Point }
```

Two points in the frame the stage sees — normalized, post-rotation,
pre-crop, [ADR 0026](0026-mask-spot-coordinate-referential.md)'s frame and
the one every placed tool in this program already uses.

**A line's own slope says what it is.** Steeper than 45° means *this should
be vertical*; flatter means *this should be horizontal*. There is no mode to
pick and no pair of buttons, because the drawing already contains the answer
and asking for it twice is how a two-click tool becomes a four-click one.

The objective is the sum of squared residuals: for a line that should be
vertical, how far apart its two endpoints land horizontally after the
correction; for a horizontal one, vertically.

Each residual is divided by its line's length, so what is minimised is the
**angle** each guide still makes with true vertical or horizontal, not a
distance. A person drawing a short segment along a window frame means it
exactly as much as one drawing the whole building's edge.

Two refusals, each by name:

* **fewer than two lines.** One line can be made vertical by a whole family
  of corrections, and the enumeration would return whichever it met first —
  an arbitrary answer wearing a measurement's clothes.
* **a line of no length**, a click that never moved.

And one thing it reports rather than hides: **the residual it could not
remove**, in degrees. Either the guides disagree about where the vanishing
point is, or the correction they need is beyond what the sliders reach
([ADR 0052](0052-perspective-correction.md) caps a corner shift at a third of
the frame, deliberately). Returning the best value with the residual beside
it is more use than a refusal — the correction is still the right one, it is
just not the whole of what the photograph needs.

### 4. Picking the tool clears the correction

The lines are read in the frame the stage sees, so they must be drawn on the
photograph as it is *before* the correction. Selecting the guide tool
therefore sets both sliders to zero.

That is a plain edit, undone by the undo everything else is undone by — not a
special mode with a cancel path of its own. And it is what the gesture means:
someone drawing "this edge should be vertical" is redoing the correction, not
adding to it. Reading the guides against an already-straightened picture
would be answering a question about a picture that no longer exists.

### 5. The lines are not stored

They are a gesture, like the click that measures a white balance
([ADR 0091](0091-white-balance-picker-and-presets.md)) and unlike the spots,
red eyes, masks and reshape handles, which are stored because they *render*.
Guide lines render nothing. Keeping them would put a field in every revision,
every preset and every fingerprint to redisplay a scaffold.

The cost is stated rather than hidden: adjusting a guide afterwards means
drawing the lines again. Two clicks, on a tool whose entire job is two
clicks.

## Consequences

* One new engine module and one `Library` method that reads the revision's
  `rotation` to know the aspect of the buffer the lines were drawn on — no
  decode, no render, no catalog write.
* The CLI gains `leyline keystone <library> <version-id> --line x1,y1,x2,y2 …
  [--dry-run]`, mirroring `auto-tca`: it writes by default, and `--dry-run`
  prints the two values and the residual.
* Studio gains a guide tool in the *Geometry* group: draw, draw, apply. It
  reuses the drag the reshape tool already reads.
* **No stage version, no schema change, no golden entry.** Nothing about the
  rendering moves: the tool only picks a value the two existing sliders
  already accepted.
* ADR 0052 §1's refusal of *automatic* correction is unchanged and is now
  worth more, not less: there is a way to get a keystone right without an
  algorithm guessing which edges mattered.

## Rejected

* **Detecting the lines** — Hough, or a model. That is ADR 0052 §1's refusal,
  and [ADR 0102](0102-paid-extensions-and-the-pixel-boundary.md)'s boundary
  says where such a thing would live if it ever shipped: an extension that
  proposes settings, never pixels, and never in the same gesture that applies
  them ([ADR 0109](0109-reshape-stage.md) §5).
* **Four corner handles**, the quadrilateral every "perspective crop" tool
  shows. It expresses a full homography, which is eight numbers, and
  `Perspective` has two — the handles would promise a freedom the setting
  cannot keep, and the correction would silently be the nearest thing the
  sliders could do. Two lines promise exactly what they deliver.
* **A least-squares closed form.** The homography is not linear in the
  sliders, so the closed form would be over a linearisation, and it would
  answer with a real number the setting then has to round. §2's enumeration
  returns the representable optimum directly.
* **Solving inside the render.** An analysis run in a stage would measure
  whichever buffer size the render was asked for, and the same revision would
  look different in the loupe and in the export — ADR 0111 §2's argument,
  unchanged.
* **Letting the guides live in the revision** so they can be nudged (§5).
