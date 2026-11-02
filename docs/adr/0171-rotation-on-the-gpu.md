# ADR 0171 — Rotation on the GPU

**Status:** Accepted — 2026-11

## Context

`docs/measured-findings.md` §B6 ranks the stages the GPU preview cannot run
by what they cost a drag. `rotate` v1 (rank 200) sits after every checkpoint
of ADR 0156 §4, so **any straightening sends every drag to the CPU**, Basic
sliders included. `local_adjustments` (rank 160) costs three checkpoints of
four, `lut` and `perspective` all four. No real edit history exists on this
machine to weigh them by use (§B6), so the choice is made on rank and cost:
of the stages that block every drag, rotation is the one photographers
reach for most in any editor, and it is a pure resampling with no per-mask or
per-file state. `local_adjustments` stays on the CPU until a measurement on
real edits says masks dominate.

## Decision

1. **`Op::Rotate { degrees }`** in `leyline-gpu`, built by the engine's kernel
   table for `("rotate", 1)` from `settings.rotation`. The angle's sine and
   cosine and the rotated canvas are computed in `f64` exactly as `rotate` v1
   does (`rem_euclid`, the bounding box rounded, at least 1 px), then handed
   to the shader in `f32`.
2. **A geometric step of the output pass, like the crop.** Rotation changes
   the frame's size, and in rank order only `perspective` (no kernel, so the
   GPU is not used at all when it is active), the crop, the vignette, the
   grain and the output follow it. So rotation is not a pass over `work`: the
   output pass maps each of its pixels through the inverse rotation into
   `work` and samples it with v1's bilinear rule (centres at `n + 0.5`,
   black outside the source, clamped neighbours). A crop after it is
   computed on the rotated size; vignette and grain keep their window
   coordinates. Any other operation after a rotation makes the render
   answer `None`, the CPU's cue.
3. **Output buffers sized on demand.** The rotated bounding box can hold up
   to twice the source's pixels (at 45°). The frame keeps its source-sized
   output and readback, and grows a second pair only when a rotated window
   needs more, kept for the frame's later renders.
4. **Accuracy:** the same bar as every kernel, at most 1 level of 8-bit
   output against the CPU on the comparison test, with rotation alone and
   rotation under a crop. The geometry is `f32` on the GPU and `f64` on the
   CPU; at preview sizes the difference is far below a pixel, and the
   preview is not under §5.1 (ADR 0156 §1).

## Rejected

- **A rotation pass over `work` into a resized buffer**: every kernel after it
  would need the new size, for stages that today are only the crop and two
  output-time effects.
- **`local_adjustments` first**: three checkpoints instead of four, many
  times the work (every mask type, every per-mask value), and nothing
  measured says masks are used more than straightening.

## Consequences

- A straightened photo keeps its drags on the GPU unless it also carries a
  mask, a LUT or a perspective correction. `KERNEL_LESS_AFTER_FIRST_CHECKPOINT`
  loses `rotate`.
- `perspective` (rank 205) would fit the same mechanism (a per-pixel inverse
  map at output time); it is not written until a measurement asks for it.
