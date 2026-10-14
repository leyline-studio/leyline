# ADR 0154 — An agent surface, served over stdio

**Status:** Accepted — 2026-10

## Context

[ADR 0121](0121-remote-engine-boundary.md) decided how something other than
Studio drives the engine: the surface is the SDK's façade and not one verb
more (§2), it lives in a new crate so the engine learns nothing (§3), and
`Event` crosses unchanged because it already says *"something changed"* and
never carries state (§5). It was written for a tablet on a local network and
has no code yet.

A second caller now has the same shape and a simpler transport: an **agent**
— a program driven by a language model — speaking the Model Context Protocol
to a server it launches itself, on the same machine, over stdin and stdout.
"Find the underexposed shots from Tuesday, lift them half a stop, show me the
result, export the three I keep" is a sequence of façade verbs. Nothing in it
needs a verb the façade does not already have.

What makes it worth doing in *this* project is the history. Every edit is a
revision in `develop_revisions`, a parent and a child, undoable by the
ordinary undo ([ADR 0142](0142-a-history-that-says-what-changed.md)). An agent working
on a Leyline library leaves a trail that can be read and walked back — **if
the trail says who wrote it**. Today it cannot: `develop_revisions.author`
exists, was reserved for collaborative development (`catalog.md` §17, §38),
and is never written.

Three facts from the code shape the decision:

* `leyline-sdk` is a **pure façade** — re-exports only, guarded by
  `tests/surface.rs`. A registry of commands, which the approved plan placed
  "at SDK level", would be the first thing in that crate that is not a
  re-export.
* The edit claim of [ADR 0120](0120-edit-session-claim.md) is registered with
  the `Library` **in process**. Two processes on one catalog — Studio and an
  agent server — do not see each other's claims. SQLite in WAL mode keeps the
  file consistent; it does not stop two programs from editing one version.
* `Library::open_read_only` already exists.

## Decision

### 1. ADR 0121 applies, with a local transport first

The surface is the façade's (0121 §2), the server is its own crate (0121 §3),
events and jobs cross unchanged (0121 §5): long work goes through the
`*_async` verbs, `JobProgress` and the cancellation of
[ADR 0139](0139-cancelling-a-batch.md). This ADR adds a transport and settles
what 0121 left to the first one; it reopens nothing.

### 2. The crate is `leyline-mcp`, and it depends on the SDK alone

`leyline-mcp` depends on `leyline-sdk`, `rmcp` (the official Rust MCP SDK,
MIT/Apache-2.0) and `tokio`. It does **not** depend on `leyline-engine`, and
that line in its `Cargo.toml` is the guard: a tool that reaches past the
façade does not compile. The engine gains no dependency.

The entry point is `leyline mcp --library <path>`, a subcommand of the CLI
calling into the crate, so a user configures one binary they already have.

### 3. The registry lives in `leyline-mcp`, not in the SDK

A divergence from the plan, for the reason above: the SDK stays re-exports
only. The registry is the table, in `leyline-mcp`, mapping a **stable tool
id** to the façade call it makes. Ids are snake-case verbs (`search`,
`apply_edits`, …) and, once released, are renamed only by adding a new id;
a client's saved prompt naming a tool is a contract like any other.

### 4. The first surface: verbs whose effect is additive or a revision

0121 §2 says *not one verb more* than the façade. It does not say every
verb, and the first cut follows one rule: **an agent gets the verbs whose
effect is either nothing, a new row in a history, or a new file.** Anything
it does can then be read back and undone with the tools the photographer
already has.

| Tool | Façade verb | Effect |
|---|---|---|
| `search` | grid query, full-text search ([ADR 0144](0144-one-search-box.md)) | none |
| `get_metadata` | asset description | none |
| `get_settings` | settings of a version | none |
| `list_versions`, `history` | versions, revision history ([ADR 0142](0142-a-history-that-says-what-changed.md)) | none |
| `render_preview` | preview of a version | none (cache) |
| `apply_edits` | an edit session, committed | a revision |
| `import` | import of a folder | new assets, references only unless asked to copy |
| `export` | export of versions | new files |

`apply_edits` takes settings as JSON and passes them through `validate()`: a
setting the pinned stage version cannot express is a **named refusal**, never
a silence (the stage capability rule). The tool opens a session, applies,
and commits before it returns — an agent's call is one revision, and no
claim outlives the call that took it, so 0120's deadline is never needed
here.

`render_preview` returns an encoded JPEG as MCP image content, its long edge
capped at 1024 px by default: an agent reasons on a look, not on pixels, and
every byte it is sent is context it pays for.

