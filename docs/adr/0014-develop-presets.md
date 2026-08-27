# ADR 0014 — Develop presets: partial by category, applied as an ordinary revision

**Status:** Accepted — 2026-07

## Context

The vision (`readme.md`) and the V1 specification had never covered develop presets (named, reusable sets of settings, applicable to one photo or in bulk) — only export presets existed (`catalog.md` §27). It is nevertheless a basic expectation of a non-destructive RAW development tool, and its absence was identified as blocking for a V1 faithful to the vision. `docs/presets.md` sets the product contract; this ADR records the structural choices that follow from it.

## Decision

**Presets are partial, by category, not complete snapshots.** A preset captures only the setting categories explicitly included (`groups`, `presets.md` §3.1–3.2), at the granularity of a Lightroom-style checkbox (`white_balance`, `tone`, `presence`, `lens_correction`, `detail`, `geometry`) — never a single field. `geometry` (rotation, crop) is never included by default.

**One schema space.** `preset_json.schema` refers directly to the numbering of `settings_json.schema` (`pipeline.md` §3.2) — a preset consumes the same field vocabulary as a revision, not an independent format to evolve in parallel. No `process` field: a preset never fixes a rendering, only values.

**Applying a preset is an ordinary revision.** Applying a preset to a version introduces no new writing mechanism: it is a plain `EditSession` (`engine-api.md` §10.1) that merges the fields of the included categories onto the current state and then commits — always a new revision, never an amendment. No notion of "a revision that came from a preset" exists in `develop_revisions`: no column, no FK towards `develop_presets`.

**No batch transaction.** Applying to a set of versions treats every version independently (one revision each), with a per-version success/failure report, on the model of `export_batch` and of import (`engine-api.md` §3.2, §12). There is no all-or-nothing unit spanning the whole batch.

**A dedicated catalog table.** `develop_presets` (`presets.md` §4) is distinct from `export_presets`, despite a close SQL shape: two independent domains (develop settings vs. output encoding).

## Consequences

* No new rule of non-destructiveness or reproducibility: an applied preset produces a revision like any other, and undo/redo and the complete history work with no special-case code (`pipeline.md` §2, §6).
* A photo's crop and rotation are never altered by mistake when a style is applied in series — a necessary condition for a preset to be safe to apply to a whole library.
* Renaming or deleting a preset has no retroactive effect: the revisions it produced remain ordinary setting states, with no traceable link back to their origin (accepted: `catalog.md` §17 already defines a revision as "a user intent, never an interface event").
* A partially failing batch (say, a preset referring to a schema the engine no longer knows) leaves the already-processed versions committed: consistent with the "the version is the library's unit" model (ADR 0008), not a regression introduced here.
* `preset_json` is serializable on its own (no reference to a library): the deliberate sharing of presets mentioned in `readme.md` stays a UI extension, not a format change, on the day we build it.

## Alternatives rejected

* **A complete snapshot of `settings_json`**: replaying it would systematically overwrite the target photo's crop, rotation and lens correction — unacceptable for a bulk application, and contrary to the non-destructive "judgement per photo" spirit already settled for classification (ADR 0008).
* **Per-field rather than per-category granularity**: no product need goes beyond the Lightroom-style split; it would add test and configuration surface (matrices of individually checkable fields) with no identified benefit for V1.
* **`develop_revisions.preset_id` (an FK to the origin)**: it would break the invariant that a revision is an anonymous state, and force every consumer of the revision graph (undo, export, reprocessing) into a "revision from a preset" special case with no product need to justify it; `author` already exists should provenance become necessary one day (§17).
* **A single all-or-nothing transaction over the batch**: it would demand a distributed rollback across per-asset revision graphs, inconsistent with the already-specified behaviour of `export_batch` and of import, which report per-item failures without cancelling the rest.
* **Reusing `export_presets`**: an almost identical SQL structure, but unrelated domains (render settings vs. encoding parameters); merging them would have imposed optional columns or a type discriminator on an already simple table.
