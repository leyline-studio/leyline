# ADR 0149 — A way back one can see

**Status:** Accepted — 2026-09

## Context

Develop can be walked backwards three ways: `Ctrl+Z`, the *Develop ▸ Undo* menu
row, and the History panel in the left column, which since
[ADR 0142](0142-a-history-that-says-what-changed.md) names what each revision
changed and jumps straight to it.

None of them is a **button**. The module one spends an hour in, moving sliders,
has no visible way back — and the two that exist are found by someone who
already knows they exist: a keystroke nobody announced, and a menu one has to
open to discover. The History panel is the real answer to "where was I", but it
is a list one reads, not the gesture one reaches for after a slider went too
far; and it is the first thing to fold on a narrow window
([ADR 0125](0125-narrow-window-develop.md) §1).

Two smaller defects come with it. The two menu rows are **never disabled**, so
*Undo* at the first revision of a photograph is an entry one can click, and
clicking it does nothing and says nothing — `EditSession::undo` returns
`Ok(None)` when there is nothing to move. And nothing on screen says whether
there is anything ahead to redo.

## Decision

### 1. Two chips at the head of the develop toolbar

`↶` and `↷`, first in the tool row, with a separator between them and *Select*:
they are actions, and the chips after them are tools.

They are **glyphs with hints** rather than words. The toolbar's width is the
constraint ADR 0125 §3 exists about — ten chips already scroll on a narrow
window — and « Annuler » plus « Rétablir » cost about 140 px where the two
glyphs cost 60. A chip whose label is a glyph says what it is on hover, which is
the mechanism [ADR 0127](0127-hints-on-wordless-controls.md) §1 built for
exactly this case, and the hints carry the shortcut: « Annuler (Ctrl+Z) ».

Inside the `Flickable`, not floating over it. A `Flickable` reports no minimum
width of its own — ADR 0125 §3's whole point — so two more chips there cost the
layout nothing, while two floating over the row would cover whatever scrolled
under them.

### 2. They are disabled when there is nothing to do

`DevelopState` gains `dev-can-undo` and `dev-can-redo`, and the two menu rows
take the same guard. Both are read off the history the panel already shows: the
list is every revision of this version, oldest first, and `dev-history-current`
is where the head sits in it — so there is something to undo when that index is
above zero, and something to redo when a row follows it.

That list is the right source rather than a new question put to the engine,
because it is what the reader is looking at: a chip that is lit while the
History panel shows no row above the current one would be the interface
disagreeing with itself.

Redo stays available after an undo because `App::dev_history` **accumulates**:
revisions walked away from stay in the list, which is what makes the way forward
visible at all.

## Consequences

* The way back is on screen, in the module where one needs it, and greyed when
  there is none.
* `Ctrl+Z`, the menu row and the chip are one call; nothing about the revision
  graph moved ([ADR 0008](0008-version-as-library-unit.md) still owns it).
* Two properties and two chips. No engine, no schema.

## Alternatives rejected

* **Words instead of glyphs.** Clearer at a glance, and 140 px of a row that is
  already the narrow window's problem. The hint is the trade ADR 0127 §1
  already priced.
* **A pair in the History panel's header**, where they belong conceptually. It
  is the column that folds first, so the button would vanish exactly on the
  screens where the menu is hardest to reach.
* **Floating them over the toolbar** like the scroll chevrons. They would cover
  chips, and the chevrons are already at both ends.
* **Making `Ctrl+Z` say something when there is nothing to undo.** A message for
  a non-event; the greyed chip says it before the press, which is the better
  moment.
