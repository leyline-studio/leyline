# ADR 0156 — A GPU for the preview alone

**Status:** Accepted — 2026-10

## Context

[ADR 0012](0012-rayon-data-parallelism.md) chose the CPU and deferred the
GPU; `measured-findings.md` §B3 kept it closed in August, because the stage
cache (§B1) had made a drag fluid **on a 1024-px proxy**. That proxy is what
the loupe of a 1440p or 4K screen enlarges: fluid, and soft. The user's
choice for the year's work, recorded with the plan of 2026-10-13, is a
preview that is fluid **at the screen's resolution**.

§B5 measured what that costs on the CPU (i9-9900K, 16 threads, the
reference machine), through the path Studio's drag takes:

| Frame | Early slider | Late slider |
|---|---|---|
| 1024 px | 7–19 ms | 13–25 ms |
| 2048 px | 39–42 ms | 46–47 ms |
| 4096 px | 161–172 ms | 194–199 ms |

The threshold fixed before measuring — 33 ms at 4K — is missed five to six
times over, and §B4 already showed that the CPU path has no cheap lever left
(parallel efficiency spent, SIMD empty). Enlarging the proxy is therefore
not an answer, and this ADR is the one §B3 said would have to be written.

What has not changed is §B3's argument: inter-GPU determinism is not
guaranteed, and [`pipeline.md`](../pipeline.md) §5.1 promises the export bit
for bit. [ADR 0080](0080-the-promise-and-its-boundary.md) §3 reads §5.1 as
bearing on the **render** — export and print — and the preview is already a
separate path, already not bit-for-bit with the export (a reduced proxy,
scaled radii).

## Decision

### 1. The GPU renders the preview, never the export

A new crate, **`leyline-gpu`**, built on **wgpu** in compute mode. It serves
`Library::preview_live` and the develop view's renders, and nothing else.
Export, print, the golden manifest and `verify`/`reproduce`
([ADR 0153](0153-an-export-knows-where-it-comes-from.md)) stay on the CPU:
nothing in §5.1 moves, and no stage version declares a backend, because no
GPU pixel is ever stored or compared to a pinned one.

### 2. Optional, and the CPU is always there

The engine consumes `leyline-gpu` behind a cargo feature `gpu`, on in the
deliverables. At run time the GPU is used only when an adapter is found and
a device created; a missing adapter, a lost device or a failed allocation
sends the preview back to the CPU path, which is allowed precisely because
the preview is not under the promise. The system floor of
`system-requirements.md` (4 GB, two cores, llvmpipe) stays valid without a
GPU, and Studio says nothing about the backend unless asked (*About*).

### 3. A kernel is added next to a stage version, never in its place

`Version` gains an optional **preview kernel**. A frozen stage keeps its
`apply` untouched; the kernel is a second implementation of the same
operator, for the preview only. A stage version without a kernel is simply
one the GPU cannot run.

Each kernel is held to **±1/255 per 8-bit channel of the final preview
against the CPU preview** of the same settings, on a fixed set of images
including a real RAW, tested on a software adapter (lavapipe) so the test
runs on every machine that runs the gate. That is a tolerance on a view,
not on a render: it says the photographer sees what the export will
contain, not that the bytes are the same.

### 4. Hybrid, at the stage cache's seam

The CPU decodes, builds the proxy, and runs the plan up to the stage
cache's resume point (§B1, `StageCache`). From there:

* if **every** remaining active stage has a kernel, the GPU runs them,
  starting from a copy of the resume buffer **it keeps resident** — at 4K
  that buffer is ~134 MB of `f32`, which a per-frame upload would spend most
  of the 33 ms on;
* otherwise the CPU runs the rest, exactly as today.

The result is read back to the CPU as 8-bit RGB and handed to Slint as
every preview is today; no texture sharing with Slint's renderer is assumed,
and the read-back is measured with the rest.

### 5. Kernels in the order sliders are dragged

1. Point operators on the linear and display axes — gains (white balance,
   exposure), contrast, highlights/shadows, whites/blacks, the parametric
   and point curves, levels, saturation and vibrance, HSL, colour grading,
   monochrome — and the output rendering, without which nothing can be read
   back.
2. Neighbourhood operators — clarity, texture, sharpening, dehaze — each
   measured before and after, and each kept only if it pays.

Until a drag's remaining stages are all covered, that drag stays on the CPU,
so the first slice is useful for the Basic panel and nothing is slower than
today for the rest.

### 6. Studio asks for the screen's resolution only when it pays

The live loop of [ADR 0074](0074-live-preview-while-dragging.md) asks for a
frame sized to the loupe instead of 1024 px **when the GPU path will run
it**, and keeps 1024 px otherwise. The engine says which, before the drag
starts.

### 7. Before the first kernel

Two checks, written down as passed before any kernel is written:

* the Windows cross-build (mingw, [ADR 0019](0019-distribution-i18n.md)) links wgpu,
  and the deliverable without a GPU still starts;
* wgpu's glibc floor fits the published one (2.39) and does not raise it.

## Consequences

* A second implementation of the hottest operators, to be kept in step with
  every new stage version that has a kernel. A stage version that changes
  the CPU pixels without its kernel loses the GPU path until the kernel
  follows — never the other way round.
* A large dependency (wgpu and its backends) enters the workspace, behind a
  feature and in one crate.
* The tolerance test is the contract: a kernel that drifts beyond 1/255 is
  a failing gate, not a visual difference someone may notice.

## Rejected

* **The GPU for export**, even as a new stage version per operator
  (§B3, ADR 0080 §3): allowed by the promise, but every kernel would then be
  a published version kept forever, for an export that has no deficit to
  catch up (§B2).
* **A bigger proxy on the CPU** — §B5: 4K costs 160–200 ms.
* **Texture sharing with Slint's renderer**: it ties the engine to one
  renderer and one backend, for a read-back §4 measures rather than assumes.
* **A silent fallback inside a frame** (GPU for some stages, CPU after):
  the copy back and forth costs more than it saves; §4's rule is per drag.
