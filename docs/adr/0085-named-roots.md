# ADR 0085 — Named roots: one catalog, several volumes

**Status:** Accepted — 2026-08. **Implemented 2026-08-29**, all four steps of
§9: schema (migration 9), marker, `roots.json`, one resolution point, the
offline state and its typed error, the CLI (`roots`, `root-add`,
`root-locate`, `root-forget`) and Studio's roots dialog with its offline
badge.

## Context

[ADR 0010](0010-relative-paths.md) decided that a library is a self-contained
folder and that the catalog records **no absolute path**: every reference is
relative to the root. That decision was right and stays right — it is what
makes a library survive a change of disk, of machine and of OS, and a backup a
plain folder copy. Nothing below takes it back.

What it also decided, without saying so, is that **a library covers exactly one
tree**. A file is referenced in place only if it already sits under the root:
`reference_in_place` (`crates/leyline-engine/src/import.rs`) canonicalises the
file and the root and skips anything else with `file is outside the library
root`. Corrected in the documentation on 2026-08-28, because three documents
claimed otherwise.

That limit met a real corpus on 2026-08-28:

* **748 GB** of photographs under `G:\Mes images`, 52,099 files
  ([ADR 0084](0084-assisted-culling.md) enumerated them);
* a library on another volume with **157 GB** free — so copy mode cannot hold
  the collection, and never will;
* the only cure available today: put the library root **above** the photos.

That cure works, and it is what we will do first. It is also the whole answer
only for as long as the photographs live on one volume. The day a second disk
holds the archive — the normal end state of a 748 GB collection that grows —
one catalog cannot cover both, and the collections, keywords and searches that
make the library worth having stop at the volume boundary.

**Lightroom Classic spans volumes**, and it is worth being exact about how,
because the mechanism is the decision. Each photo is anchored to a *named
volume* plus a path; an unplugged disk turns its photos offline rather than
missing; Smart Previews let developing continue without the original. The
anchor is a **volume label** — renameable, not unique, and identical on two
identically-named external disks — which is a per-machine absolute path wearing
a name, with the fragility [ADR 0010](0010-relative-paths.md) refused.

The obvious workaround does not exist either: a junction or a symlink from
inside the library root to the real folder is **canonicalised away** by
`reference_in_place`, which resolves both sides before comparing, and the file
is skipped exactly as if the link were not there.

So the question this ADR answers is: **can a catalog span several volumes
without a single absolute path entering it?**

## Decision

Yes, by making the anchor an *identity* rather than a location.

### 1. A root is an identity, not a path

A **root** is a folder that a library may reference photos inside. It is
identified by a UUID, and by nothing else. The catalog gains:

```sql
CREATE TABLE roots (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,   -- the folder's identity, read back from disk
    name TEXT NOT NULL,          -- what the user calls it ("Archive 2019")
    created_at INTEGER NOT NULL
);
```

On disk, a root carries a marker file `.leyline-root` holding that UUID. The
marker is what makes the identity **verifiable**: a folder is that root if and
only if its marker says so — not because it sits at a remembered path, not
because a disk carries a remembered label.

A marker belongs to the folder, not to a library: two libraries may reference
the same root, and neither owns it.

### 2. The location lives outside the catalog, and is advisory

The catalog stores identity. Where that identity currently sits on **this**
machine is a hint, and hints live in `roots.json` beside `catalog.db`:

```json
{ "3f2a…": "G:\\Mes images", "9c7b…": "/mnt/archive/2019" }
```

Advisory in the strict sense: every hint is **verified by reading the marker**
before use, a hint that does not verify is discarded, and the file can be
deleted at any time at the cost of one re-location. It is rebuildable state,
like `Cache/`.

This is what keeps [ADR 0010](0010-relative-paths.md)'s property literally
intact: `catalog.db` still contains no absolute path, so a library folder
copied to another OS still opens, still shows every photo it has a preview
for, and asks once per root where it went.

Resolution order, and there is no fourth step: **(1)** the hint, verified by
its marker; **(2)** the library root itself, which is always root 1 and needs
no hint; **(3)** ask the user. No scanning of mounted volumes looking for
markers — it is slow, and it guesses.

### 3. The library is root 1

Every existing library migrates to exactly one root — itself — and nothing
about it changes: the marker is written into the library folder, `folders`
gains `root_id NOT NULL DEFAULT 1`, and `UNIQUE(relative_path)` becomes
`UNIQUE(root_id, relative_path)`. Paths stay relative, to their own root. The
validator (`validate_library_relative_path`) is untouched: it still refuses
absolutes, drive letters, backslashes and `..`, and it is now the guarantee
that a stored path cannot escape *its* root.

A library that never adds a second root is byte-for-byte the library of
[ADR 0010](0010-relative-paths.md), and pays nothing for this ADR.

### 4. One resolution point

Every `root.join(relative)` that resolves an **asset** goes through a single
`Library::locate(asset) -> Result<PathBuf>`. Not a convenience: it is the only
way the offline case of §5 can be handled once instead of at each of the dozen
call sites that open a photo today. `Cache/`, `Masks/`, `Profiles/`,
`Exports/` and `Backups/` keep joining the library root directly — they are
library-local by definition, and no root but the library's own ever holds them.

### 5. Offline is not missing, and the difference is settled now

A root whose marker cannot be found is **offline**. Its assets are not missing,
not removed, and nothing about them is rewritten — an unplugged disk is not an
edit.

* The grid keeps working. Previews live in the library's `Cache/`, keyed by
  asset and revision, and [ADR 0082](0082-embedded-preview-at-import.md) fills
  the thumbnail at import: after importing, browsing, filtering, rating,
  keywording and searching need no original.
