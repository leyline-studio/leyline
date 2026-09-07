# ADR 0134 — Keywords, in the interface this time

**Status:** Accepted — 2026-09

## Context

The catalog has had proper keywords since it was written: a tree with
`parent_id` and a materialized `path` (`docs/catalog.md` §22), an N:N join to
assets (§23), and a grid query that matches **a keyword or any of its
descendants** — filtering on *Nature* already returns the herons.

Studio exposes almost none of it. Counted on the running binary:

* Keywords appear in **one place**: a list of the focused photograph's own
  tags, in the metadata panel, with a text field under it.
* Tagging writes to `&[asset]` — the **focused photograph alone**. Select two
  hundred frames from a shoot and type a keyword, and one of them is tagged.
* The library's keyword **tree is invisible**. There is no way to see what
  keywords exist, how many photographs carry one, or to reach the photographs
  of a keyword the focused one does not have.
* There is no way to **delete or rename** a keyword. `create_keyword` exists;
  nothing removes one. A typo entered once is in the library for good.

The last two compound: a photographer types `Naure/Birds`, the level is
created silently, and neither the mistake nor its remedy is anywhere on
screen.

So the gap is not the hierarchy — that is built and works. It is that the
hierarchy has no surface, and that the one gesture the surface does have acts
on one photograph when the photographer has selected two hundred.

## Decision

### 1. Tagging acts on the selection

`add_keyword` and `remove_keyword` already take a slice of assets; Studio
passes one. It now passes the whole selection, by the same rule every other
batch action uses — `selected_indices`' rule: the multi-selection when there
is one, the focused photograph otherwise.

This is the item with the largest ratio of value to code in this ADR, and it
was a one-element array.

### 2. The tree gets a panel, under Collections

A *Keywords* section in the browser sidebar, beside Folders and Collections,
because it is the third way a library is navigated and the other two are
already there.

Each row carries the keyword's name at its depth, and **the number of
photographs in its subtree** — the count that matches what clicking it does,
since the query matches descendants. A row with children carries a chevron and
remembers whether it is open.

Three gestures, and each one is a different intention:

* **Clicking the name filters**, toggling like every other criterion in the
  filter bar. This is navigation.
* **A `+` on hover tags the selection** with that keyword. This is writing.
* **Dropping photographs on the row** does the same, reusing
  [ADR 0130](0130-direct-manipulation.md) §1's mechanism — the pointer is
  published in window coordinates and each row hit-tests it. `DragState` gains
  a *kind* beside its index, because two sorts of row are now droppable and an
  integer alone can no longer say which.

Separating filter from tag on one row is deliberate. Lightroom puts a checkbox
on the left of the name and the count on the right, and the checkbox means
*tag* while the name means *filter*; the two are a pixel apart and mean
opposite things. A `+` that appears only under the pointer says which half is
about to write.

### 3. Typing offers what exists before creating something new

The keyword field suggests as one types: every path in the tree containing
what has been typed, matched on **any level**, so `her` finds `Nature/Birds/
Heron` without typing the branch. Enter takes the highlighted suggestion when
there is one, and the typed text otherwise — which is still how a new keyword
is made, path and all.

That is the actual cure for `Naure/Birds`: the misspelling has no suggestion
behind it, and the correct one is one keystroke away.

### 4. A keyword can be renamed, and deleted

Both from the row's context menu, and both new to the catalog.

* **Renaming** changes one level and rewrites the `path` of the node and every
  descendant, in one transaction. The join to assets is by id, so nothing a
  photograph carries moves.
* **Deleting** is refused while the keyword has children. One deletes leaves,
  upward, which makes the destruction explicit — a recursive delete of
  *Nature* is a gesture whose consequence nobody can see at the moment of
  making it. It **untags every photograph** that carried it, and the
  confirmation says how many.

Deleting a keyword is not undoable, and the confirmation says so. The undo
stack ([ADR 0129](0129-library-undo.md) §4) holds edits to photographs;
restoring a branch of the hierarchy plus every tag on it is a different
mechanism, and one that ADR made a point of not pretending to have.

### 5. Keyword sets are refused, for now, with the reason

Lightroom's keyword sets — nine keywords on a pad, switchable between named
groups — are not built.

A set is a **shortcut layer over a list**, and until this ADR the list did not
exist. Building the accelerator for a panel nobody has used yet is designing
for a problem not yet met, which is the reasoning
[ADR 0084](0084-assisted-culling.md) and [ADR 0088](0088-auto-tone-and-black-and-white.md)
both applied.

Two more reasons, both specific:

* What a set actually solves is *finding* one keyword among hundreds. §3's
  suggestions and the panel's own rows answer that directly, and without a
  configuration step.
* The number row is spent — `1`–`5` rate, `6`–`9` label — so a set's
  shortcuts would have to be `Alt+1`…`Alt+9`: a binding nobody discovers, for
  a feature nobody has yet asked this project for.

Revisit when a real library makes the panel unnavigable despite §3, or when
somebody asks for it by name. Recorded here so that day starts from a
position rather than from scratch.

### 6. Out of scope

* **Moving a branch** (re-parenting *Birds* under *Wildlife*). Buildable —
  it is `rename`'s path rewrite with a different parent — and it is a
  different question: what happens to a photograph tagged with both ends.
* **Keyword synonyms and "export as"**, which are XMP concerns and belong
  with the sidecar work.
* **Tagging from the develop module.** Keywords are about the content of the
  photograph, and develop is about its rendering.

## Consequences

* Three new catalog verbs — `rename_keyword`, `delete_keyword`,
  `keyword_counts` — and no migration: the schema `docs/catalog.md` §22
  already describes was complete, and only the API over it was not.
* `DragState.target` becomes a pair (kind, index). The collection drop is
  unchanged in behaviour; it simply now says which kind it is.
* The metadata panel's keyword list and the new sidebar tree show the same
  facts from two angles — what this photograph carries, and what the library
  holds. Both are kept: the first is about the photograph in front of you, and
  removing it to avoid duplication would take the answer away from the place
  the question is asked.

## Alternatives rejected

* **A checkbox per row, as Lightroom has.** §2. Two opposite meanings a pixel
  apart.
* **A flat list of keywords instead of a tree.** The catalog is a tree, the
  query matches subtrees, and flattening it in the one place a user would see
  it would make the hierarchy something only the file format knows about.
* **Recursive delete.** §4.
* **Tagging on Enter in the field applying to the focused photo only, with a
  separate "apply to selection" button.** Two verbs for one intention, and the
  wrong one is the default.
