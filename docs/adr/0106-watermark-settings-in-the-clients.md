# ADR 0106 — The watermark's own settings reach the clients

**Status:** Accepted — 2026-09

## Context

[ADR 0034](0034-softproofing-watermark-print.md) decided the watermark and
[ADR 0051](0051-watermark-rasterization-and-soft-proof-surface.md) rasterized
it. `ExportSettings::watermark` has carried six fields ever since: the line,
the typeface, the size, the colour, the opacity and the anchor.

The clients carry fewer. The CLI takes `--watermark` and
`--watermark-anchor`; Studio's export dialog offers a single text field. The
comment sitting above the CLI's parsing said why, and it is worth quoting
because it is the part that turned out to be wrong:

> Only the line, like Studio's dialog: the rest of the decoration keeps the
> recipe defaults (ADR 0051 §3), and a preset's `settings_json` is where
> other values are written.

**That escape hatch does not exist.** `leyline preset` accepts the same two
options as `leyline export` and no others, and Studio's "save as preset"
builds its recipe from the same dialog. Nothing in either client writes a
`settings_json` by hand. The practical consequence is exact, and it is the
whole reason for this ADR: **every watermark Leyline can currently produce
is white, 3 % of the image height, 70 % opaque, in the bottom-right corner.**
A photographer who wants a dark line on a pale photograph cannot have one.

This is the shape [ADR 0094](0094-versions-in-the-clients.md) already met and
named — an engine paid for and left invisible — and it is settled the same
way: no engine change, only the gesture that reaches it.

## Decision

### 1. The CLI gains three options, and deliberately not a fourth

`--watermark-size`, `--watermark-color` and `--watermark-opacity`, beside the
two that exist. Each is refused with the range it wanted when it is out of
bounds, by `Watermark::validate` — the clients do not restate the ranges,
they hand them over and report what comes back.

There is **no `--watermark-font`**. `WatermarkFont` has exactly one variant
(ADR 0051 §2: one embedded typeface, so a recipe can never ask for something
the binary does not carry), and an option whose only accepted value is its
default is a flag that does nothing. It arrives with the second typeface, or
not at all.

Like `--watermark-anchor` before them, the three need `--watermark`: they
decorate a line, and there is no line to decorate without it.

### 2. Studio's export dialog shows the same four settings

Anchor, size, colour and opacity, under the text field that already exists.
The dialog is where a watermark is composed, and a value nobody can see is a
value nobody chooses.

### 3. The wiring reads the watermark from the state, rather than being handed it

`run_export` and `run_save_export_preset` already took seven and six
arguments. Adding four more to each would make eleven-argument callbacks
whose order is the only thing keeping them correct.

The watermark's fields are therefore **read from `DialogState` inside the
wiring**, and `watermark` leaves the callback signatures — which get shorter,
not longer. Both callbacks already hold the `window` they would read it from.
The rule this sets, for the next dialog that grows: a group of settings that
travels together is read together, not spread across a parameter list.

### 4. Nothing in the engine moves

No stage, no stage version, no schema, no golden case. `docs/pipeline.md`
§5.1 is not in play: the watermark is composited in the export path and was
never part of a revision (ADR 0034 §"neither is a process version").

## What stays refused

Restated so this ADR does not read as an opening:

* **The image/logo watermark** — cut by ADR 0034, and the asset-reference
  problem behind it is not settled by this change.
* **A drop shadow, a rotation, the nine-position grid** rather than five
  anchors. Each is more surface on a feature whose whole point is a
  copyright line, and none of them is why a watermark is currently unusable.
* **A configurable margin.** It stays half the type size, as ADR 0051 §3
  fixed it: that is placement, not decoration, and it is the one number a
  photographer has no way to judge from a dialog.

## Consequences

* The three clients express the same watermark. A recipe written by the CLI
  and one composed in Studio can now be the same recipe, which they could
  not be before.
* `Watermark::validate` becomes the single place the ranges live, and it
  already was — the clients gained no copy of them.
* The CLI's misleading comment goes with the change that makes it false.

## Alternatives rejected

* **Leaving it, and documenting `settings_json` as the way in.** It would
  mean telling a photographer to edit a catalog's JSON to choose a colour.
  The catalog is an implementation detail of the library, not an interface.
* **A watermark editor of its own**, LrC-style, with presets. The export
  preset already stores a complete watermark; a second preset system for a
  subset of one recipe would be two places to look for the same answer.
* **Passing the fields as a Slint struct** to keep the callback shape. It
  works, but it adds a type to `ui/` whose only purpose is to survive a
  parameter list — the parameter list is what should go.
