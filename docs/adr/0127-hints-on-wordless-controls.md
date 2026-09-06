# ADR 0127 — A hint on a control that has no words

**Status:** Accepted — 2026-09

## Context

[ADR 0054](0054-first-run-and-basic-mode.md) §4 refused tooltips in one line:

> Software that needs to be explained on top of its interface has a problem
> in its interface.

That is right, and the interface has since acquired the problem it describes.
Counted on the built binary, Studio now has **eleven controls with no word on
them**, every one of them added deliberately and for good reasons:

| Control | Where | Introduced by |
| :--- | :--- | :--- |
| Two small squares at the histogram's corners | Develop | [ADR 0092](0092-clipping-indicators.md) §2 |
| `⟲` beside a group heading | Develop | [ADR 0112](0112-four-panel-affordances.md) §4 |
| `?` at the end of the toolbar | Develop | ADR 0054 §4 itself |
| Two eyedroppers | Develop | [ADR 0091](0091-white-balance-picker.md), [ADR 0093](0093-range-mask-eyedropper.md) |
| `+` beside *Presets* | Develop | [ADR 0124](0124-develop-left-column.md) §1 |
| Five coloured dots | Library filter bar | [ADR 0055](0055-library-navigation.md) §3 |
| `⚑` / `✕`, and the half-disc | Grid cell | ADR 0055 §3 |

The last row is the exception, and §4 says why.

A pictogram is, by construction, an interface that has to be explained. The
argument in §4 was aimed at a *layer of explanation over a labelled
interface* — a guided tour, a first-run assistant, a tooltip on a slider that
already says `Exposure`. It was never an argument about a 10x10px square
whose entire meaning is a convention the reader either has or does not.

The two references both answer this the same way, and neither is a tour:
hovering any glyph names it, and names its shortcut.

## Decision

### 1. A hint appears **only** on a control that carries no words

That is the rule, and it is what keeps this from becoming the thing ADR 0054
§4 refused. `Exposure` gets no hint. `⟲` does. If a control needs a hint and
*could* have carried a label, the bug is the missing label — the hint is for
the ones where a word genuinely does not fit, which is why they are glyphs in
the first place.

A consequence worth stating: the list of hints in the source is a list of the
places the interface is not self-evident. It should be short, and it should
be read as a list of debts rather than a feature.

### 2. It says what the control does, then its key

`Show highlight clipping    J`, in the layout the menus already use for the
same pair (ADR 0055 §6) — so a control, a menu item and the shortcut card all
print a shortcut the same way. Where there is no key, there is no second
column.

### 3. It is drawn by the window, not by the control

A global carries the text and the position; `studio.slint` draws one hint
layer above every panel. Three reasons, all mechanical: a hint has to escape
the `Flickable` that clips its control, it must not be part of any layout it
appears over, and there must be exactly one on screen. The control reports
`absolute-position` and its own size — an output property every element
already has — so the layer needs no knowledge of who asked.

**500 ms of hover**, on a `Timer` restarted by each new control: long enough
that moving the pointer across the toolbar shows nothing, short enough to feel
like an answer. It disappears on pointer-out and on any click, and never
appears while a mouse button is down — a hint that pops up mid-drag is
covering the photograph one is adjusting.

### 4. Out of scope

* **The three badges on a grid cell** — the flag, the reject cross, the
  half-disc that means *already developed*. They are the one row of the
  table above that is **not a control**: nothing there is clickable, and a
  `TouchArea` over a 10px badge inside a cell whose whole surface selects the
  photograph would buy an explanation at the price of a dead spot on the
  cell. The badges are explained where they are *set* — in the Photo menu,
  beside the keys that set them.
* **Hints on the menu bar.** The menus already print their shortcuts.
* **A hint on every slider** — §1.
* **Rich hints** (a diagram, an example, a "learn more"). That is
  documentation, and `docs/` is where it lives.

## Consequences

* `HintState` joins `ui/state/` with four properties and three functions;
  the hint layer is ~30 lines at the end of `studio.slint`, declared after
  the menu bar so an open menu cannot cover the hint of a control beside it.
  It is the one global that carries interface state and nothing else — the
  exception ADR 0045 §2 allows, earned here because the two ends of a hint
  are in different files and neither owns the other.
* `FilterChip`, `PipetteChip` and `GroupHeader` gain an optional `hint`
  string. A control that sets none behaves exactly as before, which is what
  makes the rule in §1 enforceable by reading the source.
* Ten strings enter `leyline-studio.pot`, so the extractor has to be re-run
  in the same change — the standing trap this repository has recorded more
  than once.
* Nothing crosses to Rust; no rendering, no revision, no stored format.

## Alternatives rejected

* **Labelling the glyphs instead.** For `⟲` beside a heading, or a square at
  the corner of a histogram, there is no room, and ADR 0112 §4 chose the
  glyph knowing that. Labelling them means undoing four earlier decisions.
* **Slint's `PopupWindow`.** It takes a grab and closes on the next click,
  which is right for a menu and wrong for a hint that must survive the
  pointer moving one pixel; and inside `Flickable` its placement is the
  thing being worked around.
* **Showing hints only until the user has seen each once.** State per control,
  per user, and a hint that is missing the day it is wanted.
