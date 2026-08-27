# ADR 0007 — A develop model inspired by Git

**Status:** Accepted — 2026-07

## Context

Non-destructive development demands a dependable history: undo/redo, snapshots, virtual copies. A single JSON overwritten on every setting change — the initial model — made history impossible without a rewrite.

## Decision

Settings form a graph of **immutable revisions** (`parent_revision_id`), **versions** are named branches (a head pointer), and the current version is a plain pointer. A virtual copy is a branch, not an asset row.

Details: `catalog.md` §16–18, coalescing in §17, process versions in `pipeline.md` §3.3.

## Consequences

* Undo/redo is a pointer move; the complete history comes for free; previews are revalidated by revision identifier.
* Virtual copies with no duplication of file or metadata.
* Volume kept in check by coalescing (one revision is one intent, never one interface event).
* Complexity accepted: two tables more than a plain JSON field.

## Alternatives rejected

* **A single overwritten JSON (Lightroom)**: no history, which contradicts the project's goals.
* **A linear history stack (Darktable)**: no branches, and virtual copies are duplicated.
* **Deltas rather than complete states**: replaying a chain of deltas for every render, and fragility if one link is corrupted.
