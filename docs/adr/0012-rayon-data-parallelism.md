# ADR 0012 — Data parallelism with Rayon in the engine

**Status:** Accepted — 2026-07

## Context

Phase 7 aims at interactive renders. The operators of `process 1` are
per-pixel or per-row: they lend themselves to data parallelism. But the
reproducibility contract (`docs/pipeline.md` §5) demands a strictly
deterministic render: one `settings_json` must produce the same pixels
whatever the number of threads. The thread count is precisely the kind of
variable §5.1 refuses to let into the result — unlike the platform and the
toolchain, which §5.2 places outside the guarantee.

## Decision

The engine's hot loops use Rayon (`par_chunks_mut` over rows). The absolute
rule: parallelization never changes the scalar formula nor the order of
operations *for a given sample*. Every row is computed independently and no
cross-thread floating-point reduction is allowed — the result stays
bit-for-bit identical to the single-threaded run, which the existing render
tests verify, and, since ADR 0042 §7, so do the reference renders of
`stages/golden.rs`, none of whose fingerprints depends on the thread count.

A frozen process version can therefore be parallelized after the fact: that
is not a change of rendering in the sense of `docs/pipeline.md` §3.3.

## Consequences

* At 3 MP, a complete edit falls from 709 ms to 167 ms (−76 %) on the
  reference machine; the `cargo bench -p leyline-engine` benchmarks track
  those figures.
* Rayon has its own global pool: the engine stays free of an async runtime
  (consistent with ADR 0011 — native threads).
* Any future optimization that changed the order of floating-point additions
  (horizontal SIMD, parallel reductions) would have to go through a new
  process version.

## Alternatives rejected

* **Manual threads and channels**: reinventing proven work-stealing, for no
  gain.
* **GPU (wgpu)**: larger gains, but determinism across GPUs is not
  guaranteed; deferred to a later phase-7 exploration, behind a new process
  version if need be.
