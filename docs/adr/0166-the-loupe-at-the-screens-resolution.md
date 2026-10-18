# ADR 0166 — The loupe at the screen's resolution

**Status:** Accepted — 2026-10

## Context

[ADR 0156](0156-a-gpu-for-the-preview-alone.md) §6 says Studio asks for a
frame sized to the loupe « when the GPU path will run it », and the engine
says which. Two slices of kernels later (`measured-findings.md` §B5), a drag
of the Basic panel's tonal sliders renders at 4K in 30–60 ms on a GPU
instead of 120–460 ms on the CPU. Nothing of that reaches a photographer
yet: every image of the develop view — the committed render, the live
frames, before/after, the mask overlay, the histogram — is the `Small`
class, 1024 px, enlarged by the loupe.

Writing §6 down as code exposed three things it does not settle:

1. **The stage cache holds one context.** The develop view renders the
   committed revision at `Small` (it is cached on disk, and the histogram
   and clipping read it). A loupe-sized render next to it would retarget
   the cache at every call, each discarding the other's checkpoints: every
   release would become two full renders.
2. **The first loupe-sized render is slow by nature.** It needs a full
   decode (no half-size) and has no checkpoint yet: one to three seconds of
   CPU on a 30 Mpx RAW. On the UI thread, that is a frozen window at every
   photo opened.
3. **« Will the GPU run it » is a question about the cache.** The kernels
   cover the stages from rank 40; whether a given frame runs on the GPU also
   depends on a valid checkpoint before them. Dragging the noise reduction
   or a lens slider invalidates it, and that drag, asked at 4K, would run on
   the CPU at 4K.

## Decision

### 1. `Small` stays the develop view's truth

The committed render, the disk cache, the histogram, the clipping counts,
before/after, the soft proof and the mask overlay keep the `Small` class,
exactly as today. The loupe-sized image is **an upgrade of the view**, never
stored, and the view is always correct without it.

### 2. The upgrade arrives off the UI thread

When the GPU is available, after each refresh of the develop view a
background render of the head at the loupe's class (`preview_live`, nothing
written) is started; when it lands, the viewer shows it if the photo, the
version and the revision are still the ones on screen, and drops it
otherwise. The first one of a photo pays the full decode in the background;
the UI shows the `Small` render meanwhile.

### 3. A live frame is loupe-sized only when the GPU will render it

`Library::live_on_gpu(asset, class, settings)` answers whether
`preview_live` at that class would run on the GPU **now**: a device, a
valid checkpoint for those settings in that class's cache context, and a
kernel for every stage after it. Studio asks it per frame and renders the
frame at the loupe's class when it says yes, at `Small` otherwise — so a
drag the GPU cannot take costs what it costs today, never more.

### 4. The stage cache keeps two contexts, and a large one keeps one checkpoint

The cache keeps the two most recent contexts (asset, scale, input size), so
the `Small` refresh and the loupe-sized upgrade stop discarding each other.
A context whose frame is longer than 2048 px keeps **only its first
checkpoint** — the one before rank 40 that the GPU resumes from — because
at 4K each checkpoint is ~134 MB of `f32`, and deeper ones only serve a CPU
drag that §3 already sends to `Small`.

### 5. The loupe's class

`Small` up to 1024 px of loupe, `Medium` up to 2048, `Large` above, from the
viewer's size in physical pixels. A window that grows to 4K raises it at
the next refresh.

## Consequences

* Without a GPU, nothing changes: same classes, same renders, same cache
  behaviour (the second context is the only cost, and it is empty).
* With a GPU, memory grows by the large context: one checkpoint of up to
  ~134 MB, plus the GPU's resident copy on the device.
* A release shows the `Small` render first and the loupe-sized one a few
  tens of milliseconds later; on a freshly opened photo, seconds later.
* The histogram keeps describing the `Small` render. At this level of
  aggregation the two do not differ in any way a histogram shows.

## Rejected

* **The develop view entirely at the loupe's class.** Every commit would
  encode and re-read a 4K PNG for the disk cache, and opening a photo would
  block on the full decode.
* **Deciding the class once per drag** from the kernels alone: §3's case —
  an early slider invalidates the checkpoint — would make that drag a CPU
  render at 4K.
* **A byte budget across contexts** instead of §4's rule: more general, and
  for a case — CPU drags at 4K — that §3 removes.
