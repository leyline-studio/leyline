# ADR 0125 — A narrow window in Develop: what yields, and what never does

**Status:** Accepted — 2026-09

## Context

`StudioWindow` declares `preferred-width: 1280px` and `min-width: 900px`.
Develop honours neither. Measured on the built binary, at three widths:

| Window width | What Develop shows |
| :--- | :--- |
| 1400px | correct — the width `startup_size` opens a 1920x1080 screen at |
| 1280px | the settings panel is cut off at the window's edge: the value column of **every** slider is outside, `Reset All` is truncated mid-word, the white-balance chips are halved |
| 900px | the settings panel is **entirely** outside the window. No histogram, no mode switch, no slider. The toolbar is clipped mid-chip |

The cause is not the photograph, which was the tempting explanation and is
the same wrong one [ADR 0124](0124-develop-left-column.md)'s implementation
notes already record for the *maximum* width. It is that the middle column's
rows carry a **minimum** of their own: the toolbar is ten fixed-width chips
in Full mode — each sized by its own label, and markedly wider in French —
some 820px of them. A `HorizontalLayout` that cannot fit its children does
not shrink them: it overflows to the right, and what is on the right is the
settings panel.

So the failure is not gradual. There is a width above which Develop is
perfect and a width below which it is unusable, and the second one is inside
the range the window itself offers: 1366x768 is still an ordinary laptop, and
900x600 is a size the user is invited to drag to.

The browser does not have this defect. [ADR 0055](0055-library-navigation.md)
§6 gave it a `narrow-threshold` that folds its side panels before the layout
runs out of room, and `studio.slint` states in a comment why Develop was
deliberately excluded from it:

> Deliberately not used by Develop. There, the 300px panel *is* the module —
> every slider lives in it — so folding it automatically would turn Develop
> into a picture viewer on a small screen, which is a worse failure than the
> one this fixes.

That reasoning is right about the settings panel and wrong about its
conclusion: not folding anything turns Develop into a picture viewer *as
well*, and without even the fold to explain it. The choice was never "fold
the sliders or keep them"; it was "fold something else, or lose the sliders".

## Decision

### 1. The left column folds, the settings panel never does

Below **1150px** the left column — Navigator, Presets, Versions, History —
is folded automatically, exactly as the browser folds its own two panels, and
by the same mechanism: a property on `StudioWindow` **assigned** from a
`changed width` handler rather than bound, because a binding that reads
`root.width` to decide what content exists is the layout cycle
[ADR 0045](0045-studio-ui-modularisation.md) §3 records.

Which column yields is the decision, and [ADR 0124](0124-develop-left-column.md)
§1 already contains the answer without having been asked the question: the
left column is what one **chooses** with — where am I looking, what look can
I put on this, which branch am I on — and the right column is what one
**works** with. A photographer on a small screen can choose from the
Library module, one keystroke away; there is nowhere else to put an exposure
slider.

1150px is measured, not picked: 260px of left column and 300px of settings
leave 590px of viewer at that width, which is still a photograph one can
judge. Below it the viewer is what is being squeezed, and then the column
that chooses is worth less than the picture.

The manual fold (`Tab`, ADR 0055 §6) keeps working on both columns and is
unchanged: hiding the controls deliberately is a different act from a window
too narrow to hold them.

### 2. The middle column declares no minimum

`min-width: 0px`, the symmetric idiom to the `max-width: 100000px` the same
column already carries, and for the symmetric reason: in Slint 1.13 an
explicit constraint is the only way to override the one computed from the
content. A column beside a fixed-width panel must be able to yield space; it
is the only participant in that row that can.

This is what actually keeps the settings panel inside the window at every
width, the fold in §1 being what keeps the *viewer* worth having.

### 3. What must not vanish with the space is a **tool**

Once the column can shrink, the toolbar is what no longer fits, and a clipped
toolbar hides *Brush* behind the panel edge with nothing to say it was ever
there. The row scrolls sideways instead: the **wheel** moves it, and a
**chevron** appears at whichever end still has something behind it.

Two things about the container, both found by building the wrong one first.

A `Flickable`'s **panning** does not work here: it pans when *it* receives
the press, and in this row every press lands on a chip's own `TouchArea`, so
dragging the toolbar did nothing at all and only a horizontal wheel — which
most mice do not have — ever moved it. So the row is scrolled by hand:
`viewport-x` is driven from the wheel and from the chevrons, through a
`TouchArea` that is the row's **parent** rather than its sibling — a chip is
then a descendant and still takes its click first, while the scroll it does
not handle bubbles up. A sibling would have received neither.

A `Flickable` is nevertheless what holds the row, for its **layout info**
rather than its behaviour: an ordinary element merges its children's
constraints, so a plain clipping `Rectangle` would let the ten chips go on
reporting their ~900px minimum through it — which is the very thing §2 is
trying to be rid of. A `Flickable` reports no minimum of its own, which is
what it is for.

The chevrons are buttons the size of the chips beside them, not a scrollbar:
a 24px row carrying a 4px bar asks the user to grab something thinner than
what it scrolls, on exactly the screens least able to afford precision.

### 4. Out of scope

**A responsive settings panel.** Narrowing the 300px panel, reflowing sliders
onto two lines, or hiding groups below a width: [ADR 0054](0054-first-run-and-basic-mode.md)
§2 settled that a setting is one line — name, track, value — and that the
whole tonal set fitting on one screen is what makes the panel read as one
thing. A second layout for the same panel is a second design to keep true.

**A second window for the photograph.** Real (both references have one), and
a different decision with its own cost; a narrow window must work on its own
first.

## Consequences

* `DevelopPanel` gains one `in property <bool> left-panel-folded`, read
  beside the existing `side-panels-hidden`. Nothing else about the column
  changes: it is the same component, mounted or not.
* `StudioWindow`'s `refresh-fold()` sets one more property from the same
  handler; no new event source, no new state to keep in step.
* The toolbar's scroller changes no behaviour where the row fits: with
  nothing hidden there is no chevron, and the wheel is declined so the panel
  behind it keeps scrolling as before.
* Nothing reaches Rust: this is interface state throughout (ADR 0045 §2), no
  pixel changes, and no revision, preset or golden case is touched.

## Alternatives rejected

* **Icons instead of labelled chips in the toolbar.** It would buy the width
  back, and spend the discoverability that made them labels — the toolbar is
  where a beginner learns what Develop can do. [ADR 0091](0091-white-balance-picker.md)
  drew the eyedropper as a pictogram precisely because a picker *is* a
  pictogram everywhere; *Reshape* and *Keystone* are not.
* **Raising `min-width` to 1400px.** Honest, and it makes the application
  refuse a 1366x768 laptop outright rather than serve it a smaller Develop.
* **Wrapping the toolbar onto two rows.** Slint 1.13 has no flow layout, so
  it would mean a hand-computed split, and the honest version of it (a fixed
  grouping, always the same two rows) costs the viewer 24px of height at
  exactly the widths where the viewer is already the thing being squeezed.
  A row that scrolls costs nothing until it is scrolled.
* **Folding the settings panel and offering it back on hover.** A panel that
  appears when the pointer nears the edge is a mode with no state visible
  anywhere, and it would put every slider behind a gesture on exactly the
  screens where gestures are most cramped.
