# ADR 0148 — Choosing an order, instead of cycling through eight

**Status:** Accepted — 2026-09

## Context

The grid's order lives behind one chip at the right of the filter bar, reading
*Sort: filename A–Z*. Clicking it advances to the next of eight orders, in a
fixed ring: capture ↓, capture ↑, filename A–Z, filename Z–A, imported ↓,
imported ↑, rating ↓, rating ↑.

Two things are wrong with that, and the second is the worse one.

**It is a ring, so a choice costs up to seven clicks.** Going from *filename
A–Z* to *rating ↓* is four, and every one of them reloads the grid: a catalog
query, a window of rows, a thumbnail request per visible cell. The control asks
the user to walk through orders nobody asked for in order to reach the one they
did.

**It does not say what it is.** A control that changes state on click without
showing its other states is only learnable by clicking it eight times and
remembering — and there is nothing to remember it *from*, since the ring's
contents appear one at a time.

There is a third defect, found while reading the code for the first two: those
eight labels are **English prose built in Rust** (`SORTS` in `app.rs`), so a
French window says « Tri : filename A–Z ». `Tr` exists precisely so that no
sentence is assembled in Rust ([ADR 0019](0019-distribution-i18n.md)); this one
predates it and nothing caught it, because a string built in Rust is a string
no `.pot` ever learns about.

## Decision

### 1. The chip opens the choice, and the choice is two questions

Clicking *Tri* opens a strip under the filter bar — the surface the shot
filters ([ADR 0064](0064-metadata-filters.md) §5) already use, for the same
reason: it narrows the same grid as the chips just above it, so it belongs
under them and not in a popup of its own.

The strip holds the two things one actually chooses:

* **What to order by** — *Date de prise de vue*, *Nom de fichier*, *Date
  d'import*, *Note*. Four chips, the active one lit.
* **Which way** — two chips spelling out both ends **for the key in force**:
  « du plus ancien » / « du plus récent » for the two dates, « A → Z » /
  « Z → A » for the name, « des moins notées » / « des mieux notées » for the
  rating.

Any of the eight orders is then one click from any other, and — this is the
point — the eight are *visible* without changing anything.

Spelling the direction out rather than drawing an arrow is the decision inside
the decision. `↑` on a date and `↑` on a name and `↑` on a rating mean three
different things, and the reader has to guess which. The chip that stays in the
bar keeps a short form (`A–Z` for the name, `↑`/`↓` elsewhere), because there
it is a reminder of a state, next to a control that explains itself when
opened.

### 2. The words are the interface's, the order is Rust's

`FilterState` carries `sort-key` (an int) and `sort-ascending` (a bool), and
`Tr.sort-name` / `Tr.sort-direction` turn them into words. Rust sends what was
chosen, never how it reads — the division [ADR 0045](0045-studio-ui-modularisation.md)
§2 sets and the one the old labels broke.

What is **not** touched is `preferences.json`. The stored order was already a
stable key (`filename-asc`), deliberately "written out rather than derived from
the display label: the label is prose and will one day be translated, and a
stored value must survive that". That day is today, and the file needs no
migration — the note in `sort_key` paid for itself.

`Sort::CollectionOrder` has no place in the strip and gains none: a collection's
own order is a property of that view, not a sort anyone chose, and Studio never
sets it.

## Consequences

* Eight orders, one click each, and all eight readable before choosing.
* Three strings leave Rust and enter the `.pot`; the French window stops saying
  « filename A–Z ».
* `SORTS` loses its label column and becomes the list of orders it always was.
  `sort_label` is replaced by `sort_parts`, and `cycle-sort` by
  `choose-sort(key, ascending)`.

## Alternatives rejected

* **A dropdown anchored under the chip.** What everyone expects, and it needs a
  second layer of absolute positioning in a window whose panels all clip —
  exactly what `widgets/menu.slint` declined for its own submenus, "for little
  benefit here". The strip is in the same place, costs none of that, and is
  already an idiom of this bar.
* **Eight chips in the strip**, one per order. One click too, and it makes the
  reader compare eight things where there are only two questions — and it
  doubles every future key.
* **Keeping the cycle as a shortcut** on a modifier-click. A second way to do
  one thing, learnable by accident, documented nowhere.
* **Sorting by clicking a column header.** There are no columns: this is a grid
  of thumbnails, and the day a list view exists the header is where this
  belongs.
