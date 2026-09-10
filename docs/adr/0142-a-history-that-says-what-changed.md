# ADR 0142 — A history that says what changed

**Status:** Accepted — 2026-09

## Context

The develop history panel showed three lines: `2026-09-10 12:54`,
`2026-09-10 13:02`, `2026-09-10 13:02 (actuel)`. Nothing else. Two of them
carry the same minute, and none says what it holds — the panel is browsable
(click a row, the photograph goes back to that state) and unreadable, so the
way to use it is to click and look.

The information was already there. Every revision stores a **complete,
self-contained** develop state (`docs/pipeline.md` §3.2) and its parent's id,
so "what does this revision hold differently from the one before it" is a
comparison of two documents the catalog already has. The panel was building
its rows from `created_at` and nothing else.

## Decision

### 1. The engine names keys, the client says words

`changed_settings(before, after)` returns the `settings_json` **keys** that
differ. Compared through the serialized form rather than field by field, for
the reason §3.2 makes that format a contract: a field added to `Settings`
appears in the answer the day it is added, with no second list to keep in
step. A field back at its neutral serializes to nothing and is therefore
reported as a change, which it is.

The engine returns keys, not sentences: it has no translations and no
business having any. The clients turn `exposure` into « Exposition » —
Studio through `Tr.setting-label`, which is the shape `preset-field` already
established, and the CLI by printing the key itself, which is what a
developer's tool should show.

Keys come back **sorted by key**, because that is what a `serde_json` object
gives and because the answer has to be deterministic. Studio sorts the
*labels* afterwards, in the language it is showing them in: alphabetical by
English key is an arbitrary order to a French reader.

### 2. What a line says, and what it refuses to say

A row is now two lines: what changed, and under it, smaller, when. One line
was tried first and measured — the left column is 230px, and
« 2026-09-10 13:02 · Exposition, … » spent two thirds of its width on a date
and cut the answer off.

Four cases, and each is a decision:

* **The first revision** has no parent: « Import ». There is nothing to
  compare it with, and « 39 settings » would be true and useless.
* **A revision a preset produced** names the preset — « Preset "Portrait" » —
  and not the twenty settings it moved. `RevisionRow` gains `from_preset`,
  reading a column the catalog has stored since
  [ADR 0058](0058-preset-provenance-and-shelf.md) §5 and no client had ever
  read.
* **More than two changed settings** get the first two and a count:
  « Exposition, Contraste + 3 ». Six labels in a 230px column is a row nobody
  reads.
* **`stages` alone** means « Retraitement ». `stages` alongside other keys
  means nothing and is dropped: editing the exposure *adds* `gains` to the
  stage map, and a line reading « Exposition, Retraitement » for one slider
  would be true of the document and false about what happened.

A revision is compared with **its parent, found by id** — not with the
previous row in the list. Two revisions can share a timestamp (an amendment
lands on the same second as what it amends), and a list sorted by time is
then in an order the revision graph does not agree with.

### 3. `SETTINGS_KEYS`, and the guard that keeps the labels honest

A key with no label would reach the screen as a raw identifier. So
`leyline-core` now publishes `SETTINGS_KEYS` — every key a `settings_json`
document can carry — and Studio has a test asserting each has a branch in
`setting-label`.

The list is kept in step with the format by the test that was already there:
`every_settings_key_is_named_in_the_pipeline_specification` builds a settings
value with **every field off its neutral**, serializes it, and now compares
the result with `SETTINGS_KEYS` as well as with `docs/pipeline.md`.

Writing that comparison found what the test had been missing: six fields —
`stages`, `monochrome`, `parametric_curve`, `red_eye`, `source_encoding` and
the flattened `extra` — were still at their neutral in the value that claimed
to move everything, so their keys were never checked against the
specification at all. `source_encoding` turned out to be documented only
inside a sentence, in a form the test could not see; it now has its own row
in §3.2's table. This is the second time this test has been found
under-covering (its own comment records `demosaic`), which is the argument
for comparing it against a published list rather than trusting the
construction.

`extra` is deliberately **not** in the list: it is `#[serde(flatten)]`, so a
key from a future version round-trips at the top level under a name this
version has never heard of (§3.4). Such a key is reported as changed and
shown raw — which is the honest thing for a document written by a newer
Leyline.

## Consequences

* The panel reads: *Import / Exposition / Aucun changement / Exposition*,
  each with its date underneath. Rows are 34px instead of 26px.
* `leyline history` prints what each revision changed instead of two
  arbitrary values (`exposure`/`contrast`) — head first still, the order
  `version_history` documents; one CLI test asserted on the whole line and now
  asserts on the stage list it was really about.
* The panel now shows something the interface had never admitted: revisions
  that changed **nothing**. A drag that ends where it started still commits.
  Whether that revision should exist at all is an engine question — the
  history is where it became visible, and this ADR does not settle it.
* `RevisionRow` gains a field; `SETTINGS_KEYS` is a new public constant of
  `leyline-core`, re-exported by the SDK.

## Alternatives rejected

* **Storing a label on the revision** when it is written. A second copy of an
  answer the settings already hold, wrong for every revision written before
  the column existed, and untranslatable — it would freeze one language into
  the catalog.
* **Naming the *value*** (« Exposition +0,35 »). Revisions coalesce
  successive edits of one setting ([ADR 0120](0120-edit-session-claim.md)), so
  the value in a row is the value at the end of a gesture, not "what this step
  did" — and the value is one click away in the panel that fills when the row
  is selected.
* **Labelling by `SettingsGroup`** (« Tonalité » rather than « Exposition »).
  Thirteen labels instead of thirty-seven, and it hides exactly the
  distinction a history is read for: *which* slider moved.
* **A per-key diff of nested objects** (« Netteté : rayon »). A revision's
  `sharpening` is one commit unit ([ADR 0126](0126-slider-precision.md)
  coalesces the drag), and one more level of detail is one more level of
  vocabulary to translate for a line that is already two words long.
