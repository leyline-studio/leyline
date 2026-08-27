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

## Alternatives rejected

* **Absolute paths (historical Lightroom)**: the classic source of catalogs broken by a migration.
* **Photos outside the library folder by default**: technically possible (`copy_files: false` at import), but a self-contained folder stays the nominal case.
