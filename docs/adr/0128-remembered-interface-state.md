# ADR 0128 — The interface comes back as it was left

**Status:** Accepted — 2026-09

## Context

[ADR 0054](0054-first-run-and-basic-mode.md) §3 decided that Develop always
opens in Basic mode, and gave the reason:

> The opposite — remembering the last mode — would require storing a
> preference, hence a configuration file the project does not have, and this
> decision does not justify creating one.

That reason expired one month later. [ADR 0078](0078-preferences-panel.md)
created `preferences.json`, and it has since taken the language, the update
consent and the two viewer grounds. The clause "a configuration file the
project does not have" is simply no longer true, and the decision that rests
on it has never been re-examined.

What it costs, per session: Develop opens in Basic; the photographer who
works in Full clicks *Full*; the fifteen accordion groups are closed again,
so the three they actually use are opened again; the left column's Versions
and History sections are closed again. Both reference applications remember
all of it, and one of them remembers the scroll position too.

The second half of §3's argument — *an experienced user clicks once per
session; a beginner has no "once" to give* — survives the file question and
is answered on its own terms in §2 below.

## Decision

### 1. Remembered, and therefore stored — but never a *preference*

The Develop mode, the fifteen setting groups and the four left-column
sections are written to `preferences.json` and read back at launch.

They are **not** preferences, and they never appear in the Preferences
dialog. ADR 0078 §1's admission rule excludes them by its first condition —
a setting there must concern the installation, *not a view* — and by its
third: their natural place is the panel, where the *Basic | Full* chips and
the group headings already are, and where they will stay.

They live in that file for the reason ADR 0078 §5 already applied to
`launches` and `last_update_check`, which are not settings either: **state of
the same scope and lifetime belongs in the same file**, and a second file for
the beauty of the classification would buy nothing.

### 2. The first launch still opens in Basic

ADR 0054 §3's real argument is about the *first* session, and it is
untouched: with nothing stored, Develop opens in Basic with the Basic group
alone expanded — exactly the interface that ADR shipped. What changes is only
the second session onwards, where there *is* an answer to remember and
"beginner" is no longer the right assumption.

### 3. Rust stores a number it does not read

One value per view — a bitfield the panel composes and decomposes, bit 0
being Full mode and the rest its groups — and Rust puts it in the file
verbatim.

This is what keeps [ADR 0045](0045-studio-ui-modularisation.md) §2 true
rather than merely bypassed. That rule says a global carries only what
crosses the Rust ↔ UI boundary; persistence *is* such a crossing, so the
crossing is legitimate — but it should be as narrow as the need. Rust
learning that a group is named `geometry`, or that there are fifteen of them,
would make every future group a two-sided change. One opaque value per view
means adding a sixteenth group costs one line, in Slint, in the panel that
owns it.

A number and **not** a list of names, which was the first shape tried:
Slint 1.13 has no string search. `is-float`, `to-float`, `is-empty`,
`character-count` and the two case conversions are the whole of its string
surface, so a panel could write `"basic,detail"` and would have no way to
read it back.

**-1 means nothing is stored**, and it is not the same as 0 — 0 is Basic with
every group closed, which is a state someone can deliberately be in.

ADR 0045 §2 names the `expand-*` booleans as its example of panel-private
state; that list is amended here, and the rule it illustrates is not.

### 4. What is deliberately **not** remembered

* **The panel fold** (`Tab`, [ADR 0055](0055-library-navigation.md) §6) and
  the automatic narrow fold ([ADR 0125](0125-narrow-window-develop.md)).
  Folding is a gesture about *this moment* — one wants the photograph large
  now — and an application that reopened with its panels hidden would look
  broken to the person who hid them yesterday.
* **The window's size and position.** A different decision, about the window
  rather than about the module, and one with real failure modes of its own
  (a window restored onto a screen that is no longer connected).
* **The scroll position** of any panel. It is a position in a list whose
  contents may have changed.
* **The active tool.** Reopening Develop with the brush selected means the
  first click on the photograph paints.
* **Anything per photograph.** These are properties of how *this
  photographer* works, not of a picture; a library carried to another machine
  carries no interface state, and that is correct.

## Consequences

* `Preferences` gains two optional integers, absent by default, so an existing
  `preferences.json` stays valid and a first launch is unchanged.
* `PreferencesState` gains the pair plus two callbacks; the write goes through
  the existing `PreferencesFile::update`, which saves on every change — there
  is no "apply" step anywhere in this application (ADR 0078 §2) and none is
  introduced here.
* Writing on every accordion click is a `serde_json` serialisation and an
  atomic rename of a file under 1 KB, on a gesture a human performs a few
  times a minute.
* One trap, found by running the build rather than by reading it: `-1` must
  never reach the bit arithmetic. The first version tested it at two of the
  seventeen call sites, and `Math.mod` of a negative number came back as a
  value the comparison read as *true* — so "nothing stored" decoded as
  "everything expanded", and a first launch opened Develop with all fifteen
  groups unfolded, which is precisely the screen ADR 0054 exists to prevent.
  The test now lives inside the decoding function, where no call site can
  forget it.
* No pixel, no revision, no stored photographic format: the prohibition ADR
  0078 §1 calls non-negotiable — no preference may change a pixel — holds
  trivially, since none of this reaches the engine at all.

## Alternatives rejected

* **A separate `ui_state.json`.** Two files with one lifetime, and the
  precedent in ADR 0078 §5 points the other way.
* **Remembering per library.** The mode one works in is a property of the
  person, not of the catalog; and it would put interface state inside the
  thing that gets copied to another machine.
* **Fifteen booleans in a global.** It works, and it makes every new group a
  change on both sides of the boundary for no gain — §3.
* **Remembering the mode but not the groups.** The complaint is about
  re-opening the same three groups every session; the mode alone is the
  smaller half of it.
