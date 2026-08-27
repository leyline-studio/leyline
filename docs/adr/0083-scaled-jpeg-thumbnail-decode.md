# ADR 0083 — A thumbnail decodes an eighth of the JPEG, not all of it

**Status:** Accepted — 2026-08

## Context

[ADR 0082](0082-embedded-preview-at-import.md) closes by naming what it left
open: the import thumbnail pass spends **76 ms of its 122 per file decoding a
JPEG at full resolution** — 17.9 Mpx (5184×3456, and 30.1 Mpx on a 5D Mark IV)
to produce 256 pixels. It also named the obstacle: `zune-jpeg`, the decoder the
`image` crate carries here, exposes no DCT-scaled decoding.

Measured on 2026-08-27, five real embedded previews from the corpus, two
bodies, `--release`, warm cache, 20 iterations:

| decoder | output | 60D 5184×3456 | 5D IV 6720×4480 |
|---|---|---|---|
| `zune-jpeg` through `image` (**current**), full size | native | 68–72 ms | 119–127 ms |
| `jpeg-decoder`, full size | native | 79 ms | — |
| `jpeg-decoder`, DCT 1/8 | 648×432 | 35–40 ms (**×1.8**) | 66–73 ms (×1.7) |
| `mozjpeg` (libjpeg-turbo), DCT 1/8 | 648×432 | 19–22 ms (**×3.4**) | 34–39 ms (×3.3) |

### What the measurement corrects

* **Asking for an eighth does not divide the time by eight.** DCT scaling
  shrinks the inverse transform and the chroma upsampling; the **entropy
  decode walks the whole scan whatever the scale**. It is irreducible, and the
  ceiling is a property of the format, not a setting left to find.
* **A different decoder buys nothing by itself.** `jpeg-decoder` at full size
  is *slower* than what we run today (79 against 68 ms). The gain is the
  scale, never the swap; and the gap between the two decoders at equal scale
  is SIMD on the entropy decode, not algorithm.

### The path not taken

Canon RAWs embed several previews, and picking the small one would need no new
dependency. LibRaw exposes that list through `unpack_thumb_ex`/`thumbs_list`
— **from 0.21 only**, while the Linux build links the distribution's 0.20.2
(the Windows cross-build already carries 0.21.4). Building LibRaw for Linux is
something this repository already knows how to do, so that is a build cost and
not an impossibility.

It is rejected on a different fact: the small preview a Canon writes is
**160×120**, below the 256 px `PreviewKind::Thumbnail` asks for. We would fall
back to the full-size preview and its decode — the problem intact, after taking
on LibRaw's build on a third platform.

## Decision

### 1. `jpeg-decoder`, and the reason is not speed alone

`mozjpeg` is twice as fast again, and it is refused: it compiles libjpeg-turbo
from C at build time, on the three distribution targets of
[ADR 0019](0019-distribution-i18n.md) — AppImage, the cross-compiled Windows
installer, the dmg. Sixteen milliseconds per file do not pay for a fourth C
brick in a pass that already runs in the background and in parallel
(×5.3, ADR 0082 §4).

`jpeg-decoder` is pure Rust, from the same organisation as `image` which is
already a dependency, and adds nothing to any build chain.

### 2. The scale is asked for, the size is not assumed

`Decoder::scale(w, h)` picks the smallest DCT factor whose output reaches the
requested size on at least one axis, and **returns the size it settled on** —
1/8, 1/4, 1/2 or 1. The caller passes the edge it needs and reads back what it
got; nothing computes a factor.

That matters because two consumers share this path and both refuse an image
smaller than they asked for: `scan.rs::thumbnail` reduces to `THUMBNAIL_EDGE`,
and `preview.rs::file_thumbnail` returns `None` below
`max_edge(PreviewKind::Thumbnail)` — ADR 0082's rule that a blurry enlargement
is worse than a slow render. A preview too small to serve is still refused, by
the same guard, for the same reason: asking for 256 px of a 160×120 preview
returns 160×120 at scale 1, which the guard rejects exactly as it does today.

### 3. It applies to originals that are JPEGs too

`file_image` dispatches on media type, and a JPEG original went through the
same full decode — 248 ms per file, against 700 for a CR2. The corpus holds
29,000 of them. The same helper serves both, so the pass gains there too; PNG
and TIFF keep their path, having no DCT to exploit.

The develop pipeline's own decode (`source::decode`) **does not change**: it
owes full resolution to every stage downstream. Only the thumbnail path asks
for less, and only because it is about to throw the rest away.

### 4. Orientation stays where it was

`image` keeps the job of reading the EXIF orientation — headers only, no
pixels — and of applying it. ADR 0082 already documented the trap: LibRaw does
not rotate an embedded preview, most bodies tag it, and trusting both the tag
and the RAW's flip rotates twice. That logic is untouched; only the source of
the pixels moves.

Anything the scaled path cannot handle — a colour space that is not RGB, a
malformed scan — falls back to the decode that runs today rather than costing
the file its thumbnail.

## Consequences

Measured on the real corpus, `--release`, warm cache, both forms in the same
run over the same files — `file_image` as it now stands against the full
decode it replaces:

| | before | after | pixels decoded |
|---|---|---|---|
| CR2, embedded preview (12 files, 2 bodies) | 142.7 ms/file | **93.2 ms/file** (−35 %) | 25.0 → 0.39 Mpx |
| JPEG original from a body (12 files) | 124.0 ms/file | **90.0 ms/file** (−27 %) | 15.7 → 0.25 Mpx |

Both figures include reading the file and, for a RAW, LibRaw's extraction —
which is why they fall short of the ×1.8 the decode alone shows. Sixty times
fewer pixels leave the decoder; the rest of the cost was never the decode.

The pass keeps its shape: same order, same parallelism, same cache, same
`previews.origin`. What changes is the number of pixels it asks the decoder
for.

Two things to know:

* **The thumbnail is now reduced from 648×432 rather than from 5184×3456.**
  The 1/8 IDCT averages each 8×8 block, which is a reasonable low-pass filter,
  and the final resampling to 256 px follows as before. This is a different
  image, by a margin no contact sheet shows.
* **Reading and extracting become the dominant items.** Once the decode is an
  eighth, what is left is I/O and LibRaw pulling the preview out of the file.
  Whatever comes next on this path is there, not in the decoder — which is
  also why the end-to-end gain is smaller than the decoder benchmark.

`pipeline.md` §5.1 is not implicated: no stage, no stage version, no revision.
Nothing here renders a pixel that anyone develops — a thumbnail is not a
render, which is the whole point of ADR 0082 §2.

## Alternatives rejected

* **`mozjpeg` / libjpeg-turbo** — §1. Twice the gain, a C dependency on three
  packaging chains.
* **Picking a smaller embedded preview through LibRaw 0.21** — the Context
  above. The small Canon preview is below the size we need.
* **Keeping `zune-jpeg` and reducing afterwards** — that is today's code, and
  it is what the measurement condemns.
* **Decoding at 1/4 to keep more detail** — 25 ms against 19 on the mozjpeg
  column, and 1/8 already lands well above the 256 px the class stores. Detail
  we resample away is detail we paid for.
