# ADR 0013 — Process version 2: sRGB transfer functions by table

**Status:** Accepted — 2026-07

## Context

In `process 1`, white balance and exposure convert every sample to linear
light and back (`srgb_to_linear`, `linear_to_srgb`), two `powf` calls per
sample — the dominant cost of the per-pixel operators. A precomputed table
with linear interpolation is far faster, but its results are not bit-for-bit
those of the exact formulas: under `docs/pipeline.md` §3.3 and ADR 0012, such
a change demands a new process version.

## Decision

The engine introduces `process: 2`, defined in its own frozen module
(`process2.rs`), identical to `process 1` but for one difference: the two
sRGB transfer functions of the linear-light operators go through tables of
4096 intervals (4097 entries, entry `i` being the exact formula evaluated at
`i / 4096`) with linear interpolation in `f32`. Table size and interpolation
mode are part of the render contract: changing them would demand a
`process: 3`.

`CURRENT_PROCESS` moves to 2: new revisions write `process: 2`. Existing
revisions declare `process: 1` and go on being rendered by the `process1`
module, unchanged forever — as the rule "a given engine must know how to
render every past process version" requires. An edited revision inherits its
parent's process: no implicit migration; the deliberate migration
(reprocessing, `pipeline.md` §4.5) creates a new revision.

## Consequences

* The approximation error is < 2·10⁻⁵ in the gamma domain: invisible in
  8-bit output (< 0.005 of a quantization step), but not bit-identical —
  hence the new version. A test bounds the process 1 / process 2 difference
  to one 8-bit step at most.
* The `process2.rs` module duplicates the unchanged operators of `process 1`
  rather than sharing them: that is the accepted price of freezing ("the
  same pixels in ten years"), a future fix to a process 3 operator never
  being allowed to alter past renders.
* The benchmarks compare both versions (`cargo bench -p leyline-engine`,
  groups `process1` and `process2`).

## Alternatives rejected

* **An exact table without a new version**: the entries of a table indexed on
  the 256/65536 quantized values of the decode would be bit-exact, but only
  for the pipeline's first operator — the gain would not cover
  `linear_to_srgb`, whose input is continuous.
* **A polynomial approximation of `powf`**: the same contractual
  consequences as a table, with precision harder to bound.