**Not in the first surface**: removing assets, renaming or moving files on
disk, deleting versions, and the metadata writes that have no history —
rating, flag, label, keywords. They overwrite, and an agent's overwrite cannot
be walked back. They enter when their writes become revisions, or by a later
ADR that says why they need not.

`verify_export` joins when ADR 0153's
provenance exists; it is not built against a promise.

### 5. Local paths are allowed

0121 §4 refuses verbs that name a path on the *caller's* disk because a
tablet has none. Over stdio the caller's disk is the library's disk, and the
server runs with the user's own rights, as the CLI does. `import` and
`export` therefore take paths. Nothing is widened: the agent can reach no
file the user who launched it could not.

### 6. Every revision records its origin

Migration **v15** adds `develop_revisions.origin TEXT`. The values are
`studio`, `cli`, and `mcp:<client>`, where `<client>` is the name the MCP
client declares at `initialize`.

* The origin is a property of the **opened library handle**, declared once by
  the client when it opens it, and stamped by the engine on every revision
  that handle writes. It is not a parameter of each write: a parameter can be
  forgotten, and a forgotten origin is a lie by omission.
* `author` keeps its meaning — a *person*, for the collaboration of §38. A
  program is not an author; overloading the column would cost that meaning
  the day it is needed.
* Revisions written before v15 keep `origin` **NULL**, read as *unknown*.
  Nothing is backfilled: a history that guesses is worse than one that admits
  a gap.
* The client name is **self-declared and unauthenticated**. It is a label, not
  an identity — trimmed, restricted to printable characters and capped at 64,
  and documented as such.

### 7. Studio and the agent: read-only first, then one engine

Step 1 (built with this ADR). Studio holds an **OS advisory lock** on a file
in the library's catalog directory for as long as it has the library open
(`flock` / `LockFileEx`): the operating system releases it when the process
dies, so a crash never leaves a stale lock. `leyline mcp` tests it without
blocking. Held → it opens with `open_read_only`, the writing tools refuse
**by name** with *"Leyline Studio has this library open; close it, or edit
from Studio"*, and the reading tools work.

Step 2 (decided here, built later). Studio serves **the same MCP protocol** on
a local socket — a Unix socket, a named pipe on Windows — reachable only by
the same user, and `leyline mcp` becomes a relay between its stdio and that
socket when Studio is open. One engine then holds the library, the agent's
revisions arrive through the existing `Event`s, and the photographer sees
them appear in the grid. There is no second wire format: the socket speaks
MCP because the registry already does.

The CLI has the same gap as the agent today and is not changed by this ADR;
it can take the same lock test when someone needs it.

### 8. Nothing leaves the machine unless the user's agent sends it

Leyline never starts an agent, contacts a model or opens a network
connection: it answers on stdin the process that launched it. The explicit
action of [ADR 0080](0080-the-promise-and-its-boundary.md)'s fifth guarantee
is the user's configuring their MCP client to launch `leyline mcp`.

What follows is outside Leyline and must be said where the feature is
documented: **the agent's model may run elsewhere**, and whatever a tool
returns — previews, captions, GPS coordinates — goes where that model is.
The user guide states it in those words.

This is also not AI in the engine, which [ADR 0102](0102-paid-extensions-and-the-pixel-boundary.md)
and `specification.md` §4 keep out. The model lives in the client; Leyline
exposes verbs, and the pixels are still computed by stages whose versions
are pinned.

## Consequences

* A new crate, `leyline-mcp`, and two dependencies outside the engine's tree.
* Migration v15 and an origin carried by the `Library` handle; Studio and the
  CLI declare theirs at open.
* Studio takes an advisory lock on open. A second Studio on the same library
  is unaffected by this ADR — it does not test the lock.
* An end-to-end test launches `leyline mcp` over stdio, then imports, edits,
  previews and exports, and checks that the revision's `origin` reads
  `mcp:<client>`.
* `architecture.md` gains the crate when it is built, not before.

## Rejected

* **A registry in the SDK** — the first non-re-export in a crate whose only
  failure mode was a hole; it would also make every client compile a table
  only one of them reads.
* **`origin` in `author`** — a program is not a person, and §38 needs the
  column for people.
* **Every façade verb at once** — an overwrite an agent makes cannot be
  walked back, and the history is the argument for doing this at all.
* **A second protocol for Studio's socket** — two wire formats for one
  registry is a translation layer that can disagree with itself.
* **Blocking until Studio closes** — a frozen tool call says nothing; a named
  refusal says what to do.
