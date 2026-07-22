# Architecture

## Workspace

```text
{{#include ../../docs/architecture.md:5:19}}
```

| Crate | Role |
|---|---|
| `leyline-core` | shared types, ids, errors |
| `leyline-engine` | orchestration: jobs, events, rendering |
| `leyline-raw` | RAW decoding (LibRaw) |
| `leyline-catalog` | SQLite catalog |
| `leyline-preview` | thumbnail/preview cache |
| `leyline-color` | color management (ICC) |
| `leyline-lens` | lens corrections (Lensfun) |
| `leyline-export` | image encoding (JPEG/TIFF/WebP/AVIF) |
| `leyline-sdk` | stable public API |
| `leyline-cli` | command-line client |
| `leyline-studio` | desktop application (Slint) |

## Dependency direction

```text
{{#include ../../docs/architecture.md:23:23}}
```

Catalog, RAW, Color, Lens, Preview, and Export are consumed by Engine —
they never depend on each other, only on Core, and never appear in Studio's
or the CLI's dependency list directly.

This one-way arrow is the load-bearing rule of the workspace: **the engine
is fully independent of the GUI**. Studio, the CLI, and the SDK are all
just clients of `leyline-engine`, calling exactly the same surface — which
is also why the CLI has no logic of its own beyond argument parsing (see
[leyline-cli](./crates/leyline-cli.md)), and why anything Studio can do,
a script can do too.

`leyline-sdk` is the crate that makes this contractual rather than just
conventional: it re-exports the stable slice of `leyline-core` and
`leyline-engine` that clients are allowed to depend on. `leyline-engine`
itself stays free to change internals at every version; `leyline-sdk` is
the semver boundary (see [leyline-sdk](./crates/leyline-sdk.md)).

## No circular dependencies

Enforced by the dependency graph above, not just convention — a change
that would require e.g. `leyline-raw` to depend on `leyline-catalog` is a
sign the responsibility boundary is wrong, not a reason to add the edge.

## Technical choices

See `docs/architecture.md` §"Choix techniques" for the full, authoritative
list (Rust, Slint, SQLite, LibRaw, Lensfun, LittleCMS, `rfd`) and the ADR
linked from each per-crate chapter for the reasoning behind each choice.