* Anything that needs pixels — develop, export, print, reprocess, a new
  preview kind — fails with a typed error **naming the root**, not with the
  file-not-found of a deleted photo.
* `assets.is_missing` must never be set because a root is offline. Today
  nothing sweeps for missing files (`is_missing` is written at insert and read
  only by `folders.rs`), which is exactly why this is decided now: the first
  sweep to be written would otherwise mark twenty thousand photos missing the
  first time a drive is unplugged, and *that* is the failure that loses work.

### 6. A root is created by an explicit gesture, never implicitly

An import whose source sits under a known root references in place. An import
whose source sits elsewhere keeps today's behaviour — skipped, with today's
message — unless the user adds that folder as a root, which writes the marker
and the `roots` row. Copy mode is unchanged and remains the answer for a card
offload.

Watched folders ([ADR 0039](0039-watched-folder-import.md)) keep importing by
copy, as they do today, and are unaffected.

### 7. What this costs the backup promise

[ADR 0010](0010-relative-paths.md) promises that backup and restore are a
folder copy. With external roots that promise covers **the catalog and the
work** — every setting, revision, rating, collection and keyword — and no
longer the originals, which are on the other volumes by construction. This is
the honest price of the feature, it must be said in the interface where roots
are managed, and it is why the library's own root stays the default: a library
that keeps its photos inside itself keeps the old promise whole.

### 8. Clients

* **CLI**: `leyline roots <library>` (name, uuid, path, online), `root-add`,
  `root-locate`, `root-forget` (refuses while assets reference it).
* **Studio**: the roots list in a dialog, an offline badge on the grid, and
  one "locate…" gesture. Nothing else in the interface changes.

### 9. Order of work

1. ✅ `roots` + `root_id` + marker + `roots.json` + `Library::locate`, with the
   library as root 1 and no way yet to add a second: the migration lands and
   changes no behaviour.
2. ✅ The offline state, its typed error, and the guarantee of §5 on
   `is_missing`.
3. ✅ Adding, locating and forgetting a root — CLI (`roots`, `root-add`,
   `root-locate`, `root-forget`), then Studio's dialog.
4. ✅ `catalog.md` §2.3, §3 and §8 change with step 1, not before: until the
   migration exists, the specification describes the schema that exists.

Two things the implementation settled that the decision above left open, both
worth recording because neither is obvious from the text:

* **Root 1 takes the library's own `library.uuid`** rather than a fresh one.
  Nothing needs generating, the identity is already unique and already stable
  across copies, and the marker becomes *derivable from the catalog* — so a
  library restored from a backup that dropped the dotfile gets it back on the
  next open instead of becoming unidentifiable.
* **Migrations now run with foreign keys off, and `PRAGMA foreign_key_check`
  after each one.** Changing `UNIQUE(relative_path)` to
  `UNIQUE(root_id, relative_path)` requires rebuilding `folders`, hence
  dropping a table `assets` still references; `ON DELETE RESTRICT` fires
  immediately even inside a transaction and even under `defer_foreign_keys`,
  so enforcement had to come off. What replaces it is stronger: the check
  validates *every* key in the database rather than only the rows touched.

## Consequences

* One migration (v9), and one more marker file to write when a library is
  created.
* `folders.relative_path` stops being globally unique; every query that
  assumed it must be found and given its `root_id`.
* XMP sidecars sit next to the photograph, so they are read and written only
  while their root is online — the same rule as the photo itself.
* `pipeline.md` §5.1 is untouched by construction: no path enters
  `settings_json`. The referenced files that *do* live in settings — DCP
  profiles ([ADR 0035](0035-camera-profile-dcp.md)) and LUTs
  ([ADR 0053](0053-creative-lut.md)) — are library-relative and stay under the
  library root, which is always online.
* Two folders carrying the same marker (a copied root) resolve to whichever
  verifies first. The marker states identity; it cannot arbitrate a duplicate
  the user made.

## Alternatives rejected

* **An absolute path per asset** — the original refusal of
  [ADR 0010](0010-relative-paths.md), unchanged: the classic way a catalog
  breaks on a migration.
* **The volume label, Lightroom's own anchor** — not unique, renameable, and
  identical on two identically-named disks. It is a remembered location, and
  remembering a location is the thing being avoided.
* **The filesystem's volume UUID or serial** — needs a per-OS API for what a
  file does portably, is not preserved by copying a volume or restoring it to
  a new disk, and identifies *a disk* where the user thinks of *a collection*.
* **Keeping the location in the catalog** — reintroduces the absolute path, and
  costs the property that a library folder opens on another OS.
* **Keeping the location in Studio's `preferences.json`**
  ([ADR 0078](0078-preferences-panel.md)) — the CLI and the SDK would be blind
  to roots that Studio can see. This is not a preference: it does not govern
  the installation, it describes one library's view of one machine.
* **A symlink or junction inside the library root** — the workaround that looks
  free. It is refused by `reference_in_place`'s canonicalisation, and were it
  allowed it would make `folders` describe a tree that does not exist.
* **One catalog per volume, searched across later** — collections, keywords and
  smart collections stop being global, which is most of what a catalog is for.

## What this ADR does not do

* **No Smart Previews, no offline develop.** Offline means browse, organise and
  search — not develop. A proxy that survives its original is pixels, with a
  retention policy ([ADR 0075](0075-preview-cache-retention.md)) and a
  reproducibility question of its own; it is a separate decision, and it is not
  a prerequisite for this one.
* **No consolidate.** Nothing here moves photographs between roots.
* **No mount handling.** Leyline does not mount, unmount, wake or wait for a
  volume; it reads a marker and reports what it found.
* **No change to copy mode**, which stays the nominal case and the right answer
  for a card.
