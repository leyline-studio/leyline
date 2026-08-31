# ADR 0097 — Drag handles: editing a mask instead of retracing it

**Status:** Accepted — 2026-08

## Context

ADR 0049 shipped the local-adjustment tools and named two leftovers. The
range eyedropper closed with ADR 0093; this closes the second and last:
**drag handles on a geometry already drawn**.

Until now a radial that was ten pixels too high had to be *retraced* —
the tool replaces a geometry of the same kind rather than editing it
(ADR 0049 §2), so the only correction available was to do the whole
gesture again, and with it lose the feather, the inversion and the range
band the entry had accumulated. That is not a missing convenience; it is
the reason people stop using a mask tool.

## Decision

### 1. A handle drag is a geometry edit, decided in Rust

`masks::drag_handle(handle, release, view, image, current)` takes which
handle was grabbed and where it was let go, and returns the new `Mask` —
pure, unit-tested, free of any Slint type, exactly as `drag_geometry`
already is. Everything else about the entry is untouched: the same row,
the same feather, the same range, the same values. Only the geometry
moves.

The handles are:

* **radial** — `center` (moves the ellipse), `rx` and `ry` (resize along
  one axis each, from the centre outward);
* **gradient** — `from` and `to`, its two ends.

A rotation handle is **not** among them: `Mask::Radial::angle` exists and
the outline deliberately does not draw it (a rotated ellipse shows its
unrotated outline today), so a handle that set it would be a control
whose result the interface cannot show. Drawing the rotated outline is
the prerequisite, and it is its own slice.

### 2. Handles appear under the Select tool, and only for the selected row

A tracing tool is armed to *draw*; if handles were live at the same time,
the first click of a new radial would land on the old one's handle. So
handles show when `active-tool == "select"` and a mask row is selected —
which is also the state a photographer is in when looking at what they
just made.

They are drawn as small squares, not `Path` shapes: the software renderer
drops most `Path` glyph-like marks (ADR 0049 §5's trap, paid again by the
star ratings), and a `Rectangle` with a border radius is the shape the
repository already trusts.

### 3. A degenerate result is refused, not stored

Dragging `rx` onto the centre would make a zero-radius ellipse — a mask
covering nothing, which `Settings::validate` refuses anyway. The pure
function returns `None` below the same 0.005 floor `drag_geometry` uses,
so the gesture is simply ignored and the previous geometry stands. The
two functions share the constant rather than each having an opinion.

## Consequences

* `docs/specification.md`'s "what remains" sentence for ADR 0049 can go:
  both leftovers are now delivered.
* Studio only. No engine surface, no stage, no stage version, no golden:
  a handle drag writes the same `Param::LocalAdjustment` any slider does,
  through the ordinary session, as one undoable revision.

## Rejected

* **Editing on the tracing tool itself** (grab a handle while `radial` is
  armed) — §2: the tool is armed to draw, and the ambiguity would cost a
  mask on every mis-click.
* **A rotation handle now** — §1: it would set a value the outline cannot
  show, which is a control that lies.
* **Live geometry while dragging a handle** — the drag commits on release
  like every other gesture here (ADR 0049 §1); the live-preview path of
  ADR 0074 is for sliders, whose value is a number rather than a shape,
  and pointing it at a mask would re-render the mask overlay per frame.
