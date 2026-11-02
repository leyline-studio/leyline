# ADR 0170 — A failing GPU hands the session to the CPU

**Status:** Accepted — 2026-11

## Context

[ADR 0156](0156-a-gpu-for-the-preview-alone.md) §2 promises that "a missing
adapter, a lost device or a failed allocation sends the preview back to the
CPU path". Only the first case was built. `leyline-gpu` installs no handler
for wgpu's errors, and **wgpu's default uncaptured-error handler panics**:
a validation error, an out-of-memory allocation, or a device the driver took
away (a driver update, a TDR reset on Windows, a laptop switching GPUs)
became a panic on the thread rendering the preview. Under
[ADR 0152](0152-a-panic-is-not-an-error-message.md) that is a crash with a
bug report, for a path that is optional by design.

0.2.0 shipped the GPU preview on in every deliverable, so the hardware it
meets is no longer one RTX card.

## Decision

1. **wgpu errors are recorded, not raised.** `Gpu` installs an
   uncaptured-error handler and a device-lost callback. Each writes the
   **first** failure's text into the `Gpu` and returns; `Gpu::failure()`
   reads it. Native wgpu calls the handler on the very call that failed, so
   by the time a render returns, its failure is known.
2. **A failed device is retired for the session.** Once `failure()` is set,
   the engine's `gpu()` answers `None`: `covers`, `live_on_gpu` and every
   later render take the CPU path, and *About* says CPU. The frame of the
   render that failed is **discarded**, even if a readback produced bytes,
   and the CPU renders that frame instead. No retry and no new device within
   the session: a device that failed once on a driver is not trusted to
   draw the next frame correctly, and the CPU path is always correct.
3. **A panic inside the GPU call is contained the same way.** The engine
   runs the GPU part of a render through `panic_net::isolate`
   ([ADR 0152](0152-a-panic-is-not-an-error-message.md) §5): the panic hook
   still prints and records it (it is a bug, ours or a driver's, worth a
   report), and the device is then retired as in §2.
4. **One line on stderr**, once: `GPU preview off for this session: <reason>`.
   Nothing on screen: the preview keeps working, only slower, and ADR 0156
   §2 already says Studio speaks of the backend only when asked.
5. `Gpu::destroy()` gives the device up on purpose, which is what a driver
   reset does; the tests use it to prove §1–§2 on any machine, llvmpipe
   included.

## Rejected

- **Error scopes around every render.** They catch the same errors, but each
  `pop_error_scope` is a future to block on per frame, and the uncaptured
  handler is needed anyway for errors outside a scope (a lost device).
- **Re-acquiring a device after a loss.** Possible, but it reopens every
  pipeline on a driver that just failed, mid-drag, for a speed-up the CPU
  path makes optional. A restart of Studio acquires afresh.
- **Telling the photographer.** A dialog for a slower preview is noise; the
  stderr line and *About* are where a support exchange looks.

## Consequences

- No wgpu error can end Studio any more; the worst case is a CPU preview
  until restart.
- A real GPU fault leaves a trace (stderr; `last-panic.txt` for a panic)
  instead of a crash dump, so it is still diagnosable.
- Not tested on an Intel iGPU, as the 2026-10-20 review hoped: none is
  available. The loss itself is tested through `Gpu::destroy()`.
