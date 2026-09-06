# ADR 0126 — The precision of a slider

**Status:** Accepted — 2026-09

## Context

`EditSlider` ([ADR 0054](0054-first-run-and-basic-mode.md) §2, extended by
[ADR 0112](0112-four-panel-affordances.md) §1) is the control the whole of
Develop is made of. It carries three gestures: drag the track, double-click
to return to neutral, type the number. Measured against what the two
reference applications give the same control, two things are missing and one
is broken.

**The track is 110px wide.** In a 300px panel, with an 88px label column and
a 46px value column, that is what is left. Exposure runs over ±5 EV, so one
screen pixel is 0.09 EV — and the drag is *absolute*: the value follows the
pointer's position in the track, not its movement, so there is no way to ask
for a smaller increment than one pixel. Both references solve this with
movement rather than position: dragging further than the track is worth,
optionally with a modifier that scales the movement down.

**The wheel does nothing.** Over a slider it scrolls the panel, which is the
Lightroom behaviour and not darktable's — there the wheel over any control
is *the* way to adjust it, and it is the single most-cited reason its users
say the interface is fast.

**And a click on the track does nothing at all.** The drag begins on
`moved`, so pressing on the track without moving the pointer sets no value
and releases with nothing changed. That is not a decision anyone took; it is
what the handler happens to do. A dead gesture in the middle of the most-used
control in the application.

Typing the number, added by ADR 0112 §1, is what has been standing in for all
three, and it is the right answer for *8.5 EV exactly* and the wrong one for
*a little less than that*.

## Decision

### 1. A press sets the value; the drag that follows is relative to it

Pressing anywhere on the track moves the handle there — the gesture everyone
already has — and that position becomes the **anchor**. Movement afterwards
is counted from the anchor, not read off the pointer's absolute position.

At normal speed the two are the same thing, which is deliberate: nothing
about the existing gesture changes. What it buys is §2.

### 2. `Shift` divides the movement by five

With `Shift` held, the same hand movement covers a fifth of the range from
the anchor, so the track is worth 550px instead of 110 and exposure resolves
to 0.018 EV per pixel. `Shift` is checked continuously rather than at press
time: one can start coarse, press `Shift`, and land the value without
releasing.

Five, not two and not twenty: a factor small enough that the handle still
visibly follows the hand, and large enough that the value column stops
skipping numbers.

### 3. The wheel adjusts the value, over the **track** and nowhere else

One notch is **1 %** of the slider's range, `Shift`-notch is 0.2 %, each
rounded to the slider's own `decimals` so the number that lands is a number
the slider can hold.

Restricted to the track on purpose. The panel is a `Flickable` and the wheel
is how it is read; a wheel that edited whatever the pointer happened to be
over would turn scrolling past a group into an edit of it. The track is a
110x22px target one is on deliberately — over the label, the value, or the
gap between rows, the wheel still scrolls the panel.

There is no release to commit on, so each notch commits. That is affordable
only because of something already built: successive edits of the same
setting inside the amendment window coalesce into one revision
([ADR 0120](0120-edit-session-claim.md) records the 2 s figure), so a turn of
the wheel writes one revision, not thirty.

### 4. Arrow keys are refused, and the reason is not effort

`←` and `→` move to the previous and next photograph in Develop
([ADR 0055](0055-library-navigation.md) §4), from anywhere in the module. A
focused slider that consumed them would make the primary navigation depend on
where the last click landed — a photographer arrowing through a shoot would,
after touching a slider, silently start editing it instead. The alternative
is a focus ring the panel does not otherwise have, on fifty controls, to
serve a gesture the wheel already serves better.

### 5. Out of scope

**A wider track.** The panel is 300px and a setting is one line (ADR 0054
§2); widening the track means narrowing the label or the value, and both are
read as columns.

**Right-click for a numeric popup** (darktable's). We have the number itself,
always visible and always typeable — ADR 0112 §1 chose the better of the two.

## Consequences

* `EditSlider` gains an anchor pair (`anchor-value`, `anchor-x`) and a
  `scroll-event`; `previewing` and `edited` keep their exact meaning — the
  first shows, the second commits (ADR 0074 §1).
* `MiniSlider` is untouched. It sets a thumbnail size, where the feedback is
  the effect itself and a wheel over the grid means scrolling the grid.
* No Rust changes, no new callback, no state that crosses to the engine.
* The gesture that existed still works identically, which is what makes this
  safe to put under fifty controls at once.

## Alternatives rejected

* **Acceleration instead of a modifier** (fast movement covers more range).
  It makes the same gesture mean two things depending on how quickly it is
  performed, and the value one lands on stops being reproducible.
* **The wheel over the whole row.** Simpler to implement and one accidental
  edit per scroll past a group.
* **Keeping absolute dragging and widening the range mapping.** Any mapping
  that is a function of pointer position alone is bounded by the track's
  width in pixels; the fix has to be movement.
