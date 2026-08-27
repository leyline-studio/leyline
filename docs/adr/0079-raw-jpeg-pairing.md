# ADR 0079 — A RAW and its JPEG are one photo

**Status:** Accepted — 2026-08

## Context

On 2026-08-24, a test library was built on a real corpus: three folders from a
Canon 5D Mark IV, **39 shots**, out of the camera in RAW+JPEG. The grid showed
**78**. Every photo, twice, side by side.

That is not a regression: it is what the repository has always done, and it
never decided to. The search is easy to redo — no ADR speaks of pairing, of
stacks or of a companion file; `catalog.md` §38 files HDR, focus stacking and
panorama among the future extensions **without mentioning this one**; and
`specification.md` §4 records it in no deliberate exclusion. It is therefore a
gap, not an abstention.

It deserves closing before the others for a simple reason: RAW+JPEG is a
**camera mode**, not a marginal practice. A user who turns it on sees their
library double, and it is the first thing they see.

### What the corpus imposes on the design

Three measured facts, each of which decides one point of the decision and none
of which any reading of the code would have given:

* **The two files are not necessarily in the same folder.** In the folders
  `2026_08_10`, `_12` and `_14`, the JPEGs are at the root and the CR2s in a
  `raw/` subfolder — hence in **two distinct catalog folders**. In
  `2026_08_16`, they sit side by side. The same camera, the same week, two
  arrangements. Folder proximity therefore cannot be the criterion.
* **The capture instant is identical and the dimensions are not.** On
  `5D4_2325`, both files carry `capture_date = 1786731816000`, to the second,
  while the RAW measures 6744×4502 against 6720×4480 for the JPEG — the
  sensor's border pixels. The instant pairs; the geometry does not.
* **The JPEG is imported before the RAW.** `collect()` enumerates by sorted
  path: `Photos/5D4_2326.JPG` precedes `Photos/raw/5D4_2326.CR2`. Pairing
  therefore cannot content itself with looking for an already-present RAW when
  a JPEG arrives — in the most common arrangement, it is the reverse that
  happens.

## Decision

### 1. The pair is a fact of the catalog, not a display artifice

`assets` gains a column:

```sql
companion_of INTEGER NULL REFERENCES assets(id) ON DELETE CASCADE
```

`NULL` — by far the most frequent case — means "this photo is itself". A value
designates the **master** whose companion this file is.

A nullable column rather than a `pairs` table: the relation is **asymmetric**
(there is a master and a follower, which a pairs table would have to re-encode
through a role column) and it is read on every grid display, where one more
join would be paid on every query in order to serve a case that does not exist
in the majority of libraries. The `ON DELETE CASCADE` says the only thing that
matters: a companion does not survive its master.

Migration `SCHEMA_V3` — the column is added, it pairs nothing (see §7).

### 2. The criterion: the same stem, the same instant, the same body

Two files form a pair if **all three** hold:

1. the same filename **stem**, case-insensitive (`5D4_2326`);
2. the same **capture instant** (`capture_date`, to the second);
3. the same **body** (`metadata.camera`).

**At any depth in the library**, and not in the common folder alone — the
context's first fact requires it, and the case that opened this ADR would be
precisely the one a per-folder rule would leave doubled.

The name alone does not suffice: two bodies reset their counters, and
`5D4_2326` may exist twice in a ten-year-old library. Nor does the instant
alone, and the corpus proves it: it contains bursts at 1/500 s, where two
distinct frames fall within the same second. It is the conjunction that makes a
false positive improbable, and each of the three criteria taken alone that
makes it common.

A file with no capture instant does not pair. A missing piece of metadata is
never guessed.

### 3. The master is the RAW, and a pair is exactly two

The master is the file whose pixels Leyline knows how to develop; the companion
is the rendering the camera wrote. A pair is therefore **a RAW and a non-RAW**:

* two RAWs (a CR2 and a DNG of the same frame) do not pair — neither is the
  other's rendering, and which would be the master has no answer;
* two non-RAWs do not pair — there is no master to designate;
* a companion cannot become a master in turn. No chains: a `companion_of`
  always points at a row whose `companion_of` is `NULL`, and pairing refuses
  any candidate already engaged on either side.

A third file of the same frame (a HEIF beside the CR2 and the JPEG) attaches to
the same master: the column allows it with nothing changed.

### 4. Pairing happens at import, in both directions

Every imported file looks for its counterpart **among what the library already
holds**, in both directions:

* a non-RAW that finds a matching RAW becomes its companion;
* a RAW that finds an **already-imported and unpaired** non-RAW adopts it.

The second direction is not a symmetry of convenience: it is the normal case,
established by the context's third fact. Shipping only the first would leave
precisely the corpus that motivated this ADR entirely doubled.

Pairing is an **import option** (`ImportOptions::pair_companions`, true by
default; `--no-pair` in the CLI), on the same footing as `--reference` and
`--flat`. See §8 for why it is not a preference.

### 5. The grid shows the masters — one clause, in one place

`grid::build` adds `AND a.companion_of IS NULL` to its `WHERE` clause, for both
shapes of query (manual collections included). Everything that counts, filters,
sorts or paginates goes through it: the count, the shot filters of
[ADR 0064](0064-metadata-filters.md), the full-text search and the smart
collections all follow without any of them having to know the notion of a pair.

