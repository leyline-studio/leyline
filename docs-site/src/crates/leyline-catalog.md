# leyline-catalog

> SQLite catalog: libraries, assets, versions and revisions.

## What it does

Owns `catalog.db` end to end: opening/creating it, applying the
per-connection configuration (`docs/catalog.md` §6), and running the
incremental `user_version` migrations (§34). It stores references,
metadata, edit settings, collections and indexes — **never the photos
themselves** (`docs/vision.md`).

## Why it's built this way

The catalog is explicitly an **implementation detail of the engine, never
a public interface** (`docs/engine-api.md` §14) — clients (Studio, the
CLI) always go through `leyline-sdk`, never open `catalog.db` directly.
That indirection is what lets the catalog's schema evolve (new migrations)
without becoming part of the SDK's semver contract.

SQLite itself was chosen for the Local First constraint: no server
process, a single file, and the reliability of a widely embedded engine
(`docs/adr/0003-sqlite.md`). Paths are stored relative to the library root
(`docs/adr/0010-relative-paths.md`) so a library stays portable if the
folder it lives in is moved.

## See also

* `docs/catalog.md` — schema specification
* `docs/adr/0003-sqlite.md`, `0010-relative-paths.md`
* `docs/adr/0023`, `0024` — catalog-lock narrowing for preview/export
