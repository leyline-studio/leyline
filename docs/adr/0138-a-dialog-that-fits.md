# ADR 0138 — A dialog that fits its window

**Status:** Accepted — 2026-09

## Context

A screenshot of the import dialog, taken on Linux: the row of options runs out
through the card's right border, past the buttons, and off the edge of the
window. *Copy into library · Recursive · Pair RAW+JPEG · Exact (reads every
file) · Look first* is 640px of French chips in a card that is 480px wide, and
[ADR 0125](0125-narrow-window-develop.md) §1 already recorded what a
`HorizontalLayout` does when it cannot fit its children: it does not shrink
them, it overflows to the right. There it was the develop toolbar. Here it is a
modal dialog, and the overflow paints over the application behind it.

Rather than fix the one row, every dialog was opened and photographed in
French, on a build with a temporary hook that opens a named dialog at startup —
twenty-three of them, `import` through `about`. The frame they share
(`panels/dialogs.slint`, [ADR 0045](0045-studio-ui-modularisation.md) §3) is a
480px card whose height is `body.preferred-height` and which clips nothing, so
the survey found two failures and one near miss:

* **Horizontal.** The import row above.
* **Vertical.** *Contact sheet*, *Print* and *Keyboard shortcuts* are taller
  than the window at 900x667. A card centred on a window it exceeds grows out
  through the **top and the bottom at once**, and what leaves by the top is the
  title: the contact-sheet dialog opened on a headless subtitle, and its
  buttons were below the bottom edge of the screen.
* **The near miss.** In `SettingsGroupChips` — the seventeen categories of
  *Copy settings* and *Save as preset* — the second row's fourth chip, « LUT
  créative », ended exactly on the card's border. Those rows were balanced by
  hand on the English labels; French is longer, and the measurement that
  settles them is the translated one.

The common cause is that the frame guarantees nothing. It draws a card of a
fixed width and lets each dialog be as large as it likes in both directions.

## Decision

### 1. The card is bounded by the window, and it clips

`height: min(body.preferred-height, window-height - 80px)`, and `clip: true`.
Anything longer scrolls inside a `Flickable` whose `viewport-width` follows the
card, so the text inside still wraps to the card rather than laying itself out
on one line the reader would have to scroll sideways to read.

The window's height is **handed down** from `studio.slint` as a property rather
than read from the overlay's own `height`: the overlay is a child of a
`FocusScope` inside the window, its height is whatever that tree gives it, and
what a dialog must fit inside is the window. This is not a detail — the first
version read `root.height` here and produced a value large enough to make every
bound useless while looking correct in the source.

A slim mark on the right edge, 3px wide, appears only when there is something
below the fold. It is a **mark and not a handle**: the wheel is what scrolls
here, and [ADR 0125](0125-narrow-window-develop.md) §3 refused the same thin
grab-target in the toolbar for the same reason.

### 2. The width stays fixed, and rows are split by hand

Letting the card widen to its content was built and measured before being
rejected. `width: clamp(480px, body.preferred-width, window - 80px)` fixes the
import row — and turns the tether dialog into a single 505px line and the
report dialog into a 690px one, because a wrapping `Text` reports its
*unwrapped* width as preferred. The result is no two dialogs the same width and
a paragraph measure set by whichever sentence happens to be longest.

So the card keeps its 480px, and a row that does not fit is split into two by
hand. In the import dialog the split is also the reading: what the import will
do (copy, recurse, pair) on one line, what looking at the folder first would
cost (exact, look) on the next, right-aligned away from the options it is not
one of.

### 3. Chip rows are measured in the translated build

`SettingsGroupChips` goes to **three chips per row**, from four-and-three. The
widest three in French — « Courbe des tonalités », « Mélangeur TSL »,
« Étalonnage couleur » — come to 409px of the 440px between the card's
paddings; a fourth puts the row through the border. The row count is unchanged
at six, so the dialog is no taller than before.

The rule this makes explicit: **a hand-balanced row is measured on the
translated build**, not the English one. The comment in that widget already
said "measured on the built dialog" and it was measured in English, which is
how « LUT créative » reached the border with nobody noticing.

### 4. The import contact sheet yields its height

The one dialog whose height cannot be written down in advance: after a scan it
carries a list of candidates, one 60px row each ([ADR 0065](0065-selective-import.md)).
It kept six rows whatever the window, which put *Cancel* and *Import 14* — the
point of the dialog — below the bottom of the card, reachable only by
scrolling.

It is now handed the room the card has (`body-space`) and gives back what it
cannot use: `max(120px, min(rows × 60px, body-space − 265px))`, where 265px is
measured on the built dialog — 190px for everything above and below the list,
40px for its own heading and gaps, 35px for the result line the frame adds
underneath once a scan has run. Two rows minimum: a list of one row is not a
contact sheet, and a window that small has a worse problem.

Two traps found by measuring rather than by reading, both worth the line they
take here:

* **`clamp` takes the value first.** Slint's is `clamp(value, minimum,
  maximum)`. Written `clamp(60px, rows * 60px, room)` it reads like a bound,
  compiles, and silently returns a list taller than the card. Three builds
  looked identical before the values were printed into the dialog's title to
  find out why.
* **A sibling's `preferred-height` reads as 0** from inside a layout, so
  `body-space − head.preferred-height − foot.preferred-height` is not
  available and the measured constant is. That is also why the constant is
  written with what each part of it pays for.

## Consequences

* No dialog can paint outside its own card any more, in any language: past the
  card's edge the content is clipped, and past its bottom it scrolls.
* The 480px card is now a **contract**: a new dialog is written to fit 440px of
  content, and a row that does not is split. The frame no longer forgives it
  silently.
* Three constants — 80px of margin, 265px of import chrome, three chips per row
  — are measurements of the built interface at today's font size. Each says in
  its comment what it is made of, so the next person changing a padding knows
  which number moved.
* The survey that found this is repeatable: a temporary hook that opens a named
  dialog at startup, twenty-three launches under `Xvfb`, one screenshot each.
  It cost one build and found three defects that `make check` cannot see.

## Alternatives rejected

* **A card that widens to its content** — §2, built and measured.
* **A wrapping flow layout for chips.** Slint has none, and building one means
  a model of chips with computed positions: the chips lose their individual
  bindings and callbacks, and every dialog that uses one gains an indirection.
  Splitting a row by hand costs two lines and keeps the source readable.
* **A visible scrollbar with a draggable thumb**, or Slint's `ScrollView`. The
  first is a 3px grab target; the second brings the standard widget style into
  an interface that draws its own.
* **Raising the window's `min-height`** so tall dialogs always fit. It refuses
  an ordinary laptop for the sake of two dialogs, which is the trade
  [ADR 0125](0125-narrow-window-develop.md) already refused for Develop.
* **Eliding chip labels to fit.** A chip whose label is cut is a control whose
  meaning is cut; the label is the whole affordance
  ([ADR 0127](0127-hints-on-wordless-controls.md)).
