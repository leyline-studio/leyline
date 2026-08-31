# ADR 0100 — Renaming files: the one place non-destructive is a promise about the disk

**Status:** Accepted — 2026-08

## Context

Lightroom renames: at import, and in batch through F2, from templates
(date, sequence, custom text). Leyline never renames — a file keeps the
name its camera gave it, `IMG_4231.CR2`, forever.

Every other feature in this parity run touched the catalog or the
pipeline. This one touches **files on disk**, and it is the only place
where "non-destructive" stops being a property of the architecture and
becomes something the code has to actively not get wrong. The catalog
stores no asset path — it stores a folder and a filename
(`docs/catalog.md` §9) — so a rename is a `filename` update plus a
filesystem move, and the two must not be able to disagree.

Three things already in the repository decide most of this:

* `UNIQUE(folder_id, filename)` — the catalog already refuses two files
  of one name in one folder;
* `validate_library_relative_path` — the one place a user string becomes
  a path, and the guard ADR 0087 already routed a session name through;
* companions (ADR 0079) — a RAW and its JPEG are one shot under two
  files, and renaming one alone would silently unmake the pair's stem
  criterion.

## Decision

### 1. A template is a name, never a path

`{name}` (the current stem), `{date}` (capture date, `YYYY-MM-DD`),
`{time}` (`HHMMSS`), `{seq}` (position in the batch, zero-padded to four);
the extension is kept and never named — it says what the file *is*.

Dates are **UTC**, not local: a filename that depended on the machine's
zone would give one photograph two names depending on where it was
renamed, and the same batch run on two machines would disagree. That is
also what `format::capture_date` already does. Unknown placeholders are **refused**, not left
as literal text: `{sequence}` typed instead of `{seq}` must say so rather
than produce ten thousand files called `IMG{sequence}`.

The result is a **filename**, and a `/` in it is refused — renaming does
not move a photo between folders. Anything the template yields still goes
through `validate_library_relative_path` against its folder, so the guard
that protects every other stored path protects this one.

### 2. The disk moves first, and the catalog follows only if it worked

`std::fs::rename` first, catalog update second, and a failure at the
first step leaves nothing changed. The reverse order would give a catalog
naming a file that does not exist — the state that makes a library look
corrupted rather than merely unfinished. Within one batch each file is
its own transaction: renaming four hundred photos and failing on the
three hundredth leaves 299 renamed and correct, and reports the failure.

A destination that already exists is **refused before the move**, not
overwritten. This is the one operation in Leyline that can destroy a
photograph, and it declines to.

### 3. Companions travel with their master

Renaming a RAW renames the JPEG shot with it, to the same stem
(ADR 0079's pair criterion is the shared stem, so renaming one alone
would break the pair — silently, since nothing re-checks pairing after
the fact). Sidecars travel too: both naming conventions the repository
reads (`photo.xmp` and `photo.CR2.xmp`) are moved if present, so the
edits a sidecar carries follow the photo.

### 4. It is a batch operation, reported like the others

`Library::rename(assets, template) -> RenameReport`, listing what was
renamed and what was refused with the reason — the shape `ImportReport`,
`ExportReport` and `ReprocessReport` already have. `{seq}` counts within
the batch, in the order given.

## Consequences

* The first engine operation that moves a user's original file. The RAW's
  *content* is still never modified, so `docs/vision.md`'s promise holds
  in the sense it was written in — but the promise now needs the sentence
  above about refusing to overwrite, and gets it.
* No migration: `filename` is a column that already exists and already
  has its uniqueness constraint.

## Rejected

* **Renaming into another folder** (`{date}/{name}`) — that is
  *organising*, a bigger decision about who owns the library's shape, and
  it would need its own answer to "what happens to the folder left
  empty". A rename that only renames is the honest half.
* **Undo** — the inverse of a rename is another rename, and offering
  "undo" for a filesystem operation whose target may since have been
  touched by another program would be a promise the engine cannot keep.
  The refusal to overwrite is what protects the user here instead.
* **A rename-at-import option** — the same argument ADR 0099 §4 made
  about description templates: it would touch every `ImportOptions`
  literal to save a call the client can make on the report it just got.
