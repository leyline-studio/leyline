# ADR 0043 — Collapsing the pre-release render history, and the revision carrying its own stage map

**Status:** Accepted — 2026-07
**Completes and amends:** [ADR 0042](0042-versioned-stage-pipeline.md) (§2 delivered, §5 rendered moot)

## Context

ADR 0042 replaced the eleven duplicated `processN.rs` modules with
independently versioned stages. Its §5 provided that `process: N` would stay
read and understood forever, through a **frozen expansion table** of eleven
rows: `process: 4` meaning `lens::v2` + `gains::v2` + `contrast::v1` + …

That table exists, it is correct, and the 77 reference renders prove it renders
the eleven versions to the bit (commit `bf63df1`). The question the present
ADR settles is not *whether it works*: it is **whom it serves**.

**The answer: nobody.** Leyline has not been published. Nowhere does there
exist a single revision citing `process: 3` outside the author's development
catalogs and the test fixtures. The eleven versions are not eleven promises
kept to eleven generations of users — they are eleven construction steps, kept
by applying a rule ("an engine must know how to render every past process
version") at a time when that rule had nothing yet to protect.

What that history costs, once collapsing is set aside:

* three stage versions that exist only for it — `gains::v1` (the exact `powf`
  from before ADR 0013), `lens::v1` (distortion alone) and `lens::v2`
  (distortion plus vignetting, without TCA);
* eleven expansion-table rows to keep exact, of which ADR 0042 itself says
  that an error "would render an old photo differently" — the engine's most
  critical point, maintained for photos that do not exist;
* 77 golden cases to recompute on every evolution of the harness;
* and above all a **duplicated** versioning axis. ADR 0042 §2 has versioning
  carried by the revision's `stages` map; `process: N` was to survive as a
  historical shorthand. Two coexisting mechanisms designating a rendering,
  only one of which has a future.

The reasoning is exactly the one ADR 0042 already applies to itself about the
change in `settings_json`'s shape: *"this is acceptable only because the
project is pre-release; after opening to the world, that shape would be
definitive. That is the reason to make this change now and not later."* The
same window, exactly, is closing on the render history. On publication day
those eleven versions become irreversibly commitments; today they are only
code.

## Decision

### 1. One published version per operator, numbered `v1`

The pre-release render history is collapsed. Every operator keeps **one**
version: the one that renders today, that is, the state of `process: 11`.
`gains::v1`, `lens::v1` and `lens::v2` are removed; `gains::v2` and `lens::v3`
become their operator's `v1`.

They are numbered `v1` and not `v0`: these are not drafts. They are each
operator's first **published** versions, frozen in ADR 0042 §1's full sense on
the day of opening to the world. `v0` would suggest a provisional status that
will cease to be true without a line of code changing.

### 2. The revision records its stage map; `process` disappears

ADR 0042 §2 is delivered here, in the same move, and for a mechanical reason:
collapsing makes it trivial. There is only one possible expansion left, hence
nothing left to expand — the revision writes directly the version of each
stage it uses:

```json
"stages": { "gains": 1, "tone_curve": 1, "dehaze": 1 }
```

The `process` field is **removed** from `settings_json`, with no replacement
and no historical shorthand: it no longer has a value to designate. The
`schema` field, by contrast, stays — it versions the document's *shape*, not
the rendering, and the two axes stay distinct (`docs/pipeline.md` §3.4).

ADR 0042 §2's rules apply unchanged: a neutral stage does not run, therefore
has no behaviour to pin, and **does not appear** in the map.

### 3. Pinning a version happens at write time, never at read time

When a revision is written, every active stage receives an entry:

* a stage **already present** in the map keeps its version — correcting the
  exposure of a photo from 2026 in 2036 does not change its rendering;
* a stage **absent** (newly moved off its neutral value) receives the engine's
  current version — turning sharpening on in 2036 gives 2036's best
  sharpening, not 2026's;
* a stage **back to neutral** loses its entry, since it no longer renders
  anything.

Nothing is ever inferred at read time: a map read is applied as it stands.
That is what makes the revision self-describing in ADR 0042 §2's sense.

### 4. An unknown stage version is refused, never approximated

`process > CURRENT_PROCESS` was ADR 0042's guard (`docs/pipeline.md` §3.4): a
revision written by a newer engine is refused, never guessed. It becomes:
**any map citing a stage or a stage version this engine does not know fails**
with `NewerSettings`, and the caller falls back on the best cached preview.
The observable behaviour is identical; its granularity is better, since the
refusal names the offending stage.

### 5. Existing development catalogs are not migrated

No migration code is written. A stored revision citing `process: N` no longer
has a valid shape and is refused at read time, like any unreadable revision.
Development libraries are re-imported.

Writing a migration would be **false rigour** here: it could not preserve the
pixels of revisions at process < 11 (their rendering was defined by the
absence of vignetting, of TCA, of a tone curve…, and collapsing precisely
makes that absence disappear). It would preserve only settings, on test data,
at the price of conversion code to maintain and test — whose only
justification would be to act *as if* the promise already applied to photos it
does not yet apply to.

### 6. The reference renders are re-blessed once, deliberately

ADR 0042 §7's 77 golden cases become the single pipeline's cases. The manifest
is regenerated **once**, by this decision and on its record.

That is the exact opposite of the forbidden case. ADR 0042 §7's rule — "it is
the fixtures, not reading the diff, that establish equality" — targets the
digest that moves **during a refactor meant to change nothing**: there, the
moving digest *is* the failure signal. Here the decision precedes the
measurement and owns it: we have decided that those renderings are no longer
owed to anyone. After that regeneration, the rule regains its full force with
no exception.

### 7. The versioning mechanism keeps a living example

A side effect to be handled rather than endured: with a single version
everywhere, the path "an old version still renders what it used to render" is
no longer exercised by any test. That is the project's central guarantee
becoming, for the first time, uncovered code.

The registry therefore permanently carries a **two-version stage reserved for
tests**, compiled only under `cfg(test)`: the suite verifies that a map citing
`v1` renders `v1` even though `v2` exists, that the rank each version declares
decides the position, and that an unknown version is refused. The mechanism
thus stays demonstrated without waiting for the first real pixel fix — which,
when it comes, really will be a `v2`.

## Consequences

* **What disappears:** three stage versions, the expansion table and its
  eleven rows, the `process` field, `CURRENT_PROCESS`, and the notion of
  "migrating a photo to a recent process version" — replaced by "raising a
  revision's pinned stages to their current version", that is, reprocessing
  (`reprocess`), which keeps its name and its meaning.
* **The promise does not move by an inch** — it merely takes its true date. It
  is still stated as ADR 0042 §6 states it, per stage version; it starts
  running at publication, which is exactly when it becomes owed. Pretending it
  was already running was the illusion the present ADR removes.
* **The window closes here.** After the first public version, no ADR will be
  able to collapse anything: stage versions become commitments, and the only
  possible evolution is addition. The present ADR is therefore, by
  construction, the last of its kind.

## Alternatives rejected

* **Keeping the expansion table "just in case"**: it protects no existing
  photo and costs the engine's most critical exactness. A safety mechanism
  maintained against a nil risk is not safety, it is an error surface — and
  this one errs silently, in pixels.
* **Collapsing now, delivering the `stages` map later**: it would change
  `settings_json`'s shape twice in a row, hence break the development catalogs
  twice, for a single end result.
* **Numbering `v0`**: see §1. The number would outlive the status it
  describes.
* **Writing a migration for the development catalogs**: see §5.
* **Giving up freezing and adopting `legacy_params` (darktable)**: already
  examined and rejected by ADR 0042, for a reason the present ADR does not
  touch. We are collapsing a history that commits us to nobody; we are not
  giving up freezing the one that will.
