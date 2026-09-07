# ADR 0136 — What is not adjustable, and what should have been remembered

**Status:** Accepted — 2026-09

## Context

The ergonomics survey's last item was *customisation*, and it listed four
absences: no shortcut remapping, no panel reordering, no solo mode, no module
search. Darktable has all four; Lightroom has two.

Taken one at a time, three of them turn out to be cures for illnesses this
interface does not have, and the fourth is expensive in a way that follows
directly from decisions already made. What the survey missed is the thing
that actually is missing, and it is smaller and duller than any of them: the
interface forgets three ordinary settings every time it is closed.

So this ADR refuses four things by name, and builds the one nobody listed.

## Decision

### 1. Shortcut remapping is refused, and the number is the reason

Counted on the source: **88 translated strings in the interface contain a
key**. `Develop    D`, `Show clipped highlights    J`, `Copy Settings…
Ctrl+Shift+C`, `Thirds    O` — in menu titles ([ADR 0055](0055-library-navigation.md)
§6), in hints ([ADR 0127](0127-hints-on-wordless-controls.md) §2), on chips,
and on the shortcuts card.

That is not an accident: putting the key next to the thing it does is how this
interface teaches its own keyboard, and three ADRs of this pass added to it.
Remapping makes every one of those 88 strings a **variable**, and a printed key
that no longer matches the binding is worse than no printed key at all — it is
an interface that lies in eighty-eight places.

There is a second cost, less obvious: those strings are *translated*. A
translator today sees `Develop    D` and translates one word. With a
substitution they would see `Develop    {}`, which is a worse string to
translate and one whose alignment nobody can check.

None of that makes remapping wrong. It makes it a feature whose price is a
rework of how the interface names its keys, and that price should be paid when
somebody names a binding they cannot live with — not on the strength of a
competitor's feature list. **Revisit when** someone reports a conflict, or when
one-handed use is asked for; both are real, and neither has been asked.

### 2. Solo mode is refused, because the panel already behaves that way

Darktable needs solo mode because its modules open themselves: a group tab
expands several at once, and the panel becomes a scroll.

Ours does the opposite. The develop panel ships with **one group open** —
Basic — and every other one closed ([ADR 0128](0128-remembered-interface-state.md)
§2's defaults), and the fold state is remembered. A photographer already opens
one group at a time; solo mode would automate a discipline the interface
already imposes.

Building it would mean *closing groups the user deliberately left open*, which
is the one thing about the current arrangement that is definitely right.

### 3. Reordering and hiding panels is refused, because Basic/Full already
### answers the question

The problem both gestures solve is *too many panels*.
[ADR 0054](0054-first-run-and-basic-mode.md) §2 answers that problem, once, with
a curated two-level answer: Basic shows the four groups that matter on every
photograph, Full shows all fifteen.

A per-user order and a per-user hidden set would be a **third mechanism over
one problem**, and the third mechanism is the one whose result nobody can
support, document or screenshot: every ADR that says "Basic's Full-mode block"
would name a place that may not exist for that user.

One thing worth recording, because it would have been a good argument and is
false: the panel order is **not** the pipeline order. Checked against the stage
ranks — `lens` is rank 20 and sits fourth in the panel, `lut` is rank 165 and
sits third. The panel is ordered by how often a photographer reaches for each
group, which is a curated judgement and not a mechanical fact. The refusal
above stands on Basic/Full, not on a correspondence that does not exist.

### 4. Module search is refused, at fifteen

Darktable's search box exists because it has some seventy modules across five
tabs. Develop has **fifteen named groups in one column**, all of which fit on
one screen in Full mode.

A search box over fifteen items is a scrollbar with extra steps.

### 5. What was actually missing: the interface finishes remembering itself

ADR 0128 taught Studio to remember how its panels were folded, and stopped
there. Three ordinary settings are still reset on every launch:

* **The thumbnail size.** A hard-coded `176px`, set again by hand every
  session by anyone whose screen or eyes disagree with it.
* **The side panels, folded by hand.** `Tab` folds them, ADR 0130 §2 gave
  them a handle back, and the next launch undoes the decision.
* **The sort order.** Cycled through eight, and back to the first one
  tomorrow.

These are the customisation that a photographer actually performs — not a
rebound key, but a window that opens the way it was left. They go in
`preferences.json` beside `develop_view`, under ADR 0078 §5's rule that state
of the same scope and lifetime belongs in the same file, and they stay out of
the Preferences dialog under §1's third condition: each has a natural place in
the surface it governs.

The sort is stored **by name, not by index**, unlike the panel bitfields. Those
numbers are opaque to Rust and the panel decodes them; this one Rust decodes
itself, and an index would silently become a different sort the day a ninth is
inserted rather than appended — the same reasoning
[ADR 0134](0134-keywords-in-the-interface.md) §6 applied to keyword categories.

### 6. Out of scope

* **Remembering the filters.** A rating filter left on is how a library
  appears empty tomorrow with no visible cause. Folds and sizes describe the
  *window*; a filter describes *which photographs exist*, and that must start
  from the whole library.
* **Remembering the selected folder or collection.** Same rule.
* **A layout editor of any kind.** §3.

## Consequences

* Four features are now refused in writing, each with a condition for
  revisiting rather than a verdict. That is the shape ADR 0101 established for
  refused modules, and it is the useful half of a refusal: the next person to
  ask gets the reasoning rather than a "no".
* `preferences.json` gains three fields. It is now ten, which is close to the
  point where the file wants a section per subject; recorded so the eleventh
  prompts the question rather than being appended by reflex.
* The count in §1 is a measurement of today. If the interface ever stops
  printing keys beside actions, the argument weakens with it — which is the
  right way round.

## Alternatives rejected

* **Remapping only some keys** — the classement row, say. An arbitrary
  boundary that would have to be explained, and a user whose conflict is
  outside it is no better off.
* **Solo mode as an option**, off by default. An option is not free: it is a
  second behaviour to hold in mind, for a gesture the interface already
  performs by default.
* **Storing the thumbnail size per library.** It is a property of the screen
  and of the eyes in front of it, not of the photographs.
