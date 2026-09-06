# ADR 0129 — Undo in the library

**Status:** Accepted — 2026-09

## Context

`Ctrl+Z` works in Develop and nowhere else. The reason is historical rather
than decided: Develop's undo is not an undo stack at all, it is
[ADR 0007](0007-git-develop-model.md)'s revision graph walked backwards — the
history existed, so the gesture was free.

The library has no such graph, and so has no undo at all. Everything a
photographer does while sorting is one-way:

| Gesture | What it writes | Undo today |
| :--- | :--- | :--- |
| `0`–`5`, `6`–`9`, `P`/`X`/`U` on a selection | rating, colour label, flag | none |
| a keyword typed in the detail panel | `asset_keywords` | none |
| `B` / `Shift+B` | collection membership | none |
| a title, caption, creator or copyright | the authored description (ADR 0099) | none |

These are exactly the gestures made fastest and in bulk. Rating a run of a
hundred photographs three stars, noticing that the selection was not the one
intended, and having no way back, is a plain defect — and the reference
application undoes all four.

Two things that look like the same problem and are not:

* **Removal** ([ADR 0060](0060-asset-removal.md)) already has a confirmation
  dialog, and a file in the system trash is not the catalog's to restore.
* **Rename** ([ADR 0100](0100-file-renaming.md)) moves files on disk, and the
  engine verb is template-driven — see §4.

## Decision

### 1. An edit is two snapshots, and undo and redo are one mechanism

Each of the four operations *replaces* catalog state that can be read back
first. So Studio records, for each edit, the state before and the state
after, both in the same shape, and both idempotent to apply. Undo puts back
the first, redo puts back the second, and there is one code path.

That symmetry is what makes redo cost nothing extra here, and redo matters
for the same reason undo does: the gesture that undoes a hundred ratings
should be as reversible as the one that made them.

### 2. It lives in Studio, and the engine gains nothing

No new engine verb, no schema change, no event. The snapshots are taken from
values the client already has in hand — the grid rows carry rating, label and
flag; the detail panel carries the description it is about to overwrite — and
they are put back through the same public calls that made the change.

That boundary is the decision, not an implementation detail. An undo stack in
the engine would be a second history beside the revision graph, with its own
opinion about what a "state" is, shared by three clients that do not agree
about what a selection is.

Fifty edits deep, oldest dropped. An undo stack is for the mistake one
notices, and the mistake one notices is within the last handful of gestures;
a stack deep enough to hold the afternoon invites the belief that it holds
the morning.

### 3. `Ctrl+Z` means "undo here"

In Develop it walks the revision graph, as before. In the library it steps
this stack. The menu says which: **Library ▸ Undo** and **Redo** join the
Library menu, where they are reachable exactly when they apply — the Develop
menu is disabled outside Develop, which is why its `Ctrl+Z` could never have
served the grid.

The item names what it will undo — *Undo Rating*, *Undo Keyword* — from a
closed set of words Rust hands over and Slint turns into a sentence, the
arrangement ADR 0078 §3 requires so that every string a user reads is
translatable.

### 4. What is deliberately **not** undoable

* **Renaming files** (ADR 0100). The engine's `rename` takes a *template*,
  and restoring `N` distinct former names is not a thing a template can
  express; the honest fix is a new engine verb that renames one asset to one
  literal name, which is a decision about the engine's surface and not about
  undo. Until then the operation keeps what it has: it refuses to overwrite,
  and it reports per asset what it did.
* **Removal from the catalog and deletion from disk** (ADR 0060). The first
  is confirmed; the second is not the catalog's to reverse.
* **Import.** It is a job, not an edit — it is undone by removing what it
  brought in, which is the previous point.
* **Develop settings pasted onto a selection.** Every photograph that
  received them kept its own history, and that history is where those edits
  are undone, one photograph at a time. An undo entry that quietly rewrote
  fifty revision graphs would be a different and much larger promise.

### 5. Out of scope

**A visible history panel for the library**, the way Develop has one. What
this fixes is a missing gesture; a panel is a feature, and it would need an
answer for what a *named* point in a library's history even is.

## Consequences

* A new `undo.rs` in Studio: two enums, a bounded stack, no Slint types, and
  the whole of it unit-testable without a catalog.
* Four call sites gain a snapshot taken immediately before they act. Each is
  three lines, and reads as what it is.
* `LibraryState` gains two booleans, two words and two callbacks.
* Nothing crosses into the engine, so `tests/surface.rs` — the guard that
  keeps the SDK a pure façade — is untouched.
* The stack lives as long as the window. Opening another library relaunches
  the application, so there is no moment at which a stack could refer to a
  catalog that is no longer open.

## Alternatives rejected

* **Undo in the engine, shared by the three clients.** §2.
* **Undo by re-reading the catalog rather than snapshotting.** It would mean
  a read per undo, of state that may have changed for other reasons since —
  and "put it back the way it was" would silently become "put it back the way
  it is now".
* **A single undo, no redo.** The mechanism that gives one gives the other;
  refusing redo would be a decision to make the interface worse to save
  nothing.
* **Coalescing successive ratings into one undo entry**, the way the engine
  coalesces develop edits (ADR 0120). Rating five photographs in five
  keystrokes is five decisions, not one gesture interrupted; the develop case
  coalesces a *drag*, which is genuinely one.
