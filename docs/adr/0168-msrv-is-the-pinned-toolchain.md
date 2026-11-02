# ADR 0168 — The declared MSRV is the pinned toolchain

**Status:** Accepted — 2026-11

## Context

The workspace `Cargo.toml` declared `rust-version = "1.85"` while
`rust-toolchain.toml` pins **1.97.1**. Nothing ever built the workspace at
1.85, and the figure was already false on 2026-11-02:

- the dependency graph requires more: `lopdf`, `printpdf` and `rmcp` declare
  1.88, `weezl` 1.88.0 (read from `cargo metadata`);
- our own code uses edition-2024 `let` chains (`grid.rs`, `kernel/v3.rs`,
  `events.rs`), stable only since 1.88.

A false MSRV is not harmless. Clippy's `incompatible_msrv` lint and Cargo's
resolver both trust it, and it already misled one decision: wgpu was pinned
at 26.0.1 because its MSRV "fits the workspace's 1.85"
(`docs/measured-findings.md`). The `fs4::try_lock` pitfall showed the other
direction: a std method that appeared after the declared version shadowed a
dependency's trait method, and the declared figure said nothing about it.

There are two ways to make the figure true. **Test an older version** in CI
(a third check leg, kept green forever, for builders nobody has met), or
**declare what is built**. [ADR 0086](0086-decoder-in-the-promise.md) and
`docs/pipeline.md` §5.2 already make the toolchain an input of the render: a
build at any other version is outside the reproducibility promise, so an
older MSRV would promise a build whose pixels the project does not stand
behind.

## Decision

1. The workspace `rust-version` is the **major.minor of the channel pinned
   in `rust-toolchain.toml`**: `1.97` today. Every crate inherits it
   (`rust-version.workspace = true`, unchanged).
2. A test in `leyline-core/tests/docs.rs` reads both files and fails when
   they disagree, so bumping the toolchain without the MSRV (or the reverse)
   breaks `make check`, the way a stale ADR count already does.
3. Raising the toolchain stays the deliberate act `rust-toolchain.toml`
   describes (replay the golden renders, record any drift); the MSRV follows
   in the same commit.

## Rejected

- **Keep 1.85 and add a CI leg at 1.85**: impossible without downgrading
  four dependencies and rewriting the `let` chains, for no known user.
- **Declare 1.88, the floor the graph computes today**: true for one day,
  untested from the next, which is exactly the silent divergence this ADR
  removes.
- **Remove `rust-version`**: Cargo and Clippy then assume any version, and
  the lint that catches a too-new std API is lost.

## Consequences

- Someone building with an older rustup toolchain gets Cargo's clear MSRV
  error instead of an obscure compile failure. `rust-toolchain.toml`
  already installs the right one automatically.
- A dependency can now be chosen up to the pinned toolchain; the wgpu pin's
  stated reason (MSRV 1.84) no longer holds and its note says so. Upgrading
  wgpu is a separate decision (it moves the GPU path, ADR 0156).
