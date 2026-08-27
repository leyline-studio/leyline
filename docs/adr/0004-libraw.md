# ADR 0004 — LibRaw (the LGPL branch) for RAW decoding

**Status:** Accepted — 2026-07

## Context

RAW decoding demands coverage of hundreds of proprietary formats (CR3, NEF, ARW, RAF…), which keep evolving with every new camera body.

## Decision

`leyline-raw` builds on LibRaw, used exclusively under its **LGPL-2.1** branch (its CDDL branch is incompatible with the project's GPL-3.0).

Decoding is isolated behind `leyline-raw`'s API: no other crate ever sees LibRaw.

## Consequences

* Immediate format coverage, maintained by an established project.
* C FFI confined to a single crate.
* For a future proprietary edition: dynamic linking is required (the LGPL substitution obligation).
* A documented plan B: `rawler` (a pure-Rust RAW decoder, LGPL-2.1) can replace LibRaw behind the same API should the need arise.

## Alternatives rejected

* **rawler alone**: pure Rust and appealing, but its format coverage and maturity fall short of LibRaw's today — kept as the fallback.
* **dcraw**: abandoned upstream.
* **Decoders of our own**: years of work to catch up with what already exists.
