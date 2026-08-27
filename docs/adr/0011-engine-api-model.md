# ADR 0011 — Engine API: a Rust library, synchronous queries, asynchronous jobs

**Status:** Accepted — 2026-07

## Context

The engine serves several clients (Studio, CLI, scripts). It needs a stable boundary, responsive enough for a UI, without imposing infrastructure on lightweight clients.

## Decision

* The API is a **Rust library** (`leyline-sdk`), not a server and not a protocol.
* **Synchronous catalog queries** (SQLite answers in microseconds); **asynchronous heavy jobs** (`JobId` plus an event stream).
* No async runtime imposed: native threads and standard channels.
* Events are notifications, never data: the client re-queries.
* Editing goes through `EditSession`, which implements revision coalescing.

Details: `docs/engine-api.md`.

## Consequences

* Studio, the CLI and scripts call strictly the same API — "API before GUI" is structural, not declarative.
* No tokio dependency in the SDK; Slint integration through a plain channel.
* A C FFI gateway stays possible (signatures exposing neither generics nor lifetimes).
* The SQLite schema is **not** a public API: clients go through the SDK.

## Alternatives rejected

* **A local server (gRPC/HTTP)**: serialization and latency unjustified for in-process work; still possible later, on top of the SDK.
* **A fully async API (tokio)**: imposes a runtime on every client, including a three-line CLI.
* **Direct SQLite access from clients**: coupling to the schema, and the end of any freedom to migrate.