That is the decisive argument against grouping done at display time: where a
SQL clause makes the catalog and the interface agree by construction, grouping
in Studio would make them diverge — the grid would show 39 photos while
`leyline ls`, the counter and the collections saw 78.

### 6. Nothing is hidden in silence

A companion is not deleted, not moved and not modified. It keeps its row, its
version, its revisions and its classification — it leaves the grid, that is
all. Four consequences, all deliberate:

* **It is visible.** `AssetDetails` gains `companion: Option<AssetId>` and
  `companion_of: Option<AssetId>`; Studio's details panel names the attached
  file, and the thumbnail carries a `RAW+J` badge.
* **It is deleted with its master.** `remove_assets` and `delete_assets` extend
  their list to the companions before acting — without which the `ON DELETE
  CASCADE` would erase the JPEG's row while leaving its file on disk, and the
  removal report would lie.
* **It does not export on its own.** An export bears on versions, and a
  companion's are no longer reachable from the grid. The camera's JPEG stays a
  file, exactly where it has always been.
* **It can be detached.** `leyline unpair <library> <asset-id>…` sets
  `companion_of` back to `NULL`; the photo reappears in the grid with what it
  had. Pairing is reversible because it destroys nothing.

### 7. An existing library does not reorganize itself

The migration adds the column and **pairs nothing**. A migration that paired
would make half a library's thumbnails vanish at the first launch after an
update — the worst moment and the worst way to learn about a feature.

Retroactive pairing is an explicit command: `leyline pair <library>`, which
says what it did, and **Library ▸ Pair RAW+JPEG…** in Studio, which announces
the number of pairs before acting. The same criterion as at import, the same
refusal of already-engaged candidates.

### 8. It is not a preference

[ADR 0078](0078-preferences-panel.md) §1's admission rule settles it on its
own: a setting enters Preferences if it bears on **the installation**. This one
bears on an import, in a library — it fails the first condition, and its
natural place is the import dialog, where it sits alongside the "reference
without copying" checkbox.

### 9. No pixel moves

No stage version, no revision entry, no rendering. `pipeline.md` §5.1 is not
concerned: pairing decides what the grid shows, never what the engine computes.

## Consequences

* A RAW+JPEG library shows the number of **shots**, and not the number of
  files. That is the point; it is also a visible change for anyone who was
  relying on the old count.
* A companion's classification, keywords and collections become unreachable
  while it is paired. They are not lost — `unpair` gives them back — but they
  cease to be modifiable, which is consistent with "one photo" and is written
  nowhere else.
* The catalog's schema moves to **v3**. `Catalog::open` already refuses a
  catalog newer than the supported version, and
  [ADR 0077](0077-application-updates.md) §4 copies `catalog.db` into
  `Backups/` before any migration: both guards apply here with nothing added.
* `import` now does one more read per file (the search for the counterpart),
  over an index. The cost is measured at implementation time; if it is not
  negligible beside the copy and the fingerprint computation, it is the
  implementation that must be revisited, not the decision.

## Alternatives rejected

**Not importing the JPEG when a RAW of the same name exists.** The shortest
option — zero schema change, no migration. Rejected because it answers the
wrong question: the photographer does not want their JPEG to disappear, they
want it to stop being a second photo. A JPEG outside the catalog is no longer
visible, no longer exportable and no longer classifiable, and finding it again
requires a second, separate import. It is a loss of information disguised as a
simplification.

**Grouping at display time only.** The catalog keeps two photos, Studio shows
one. Rejected by §5: it makes the catalog lie to the interface, and the
divergence is paid on everything that counts or filters. It would also have to
be redone in every client — Studio, the CLI and the SDK each have their own
listing — where one clause in `grid::build` serves them all.

**A general stacks table (`stacks`), of which RAW+JPEG would be a case.** That
is the tempting generalization: bursts, HDR bracketing, panoramas and RAW+JPEG
are all "several files, one photo". Rejected because it decides at once four
problems of which only one is posed, and because the other three do not have
the property that makes this one easy: a RAW+JPEG pair has an **obvious**
master, designated by the file type, with no user intervention. A burst stack
has none. The day stacks arrive, they will find `companion_of` in place and
will decide what to do with it; the reverse — shipping a general model in order
to serve the one trivial case — would cost more and decide less well.

**Pairing on the capture instant alone.** It would survive the renaming of one
of the two files, which the conjunction does not. Rejected on a measurement:
the corpus contains bursts at 1/500 s, where two distinct frames share the
second. Surviving a rename is a rare case; pairing two frames of a burst is a
common one.

**Pairing within the same folder only.** More cautious, and sufficient for a
camera that writes both files side by side. Rejected on the context's first
fact: half the test corpus files the RAWs in a subfolder, and that is exactly
the case that opened this ADR.

## What this ADR does not do

* **Stacks in the general sense** — bursts, bracketing, panorama — stay out of
  scope, and `catalog.md` §38 goes on announcing them as future extensions.
* **No setting travels between the RAW and its companion.** Developing the RAW
  does not touch the JPEG, and conversely. That would be a distinct decision,
  and it would require saying what becomes of a setting a JPEG cannot carry.
* **The JPEG does not become a preview of the RAW.** The camera rendered it
  with its own profile; using it as a preview would make the thumbnail lie
  about what Leyline will render. The preview cache of
  [ADR 0075](0075-preview-cache-retention.md) stays solely responsible for what
  the grid shows.
