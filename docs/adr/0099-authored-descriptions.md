# ADR 0099 — Authored descriptions: IPTC that a re-read cannot destroy

**Status:** Accepted — 2026-08

## Context

Lightroom's Metadata panel lets a photographer *write*: title, caption,
creator, copyright, credit, and where the picture was taken. Leyline reads
EXIF and writes XMP sidecars, so the plumbing exists at both ends, but
nothing between them can be edited — every metadata field in the catalog
comes from the file.

That is not an oversight, it is `docs/catalog.md`'s rule: **an asset is
purely factual**. The `metadata` table holds what the file said, and
`set_metadata` *replaces the whole row* every time the file is read again.
Storing an authored copyright there would work exactly until the next
re-import or EXIF re-read, and then vanish — silently, because nothing
would report a fact overwriting a decision.

So the question this decision answers is not "which fields" but "where do
authored fields live", and the repository has already answered its
cousin: classification (rating, label, pick) is not in `assets` either.

## Decision

### 1. A separate table, because it is a different kind of truth

Migration 10 adds `asset_descriptions`, keyed by `asset_id`: `title`,
`caption`, `creator`, `copyright`, `credit`, `city`, `state`, `country`.
Every column nullable, the row absent until someone writes one — the same
"neutral means absent" convention `Settings` uses.

Nothing that reads a file ever writes this table. `set_metadata` keeps
replacing `metadata` wholesale and cannot touch a description; a
re-import, an EXIF re-read or a reprocessing leave authored text exactly
where it was. That is the whole point, and it is a property of the schema
rather than of anyone's care.

Descriptions live on the **asset**, not the version: a caption describes
the photograph, and two develop versions of one photograph are two
renderings of the same subject. Classification went to the version for
the opposite reason — a rating judges a rendering.

### 2. Authored wins over read, and both stay visible

`metadata.artist` / `metadata.copyright` (what the file said) and
`asset_descriptions.creator` / `.copyright` (what the photographer wrote)
both persist. Display, search and XMP writing prefer the authored value
when there is one; the EXIF value is never edited, so "what the camera
recorded" remains answerable. A field that merged them would destroy the
distinction the previous section exists to keep.

### 3. The sidecar writes them, under the convention we already write

The XMP writer gains `dc:title`, `dc:description`, `dc:creator`,
`dc:rights` and the `photoshop:` location fields. The repository writes
one sidecar convention and reads two (a defect found on the real corpus
in 2026-08); this changes neither — it adds fields to the document, not a
naming rule.

### 4. A template is stamped on a batch, not hidden in an import option

`Library::describe_batch(assets, &template)` overlays a template onto many
assets at once — the case that makes this feature worth its migration: a
copyright line typed once and stamped on ten thousand photographs. It
overlays rather than replaces, so a template setting only `copyright`
leaves a title someone already wrote (`AssetDescription::overlaid_with`).

An `ImportOptions::description` field was the first shape considered and
is refused on two counts. It would touch **sixty-one** literal
initializers across the workspace for a convenience, and — the real
argument — the thing it would buy, applying the template *at
registration* so a file is never briefly uncredited, buys nothing: the
import call returns before any client can look at the catalog, so the
window it closes is not observable. A batch call also says what it does
at the call site, where an option says it in a struct three modules away.

### 5. Clients

CLI: `leyline describe <library> <asset-id> [--title …] [--caption …]
[--creator …] [--copyright …] [--credit …] [--city …] [--state …]
[--country …]`, each flag optional, `--clear` emptying the row; and
`import --copyright …` for the template. Studio: the fields become
editable rows in the detail panel. SDK: the type and the two calls are
re-exported.

## Rejected

* **Adding the columns to `metadata`** — §1: they would be destroyed by
  the next read of the file, at an unpredictable moment, with no error.
* **Keywords as an IPTC field** — the catalog has a keyword *tree* with
  its own table, its own index and its own filters (`docs/catalog.md`
  §Keywords). A flat `dc:subject` string beside it would be a second
  source of truth for the same question.
* **The full IPTC Core schema** (some sixty fields) — the eight above
  cover the panel Lightroom actually shows and the reasons people fill
  them in. The rest can be added one at a time, by someone who wants one.
* **Editing the EXIF in the original file** — the RAW is never modified
  (`docs/vision.md`); that is what the sidecar is for.
