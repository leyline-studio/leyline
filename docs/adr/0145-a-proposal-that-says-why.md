# ADR 0145 — A proposal that says why

**Status:** Accepted — 2026-09

## Context

Assisted culling ([ADR 0084](0084-assisted-culling.md)) proposes rejections and
writes nothing. The banner says how many and in how many bursts; the grid then
shows those photographs and **nothing else** — no mark on a cell, no reason,
nothing to tell the frame that lost a burst from the one that was black.

The reason exists. `Verdict::Reject` carries a `RejectReason` — `Softer`,
`Blown`, `Black` — and `Verdict::Pick` names the keeper of a burst. The CLI has
printed all four from the start:

```
asset 2  burst 0  focus 0.608  reject  (asset 1 of the same burst is sharper)
asset 4  burst 1  focus 0.000  reject  (nothing in it — black frame)
```

Studio dropped every word of it. A proposal one cannot question is one nobody
should accept — and ADR 0084 §2's whole argument for proposing rather than
writing is that the photographer decides.

## Decision

### 1. The cell says what was measured

`Cell` gains the verdict in words — translated by Rust, displayed by the panel,
the division ADR 0045 §2 sets — and the grid draws it as a small chip over the
thumbnail's foot: « la plus nette de la rafale », « moins nette »,
« sur-exposée », « noire ».

Three details are the decision:

* **Words, not a glyph.** The grid's four corner badges are glyphs because each
  stands for one state a reader already knows (`⚑`, `✕`, `◐`). A *reason* is a
  sentence; there is no icon for "another frame of this burst is sharper", and
  inventing one would need the explanation the hint mechanism exists to avoid
  ([ADR 0127](0127-hints-on-wordless-controls.md) §1).
* **What was measured, never « rejected ».** Nothing is written, and the words
  say what the assistant saw so the photographer can disagree with it.
* **The keeper is marked too**, and in a different colour — blue where the
  rejections are amber. In a burst of five, one cell reading « la plus nette »
  beside four reading « moins nette » is the proposal, whole, in a glance.

`Verdict::Keep` gets no chip: most photographs are keepers, and a badge on
nearly every cell says nothing at all.

### 2. It exists only while a proposal does

The chip is drawn from `App::proposal`, which lives in the window and dies with
it ([ADR 0084](0084-assisted-culling.md) §2 refuses to persist a proposal).
Discarding it clears the cells with it — verified on the built binary.

## Consequences

* The culling loop is now readable without the CLI: run it, see why each frame
  is proposed, press `X` on the ones you agree with.
* `Cell` gains two fields, `Tr` four words. Nothing else moved: the engine
  already computed all of it.
* This is the second surface in a week where the engine knew something the
  interface did not say ([ADR 0143](0143-a-filter-worth-keeping.md) was the
  first). Worth watching as a pattern rather than a coincidence — a feature is
  not finished when the engine can do it.

## Alternatives rejected

* **A column in the detail panel.** It answers about *one* photograph, and a
  proposal is read across a grid.
* **Naming the frame that won** (« moins nette que la n°3 »). The number is a
  grid index that changes with the sort, and the keeper is on screen two cells
  away wearing its own chip.
* **Showing the focus score.** `0.608` is a number from a measurement whose
  scale means nothing to a photographer, and the ranking is what the proposal
  is about.
