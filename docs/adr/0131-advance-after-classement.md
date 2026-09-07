# ADR 0131 — Advancing after a rating

**Status:** Accepted — 2026-09

## Context

Culling is the one task a photographer does at a rate of several photographs
a *second*, and it is the only task in Studio where the interface asks for two
gestures per photograph instead of one: rate, then move. Eight hundred frames
off a weekend is eight hundred presses of `→` that carry no decision.

Lightroom removed the second gesture twenty years ago: with **Auto Advance**
on, rating, labelling or flagging a photograph selects the next one. It is not
a convenience — it is what turns rating into a *rhythm*, where the left hand
stays on the number row and the photographs come past on their own.

Leyline has every piece: `GridState.classify` applies the classement,
`GridState.selected` moves the focus, and `browser.slint`'s
`followed-selection` already scrolls whatever the selection becomes into view
([ADR 0124](0124-develop-left-column.md)'s doing, for develop's arrows). What
is missing is the sentence joining them, and the four decisions about *when*
it must not be said.

## Decision

### 1. One photograph, one classement, one step

After a classement — a rating (`0`–`5`), a colour label (`6`–`9`) or a flag
(`P`/`X`/`U`) — the selection moves to the next photograph in the current
sort order.

Two conditions, both of them refusals:

* **Only when exactly one photograph was classed.** Rating a multi-selection
  of fifty is one decision about fifty photographs, and "the next one" after
  it names nothing. The selection stays where it is. This falls out of the
  rule `selected_indices` already implements — a multi-selection acts as a
  batch — so it is read from `App::multi_selected` and not guessed.
* **Only when the classement succeeded.** The advance is appended after the
  catalog write, the grid reload *and* the undo push, so a photograph whose
  rating did not land is still the one on screen. Nothing moves past an error.

At the last photograph it stops there. No wrap: the end of the run is
information, and a wrap would silently start a second pass over frames already
judged.

### 2. It acts in the grid and the loupe, and nowhere else

Those two are the culling surfaces, and the loupe is the more important of
them — it is where the decision is actually made, since a decision needs the
photograph large.

The other four views each have a reason of their own:

* **Compare** and **Survey** ([ADR 0057](0057-compare-and-survey.md)) are
  views *whose content is the selection*. Advancing would change what is on
  screen out from under the comparison being made — the gesture would rate
  this photograph and then remove it.
* **Develop** holds an `EditSession`, and that session is a claim on the
  revision ([ADR 0120](0120-edit-session-claim.md)). Moving the selection
  there means closing one session and opening another on every keypress, and
  a rating given in Develop is a judgment about the photograph one is working
  on, not a step through a shoot.
* **Map** ([ADR 0040](0040-gps-map-view.md)): the selection is a place, and
  the next photograph in sort order is very often nowhere near it.

Read from the view flags rather than from where the keystroke came from: the
Photo menu's *Rate ▸ 3 stars* and the grid's context menu go through the same
`classify` callback as the number row, and all three mean the same thing.

### 3. Not Caps Lock — a toggle in the menu that holds the actions it changes

Lightroom binds Auto Advance to **Caps Lock**, and it is the worst part of an
otherwise excellent feature: it is a state of the *keyboard*, not of the
application, it is invisible unless one looks at a lamp on the hardware, and
it changes what every other key in the program types while it is on.

Studio uses a checkable **Photo ▸ Advance after rating**. The Photo menu is
where every classement action already lives, so the switch is next to the
things it modifies, and its state is legible by opening the menu it is in.

It is **off** until someone asks for it. A selection that moves on its own is
the single most surprising thing an interface can do to a user who did not
request it, and the discovery path is the same menu the ratings are in.

### 4. The interface says it is on, in both places it acts

A checkmark inside a closed menu is not an affordance during a culling
session. Both toolbars — the grid's and the loupe's — carry a chip while
advance is on, which is also the one-click way back out without leaving the
photographs.

The chip is *only* shown when the setting is on. An always-present chip would
buy discoverability with permanent clutter on the two surfaces that most need
to be quiet; the menu is where one goes looking for a mode, and the chip is
how one is reminded of it.

### 5. It is remembered, and it never appears in Preferences

Stored in `preferences.json` as `advance_after_classement`, absent meaning
off — the shape [ADR 0128](0128-remembered-interface-state.md) §3 gave
`develop_view`, for the same reason: a way of working, kept across launches.

And it is excluded from the Preferences dialog by
[ADR 0078](0078-preferences-panel.md) §1's third condition — a setting with a
natural place in the surface it governs does not belong there. The Photo menu
*is* that natural place, exactly as the Basic/Full switch's place is the head
of the panel it governs.

### 6. Out of scope

* **Advancing after anything but a classement.** Applying a preset, pasting
  settings or writing a keyword are not passes through a shoot, and a
  selection that moves after a preset would make a second preset land on the
  wrong photograph.
* **Advance-and-open.** Rating in the grid does not open the loupe, and rating
  in the loupe does not leave it. The view is the user's choice, not the
  gesture's.
* **A separate "advance direction".** The sort order is the order, and it is
  already chosen in the filter bar.
* **Auto-advance in the develop filmstrip.** §2 names the blocker; it is not a
  key binding away, it is a session lifetime question.

## Consequences

* `wire_classify` gains a tail, and one that reads its conditions from state
  it does not own (`multi_selected`, the view flags, the preference). That is
  the right place regardless: the advance is part of what a classement *is*
  when the mode is on, and putting it in the six Slint call sites instead
  would give six chances to forget it.
* Rating past the end of the loaded window of rows relies on the same
  arrangement the arrow keys already rely on — the window carries overscan,
  and `followed-selection` scrolls the new focus into view, which is what
  refills it. No new guarantee is introduced, and none is needed.
* `MenuRow` gains a checkable sibling, `MenuToggleRow`. The menu bar has had
  no toggle in it until now; it will have more when the customisation
  gap is closed, and the tick column belongs in the widget rather than in a
  caller's string.

## Alternatives rejected

* **Caps Lock, as Lightroom binds it.** §3. Slint's `KeyEvent` does not
  report it either, so implementing it would mean reading the keyboard's lock
  state from the platform — a native call, for a worse interaction.
* **A modifier held while rating** (e.g. `Shift+3` rates and advances). It
  makes the fast path the *harder* one to type, which is backwards: the whole
  point is that the hand stays on the number row.
* **Advancing on a multi-selection, to the photograph after the last of
  them.** Defensible, and rejected because it makes one gesture mean two
  different things depending on a state that is not visible from the keyboard.
* **On by default.** §3. Every user who did not want it would meet it as a
  bug, and the ones who do want it look for it in the menu the ratings are in.
