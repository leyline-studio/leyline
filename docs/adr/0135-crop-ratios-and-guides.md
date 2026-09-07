# ADR 0135 — A crop with a shape, and lines to place it by

**Status:** Accepted — 2026-09

## Context

Cropping in Studio is one gesture: pick the Crop tool, drag a rectangle,
release. `drag_crop` turns the two corners into a `Crop` and commits.

Nothing else. Counted against the two references:

* **No aspect ratio.** Every crop is freehand, so a square is a square only
  as accurately as a hand can drag one, and a series of frames cropped for the
  same print comes out at a dozen different shapes. Lightroom has had a
  ratio menu since version 1; darktable has one too.
* **No composition guide.** Nothing is drawn over the photograph while the
  crop is being placed — no thirds, no centre, nothing. Both references cycle
  a set of overlays with `O`, and it is the one thing a crop tool draws that
  is about the *picture* rather than about the rectangle.

Both are absences rather than defects, and both are cheap here for a reason
worth stating: the develop viewer already letterboxes the render with
`image-fit: contain`, so the photograph on screen is undistorted. A ratio
constraint is therefore a constraint on **screen pixels**, and a guide is a
line in the same space. Neither needs to know anything about the image.

## Decision

### 1. A contextual row, shown only while cropping

The ratio and the guide live in a second toolbar row that appears when the
Crop tool is picked and disappears when it is not.

Not in the Geometry group opposite, where *Reset crop* lives: that group holds
the crop's *numbers* — four edges one can type — and this row holds what the
next drag will do. A setting that changes the meaning of a gesture belongs
beside the gesture, and a row that appears when a tool is chosen is how the
tool says what it can do.

### 2. Seven ratios, and the constraint is on screen pixels

`Free`, `1:1`, `5:4`, `4:3`, `3:2`, `16:9`, `7:5`, plus a swap that turns any
of them on its side.

The constraint is applied to the rubber band **as it is drawn**, and the
already-constrained corner is what reaches Rust. That is the load-bearing
decision: constraining in Rust after the fact would draw one rectangle and
commit another, and the two would eventually disagree about rounding. One
place, one answer.

It works in screen pixels because it can: the render is letterboxed
undistorted, so a sub-rectangle whose *screen* ratio is 3:2 encloses a region
whose *pixel* ratio is 3:2. `drag_crop` is unchanged, and knows nothing about
ratios.

The longer edge of the drag drives: whichever of width and height the pointer
has taken further from the anchor is kept, and the other is computed. Dragging
diagonally therefore always grows the rectangle rather than fighting it back.

**No "Original".** Lightroom has one, and it means the aspect of the file.
Ours cannot: §5's crop is *relative* — a drag crops what is already cropped —
so "original" would name the current crop's shape and read as a lie the second
time it is used. Better absent than wrong.

### 3. Five guides, cycled with `O`

`None`, `Thirds`, `Golden`, `Diagonals`, `Grid`, drawn over the photograph
while the Crop tool is active and nowhere else — a guide over a photograph one
is *looking at* is a guide in the way.

`O` cycles them, which is the key both references use. The chip in the row
names the current one, so the key is discoverable rather than folklore.

Drawn as `Path` elements with a viewbox equal to the element's own pixel size
— the arrangement [ADR 0119](0119-guided-keystone.md)'s guides already use and
which is known to render in the software backend. That is why `Diagonals` is
in the list at all: a diagonal cannot be drawn with axis-aligned rectangles,
and the technique that draws it is one already shipped rather than a new bet.

The lines are white at low opacity with a darker companion beneath, because a
guide has to be visible over a white sky and a black shadow, and one colour
cannot do both.

### 4. The guide follows the rubber band, not the frame

While a rectangle is being dragged, the guide is drawn **inside that
rectangle**. Before a drag, it is drawn over the whole frame.

This is the whole point of a guide during a crop: thirds of the frame one is
about to discard are thirds of nothing. Lightroom does the same, and it is the
part of the feature that is easy to get wrong by drawing the overlay once over
the viewer and leaving it there.

### 5. Out of scope, and one of them is a real limitation

* **Enlarging a crop.** A drag crops *the current crop*, because the viewer
  shows the cropped render — so a crop can be tightened, never loosened, and
  the only way back is *Reset crop*. Named here because §2's ratios inherit
  it: choosing 16:9 lets one draw a 16:9 rectangle inside the current frame,
  not around it. Fixing it means showing the uncropped photograph while the
  Crop tool is active, with the discarded margin dimmed — a change to what the
  viewer renders, not to this row, and a decision of its own.
* **Handles on an existing crop.** [ADR 0097](0097-mask-geometry-handles.md)
  gave masks handles; the crop has none, and dragging afresh is the only
  adjustment. Same blocker as above: there is nothing outside the frame to
  drag towards.
* **A ratio typed by hand** (`2.39:1` for a scope crop). The seven cover the
  formats photographs are printed and shown at; an eighth is one line here the
  day somebody names it.
* **Rotating the guide with the crop.** Rotation is a separate setting and the
  render is already rotated when it reaches the viewer, so the guide over it is
  already right.

## Consequences

* `drag_crop` is untouched, and so is `Crop`, `settings_json`, and every
  golden case. This ADR adds no engine surface at all — it is entirely a
  matter of what the panel draws and what corner it reports.
* The crop row is the second contextual toolbar in Develop. If a third
  appears, the pattern is worth extracting; two is not yet a pattern.
* `O` joins the develop key set. It is free — checked against the shortcuts
  card, which gains a line.

## Alternatives rejected

* **Constraining in Rust, after the drag.** §2. Two answers to one question.
* **A ratio dropdown.** Seven chips are one click; a dropdown is two, and the
  current ratio is then invisible until opened.
* **Drawing the guide over the whole viewer while dragging.** §4.
* **Guides outside the Crop tool**, as a permanent composition aid. A line
  across a photograph one is grading is an obstruction, and the tool that
  needs it is the one that gets it.
