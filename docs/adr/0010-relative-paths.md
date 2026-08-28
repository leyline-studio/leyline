# ADR 0010 — A self-contained library, relative paths

**Status:** Accepted — 2026-07

## Context

A library has to survive moving to another disk, another machine or another OS (`C:\…` vs `/home/…`), and has to be backed up by a plain copy.

## Decision

A library is a self-contained folder (`catalog.db`, `Photos/`, `Cache/`, `Exports/`, `Backups/`). The catalog stores **no absolute path**: every reference is relative to the root, with `/` as the separator.

An asset's path is always derived (`folders.relative_path` + `filename`) — never stored twice.

## Consequences

* Complete portability across Windows, Linux and macOS; backup and restore are a folder copy.
* Moving the library breaks nothing; renaming a folder desynchronizes nothing.
* One documented exception: export destinations (`export_history.destination`) point outside the library.
* The limit this draws, and it is a real one: **one library covers one tree**. A catalog cannot span two volumes, and a collection already on disk is catalogued in place only by choosing a root above it. Lightroom does span volumes — it anchors each photo to a *named volume* plus a path, and shows the photos of an unplugged disk as offline — which is exactly the absolute path rejected here, with exactly the fragility this ADR is about. Lifting the limit without giving that up (several named roots registered in the catalog, every path still relative to its own root) is a decision for another ADR, not an adjustment of this one.

## Alternatives rejected

* **Absolute paths (historical Lightroom)**: the classic source of catalogs broken by a migration.
* **Photos outside the library folder**: refused, and `copy_files: false` at import does not open that door. Referencing in place skips the copy into `Photos/`; it does not lift the rule — `reference_in_place` (`crates/leyline-engine/src/import.rs`) resolves the file against the root and skips anything else with `file is outside the library root`. Cataloguing a collection where it already lives is done by **putting the library root above it**, never by pointing outside.
