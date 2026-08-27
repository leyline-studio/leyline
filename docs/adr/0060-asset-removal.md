# ADR 0060 — Removing a photo from the catalog, and deleting it from disk

**Status:** Accepted — 2026-08

## Context

Leyline knows how to import a photo. It does not know how to remove one. That
absence is total: neither `leyline-catalog`, nor `Library`, nor the CLI, nor
Studio exposes anything that removes an asset. The catalog accumulates nothing
but additions, and the only way back is to throw the whole library away.

That is not merely a missing convenience. It is what makes **an already-decided
remedy inapplicable**:
[ADR 0043](0043-collapse-prerelease-render-history.md) §5 collapsed the
pre-release render history without writing a migration, prescribing in so many
words that "**development libraries are re-imported**". Yet re-importing is
blocked by the duplicate detection of
`crates/leyline-engine/src/import.rs:126`: a file whose BLAKE3 is already known
is set aside, with no recourse. A revision that has become unreadable therefore
cannot be repaired, and the asset carrying it cannot be replaced.

The decision and the implementation contradict each other. That is a bug, not a
gap.

The schema, for its part, was ready from the start. The **seven** tables that
reference `assets` — `develop_revisions`, `develop_versions`, `develop_current`,
`previews`, `asset_keywords`, `export_history` and the search index — are
**all declared `ON DELETE CASCADE`** (`docs/catalog.md`), and
`PRAGMA foreign_keys = ON` is set on every opening
(`crates/leyline-catalog/src/connection.rs:15`). A `DELETE FROM assets` already
cleans the whole graph. Only the API was never written.

## Decision

**Two distinct operations, never one ambiguous gesture.** It is the distinction
Lightroom makes, and it is not cosmetic: conflating "I no longer want to see
this photo in my catalog" and "destroy this file" is the mistake a photo
program cannot afford even once.

### 1. `remove` — taking it out of the catalog

Removes the assets from the catalog. **The file is not touched.** Those assets'
development, keywords, collections, export history and previews disappear with
them — which is what the cascade already does.

It is the default operation, the one that unblocks ADR 0043 §5: once the asset
is removed, its fingerprint is no longer known, and the file re-imports
normally.

### 2. `delete` — deleting it from disk

Does everything `remove` does, **then** sends the source file to the system's
trash.

* **To the trash, never a permanent erasure.** An irreversible deletion of a
  RAW from a photo program is exactly the gesture where "your data belongs to
  you" demands a safety net. If the platform offers no trash for that file, the
  operation **fails outright** instead of silently falling back on an `unlink`:
  a silent fallback would make the guarantee a lottery.
* **The XMP sidecar goes with it.** `xmp::sidecar_path` gives its path
  ([ADR 0047](0047-xmp-sidecar-read.md)); leaving an orphaned `.xmp` beside a
  vanished RAW makes no sense.
* **Nothing outside the library is ever touched.** Importing by reference
  already requires the file to live under the library's root — the catalog
  stores only paths relative to that root
  ([ADR 0010](0010-relative-paths.md)). There is therefore no case in which
  `delete` would leave the library's tree, and the relative-path validator
  remains the guard at the entrance.

### 3. What deletion does not call into question

`docs/catalog.md` states that "revisions are never deleted". That invariant
**bears on the develop history of a living asset**: one does not rewrite the
past of a photo one keeps, which is what makes undo and snapshots dependable.
Removing an asset is not a rewriting of history, it is the photo's exit from
the catalog. The two rules do not meet, and `docs/catalog.md` is clarified in
the same change so that the reading is not ambiguous.

The only real amendment is the [non-destructiveness contract](../pipeline.md)
§6, which guarantees that Leyline never modifies a source file. `delete` does
not modify it either — it removes it, at the user's explicit request, to a
place from which it can be recovered. The guarantee aimed at silent writes by
the software onto the originals; it never aimed at preventing the user from
throwing away their own photo.

### 4. Surface

**Catalog** — `Catalog::delete_assets(&[AssetId]) -> Result<Vec<String>>`, in
one transaction, returning the relative paths of the cached previews. That
return is the operation's only subtlety: the preview cache lives **outside**
SQLite, the cascade erases its rows but would leave its PNGs orphaned on disk.
It is exactly the shape, already proven, of `remove_revision_previews`.

**Engine** — two methods, whose names say which one touches the disk:

```rust
// Removes from the catalog; the files are not touched.
pub fn remove_assets(&self, assets: &[AssetId]) -> Result<RemovalReport>;
// Removes from the catalog, then sends sources and sidecars to the trash.
pub fn delete_assets(&self, assets: &[AssetId]) -> Result<RemovalReport>;
```

`RemovalReport` carries what actually disappeared and what resisted — a file
already absent is not an error (the catalog must be able to clean itself of a
file the user moved outside Leyline), a locked file is one, and the caller must
be able to tell which.

The engine emits an **`Event::AssetsRemoved { asset_ids }`** event, beside
`AssetsAdded` and `AssetsChanged` (`docs/engine-api.md` §3.2). Without it, any
view open on those assets — grid, filmstrip, map — would go on displaying dead
rows.

**CLI** — `leyline remove <ids…>` and `leyline delete <ids…>`, the latter
requiring `--yes` to run without an interactive terminal.

**Studio** — two distinct entries in the Photo menu and in the grid's context
menu, operating on **the whole selection**. Both ask for confirmation, naming
the number of photos; the one that touches the disk says explicitly that the
files go to the trash. Studio today has no generic confirmation dialog — it
gains one, reusable.

## Consequences

* ADR 0043 §5 becomes applicable again: an inherited development library is
  repaired by removing the assets and re-importing the folder.
* **`import.rs` is not touched.** Adding a "force re-import" option would treat
  the symptom by creating duplicates in the catalog, where freeing the
  fingerprint is the clean cause. Duplicate detection stays unconditional.
* A new `trash` dependency, for the three systems' trash. It is verified on the
  Windows cross-build (`packaging/windows/build-nsis.sh`) in the same change —
  a dependency that broke that build would have to be rejected, whatever its
  merit.
* `Event` gains a variant: every exhaustive `match` in an SDK consumer must
  handle it. The project being unpublished, that commits nobody.
* No schema migration, no stage version, no pixel changed. The render contract
  (`docs/pipeline.md` §5) is not concerned.

## Alternatives rejected

* **A single operation with a `delete_files: bool` flag.** A signature in which
  the destruction of an original fits into a boolean is a mistake waiting for
  its caller. Two distinct names make the error hard to write and obvious to
  read.
* **Permanent erasure rather than the trash.** Simpler, with no dependency, and
  with no recourse at all for a user who gets the selection wrong. A
  dependency's cost is far below that of a lost RAW.
* **A library-internal trash** (moving into an in-house `Trash/`): it avoids
  the dependency, but invents a concept the user must learn, occupies their
  disk without their knowing, and poorly duplicates what all three systems
  already do well.
* **A "force" option at import**: see Consequences. It treats the symptom, and
  leaves two rows for one file.
* **Marking the asset as removed without erasing it** (a logical deletion): it
  would keep the fingerprint known, hence leave re-importing blocked — the very
  problem to be solved — and would weigh every query down with a permanent
  filter.
