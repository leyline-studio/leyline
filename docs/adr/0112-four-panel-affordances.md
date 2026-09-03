# ADR 0112 — Four affordances the develop panel was missing

**Status:** Accepted — 2026-09

## Context

A screen-against-screen comparison with a camera maker's own RAW application,
run on 2026-09-02, found the two panels structurally alike — three columns, a
tool stack on the right, an histogram above the shooting data, sliders of the
same shape — and Leyline's own slider ahead on two counts it had already
decided (double-click back to neutral, [ADR 0054](0054-develop-panel-layout.md);
live preview during the drag, [ADR 0074](0074-live-slider-preview.md)).

What the comparison did find were **four small affordances**, none of which
needs a decision about rendering, and each of which is used every session:

1. a slider's number cannot be **typed**, only dragged;
2. there is no **R/G/B readout** under the pointer;
3. zoom is a **switch** (fit or 100 %), not a number;
4. nothing **resets a group** — the double-click resets one slider.

They are grouped into one ADR because they are one decision: *the panel is
finished enough that its remaining gaps are ergonomic, and ergonomics is worth
a slice of its own.* Two heavier findings from the same comparison are
**refused here rather than deferred silently** — see the last section.

## Decision

### 1. A number you can type

`EditSlider`'s value becomes an editable field. It commits on **Enter or on
losing focus**, is clamped to the slider's own range, and honours the
slider's `decimals` — which stops being "0 or 2" and becomes the real number
of decimals, so a coefficient measured in hundredths of a percent
([ADR 0111](0111-adaptive-chromatic-aberration.md)) can be typed as
`0.034` rather than rounded to `0.03` by the display.

Text that is not a number **reverts** to the current value rather than
resolving to zero: a slider is a quantity, and the null answer to "what did
you mean by *abc*" is the value that was already there.

The field is the same one widget every panel already uses, so this lands on
every slider in the application at once — which is the reason it is a widget
change and not a control of its own.

### 2. The value under the pointer

An `R G B` readout at the foot of the develop viewer, following the pointer,
in the 0–255 of the rendered image — what the pixel *is* after development,
which is the question a readout answers.

It reads **the render already in memory**: the `Rgb8` the panel just turned
into a displayed image is kept beside it, so the readout costs no second
decode, no second render, and no file access. It samples the unpainted
render, so a mask overlay's red tint never leaks into a number.

It follows the pointer in **Select mode**. Every drawing tool puts its own
touch area over the viewer, and the events belong to the tool while it is
active — which is also when nobody is reading pixel values.

### 3. Zoom that is a number, not a switch

Continuous zoom between **fit** and **8×**, on `Ctrl` + wheel, with the
percentage shown. The existing `100% Z` shortcut stays exactly as it is: it
is the one zoom level that means something precise (one image pixel per
screen pixel), and a photographer who wants it wants it in one gesture, not
in eleven wheel notches.

`Ctrl` + wheel rather than the bare wheel, because the bare wheel belongs to
the panel it is over — and because it is the gesture the rest of the world
uses.

The zoom **law** — the multiplier per notch, the floor, the ceiling, the way a
percentage is written — lives in one place, a Slint global, so a second viewer
that ever zooms consumes it rather than inventing its own.

**Only the develop viewer zooms continuously.** The loupe and the compare view
keep their fit / 100 % switch: they share `ZoomableImage`, whose panning is
built around that switch, and converting it would have meant reworking three
views to finish one. Named as a limit rather than left to be discovered — the
gesture works where a correction is judged, and nowhere else yet.

> **Limit lifted, 2026-09-03.** `ZoomableImage` now takes a `scale` and asks
> its parent for a new one on `Ctrl` + wheel, exactly as `panned` already
> asked for a new centre — which is what made it cheap in the end: the widget
> owned neither piece of state, so the compare view's two halves share the
> scale the way they already shared the centre. All three viewers zoom
> continuously, on the same law, and the sentence above stands only as the
> record of what was true for a day.

What the zoom magnifies is **the preview the panel already holds**, not a
fresh full-resolution render. That was already true of the 100 % button before
this ADR, and it is why 100 % has always meant "one preview pixel per screen
pixel". Rendering at full resolution when the zoom goes past the proxy's own
scale is a real feature with a real cost, and it is a decision of its own.

**Picking a tool still returns to fit**, unchanged: the tools' geometry is
computed against the fitted image, and this ADR does not touch that.

### 4. Reset a group

Each group header gains a `⟲`. One click, **one revision**, and only the
fields that group owns — the same fields the preset machinery already names
for it, extended in Studio to the groups the model has no name for (the tone
curve, the mixer, the local adjustments).

The mapping lives in `crate::develop`, the module that already holds "what
does this slider do", free of any Slint type and therefore testable: for each
group, resetting it must set exactly its own fields to neutral and leave
every other field of a fully-loaded revision untouched. That test is the
decision's real content — a reset that quietly takes a neighbour with it is
worse than no reset at all.

An **empty group is not a reset**: a group already at neutral writes no
revision, so the history does not fill with entries that changed nothing.

## Consequences

* One widget change reaches every slider in the application; one header change
  reaches the thirteen groups of the develop panel that hold settings (the
  other three are a view toggle and two lists).
* Studio keeps one preview-sized `Rgb8` per developed photo. That buffer
  already existed one line earlier in both code paths; what is new is that it
  is not dropped immediately.
* No engine surface, no stage, no `settings_json` field, no migration. The
  reset writes ordinary parameters through an ordinary session, so it undoes
  like anything else.
* The panel's interaction rules ([ADR 0054](0054-develop-panel-layout.md))
  gain two: a number can be typed, and a group can be reset. The double-click
  on a slider keeps meaning what it meant.

## Refused, in writing

* **A checkbox per tool** (disable *Curves* without zeroing it, to compare
  tool by tool). This is not a button, it is a model question: either an
  `enabled` per family inside `settings_json` — a stored flag that changes no
  pixel when true and is one more thing every preset, every reprocess and
  every golden has to carry — or a *preview* that renders without one stage,
  which is not stored at all and is much closer to what a photographer
  actually wants. The second is the better answer and it is a slice of its
  own, on top of the before/after comparison that already exists. Named here,
  not built here.
* **A levels tool** (numeric black/gamma/white under the histogram). Leyline
  has the histogram and the clipping indicators
  ([ADR 0092](0092-clipping-indicators.md)); levels is a tone operator wearing
  a display's clothes, and it would need a stage, a version and a golden case.
  It is a develop decision, not an ergonomic one, and it does not belong in
  this ADR.
* **Typing into a slider by double-clicking it.** Rejected: the double-click
  already means "back to neutral" (ADR 0054 §2), and overloading the most
  destructive gesture on the panel with the most precise one is how a
  photographer loses a value they were adjusting.
