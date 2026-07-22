# Leyline

Leyline is an open-source, non-destructive RAW photo development platform:
**Leyline Engine** (the library) and **Leyline Studio** (the desktop app
built on it).

This book is a guided tour of the workspace's crates — what each one owns,
and *why* it's shaped the way it is. It's deliberately not a copy of the
API reference (that's `cargo doc`) or the full specification (that's
[`docs/`](https://github.com/leyline-studio/leyline/tree/main/docs) and its
[ADRs](https://github.com/leyline-studio/leyline/tree/main/docs/adr)) —
each chapter here links out to those for the details, and focuses on
orienting a new reader: where does this code live, what is it responsible
for, and what decision led to that shape.

## Where to look for what

| Question | Where |
|---|---|
| What does V1 include/exclude? | `docs/specification.md` |
| What's the guiding philosophy? | `docs/vision.md` |
| What does crate X do, and why? | this book |
| What's the exact API surface? | `cargo doc --open` |
| Why was a specific decision made? | `docs/adr/` |
| How is a RAW file's develop pipeline defined? | `docs/pipeline.md` |
| How is the catalog (SQLite) shaped? | `docs/catalog.md` |
| What can the engine API do? | `docs/engine-api.md` |

## Core constraints

Three constraints shape every crate described in this book (see
`docs/vision.md`):

- **Non-destructive** — RAW files are never modified; edits are a pipeline
  of independent steps, replayed on demand.
- **Local First** — fully offline, no cloud, no accounts, no subscription.
- **Modular** — the engine has no GUI dependency; Studio, the CLI, and the
  SDK are all just clients of it (see [Architecture](./architecture.md)).
