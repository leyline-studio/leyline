# ADR 0003 — SQLite as the catalog's only database

**Status:** Accepted — 2026-07

## Context

The catalog has to handle hundreds of thousands of assets, work offline, fit in a file that a plain copy backs up, and stay readable decades from now.

## Decision

The catalog is a single SQLite database per library (`catalog.db`), in WAL mode, with FTS5 for free text. Complete schema: `docs/catalog.md`.

## Consequences

* No server, no installation; a backup is a folder copy.
* One of the industry's most durable file formats (SQLite is a recommended archival format at the Library of Congress).
* One writer per library (a lock); many readers through WAL.
* Full-text search with no external index (FTS5 is built in).

## Alternatives rejected

* **PostgreSQL**: a server to administer, contrary to Local First.
* **Sidecar files alone (the Darktable XMP model)**: search and collections become impractical at scale; XMP stays an optional export (`catalog.md` §29).
* **Embedded key-value stores (sled, RocksDB)**: no relational queries, and a less durable format.
