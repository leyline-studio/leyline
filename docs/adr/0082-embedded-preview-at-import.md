# ADR 0082 — Import shows the body's embedded preview, it does not render it

**Status:** Accepted — 2026-08

## Context

`Library::import` renders one thumbnail per imported file, serially, through the
full pipeline — sensor decode included. Measured on 2026-08-26, `--release`, on
real CR2s: importing 10 files takes **6.95 s**, of which **0.1 s** is the actual
import.

| Item, per file | Cost |
|---|---|
| BLAKE3 over the file | 5 ms |
| Copy under `Photos/` | 4 ms |
| LibRaw `identify` | 0.4 ms |
| `add_asset` (the 5 writes) | 0.2 ms |
| Everything else in `import_one` | ~10 ms |
| **`generate_import_thumbnails`** | **~680 ms** |

On the real test corpus — 15,000 CR2s — that is **2 h 50, of which five minutes
are the import**. The rest is one sensor decode per file, paid to produce a
256 px image.

The function's comment already stated the verdict: *"Left serial; worth
revisiting if import-time thumbnailing shows up in the perf benches."* It has,
and serialization is not the subject: the decode is.

### The decision is already made, one step earlier

[ADR 0065](0065-selective-import.md) §2 is titled "The preview comes from the
file, never from the pipeline", and its justification is word for word this
one's: *"the whole point of the exercise is precisely not to pay a decode per
file before knowing which ones we keep"*. The scan, which precedes the import,
has therefore been right since 2026; the import, which follows it, pays exactly
what the scan refused.

ADR 0065 §2 gave a precise reason not to keep those previews: *"the cache is
indexed by asset, and these files do not have one"*. After the import, the asset
exists. The reason is gone, the decision can cross over.

### What measurement imposes on the design

Seven CR2s, two bodies (60D and 5D Mark IV), five folders of the corpus:

* **The embedded preview is a full-size JPEG** — 5184×3456 in all seven cases,
  1.3 to 3.2 MB. It is never the limiting factor for a size class; on files
  recorded in mRAW it is even **larger than what the RAW decodes to** (5184×3456
  against 3888×2592).
* **Extracting it costs almost nothing, decoding it costs the rest.**
  `leyline_raw::thumbnail` returns the bytes in **41 ms** on average; the JPEG
  decode of those 17.9 Mpx takes **76**; the reduction to 256 px and the PNG
  write, **5**. Total **122 ms**, against **680** today.
* **The whole ladder is not worth its price.** Producing the four classes (256,
  1024, 2048, 4096) from the same JPEG decode costs 276 ms and **16.9 MB of cache
  per photo** — 250 GB on a library of 15,000 images. A single class costs 53 to
  98 kB.
* **Orientation is a trap already defused.** LibRaw applies no rotation to the
  embedded preview, unlike `decode`; and that preview *sometimes* carries its own
  EXIF orientation tag, sometimes not. `scan.rs::embedded_preview` already handles
  both cases — it is that function that is used, not a second writing of the same
  reasoning.

## Decision

### 1. The thumbnail comes from the file, whoever asks for it

The decision bears on **the preview path**, not on the import pass. It is
`preview()` itself that, for the thumbnail class, uses what the file already
carries instead of developing a revision:

* a RAW or a DNG gives its embedded preview;
* a JPEG, PNG or TIFF gives itself, decoded then reduced;
* a file whose preview is missing, unreadable, or **smaller than the requested
  class**, falls back on today's render. A blurry enlarged thumbnail would be
  worse than a slow thumbnail; `scaled_to_fit` never enlarges, and this case must
  stay a render rather than a degraded image.

Putting it there rather than in `generate_import_thumbnails` is the whole point
of the decision, and it is a correction: this ADR's first draft put it in the
import pass, which would have left the **lazy path** — the one that fills the
grid as it is scrolled — paying 680 ms per visible cell. A screen of a hundred
thumbnails would have taken over a minute, and the import pass would have become
the only way to have a usable grid: exactly the opposite of the goal.

As today, producing a thumbnail is **best-effort**: one that cannot be produced
never fails an import.

**A single size class**, the thumbnail. The others stay developed on demand — a
loupe preview is a real render, and that is what one wants to see there. The
grievance is import time, not the number of available sizes, and the full ladder
would cost 250 GB on the corpus.

### 2. The catalog says where the pixels come from

A body's embedded preview **is not a revision's render**. Passing it off as one
would be a lie everything else would believe: `valid_preview` (`catalog.md` §20)
answers "here is the head revision's preview", and a preview that never went
through the pipeline would settle there forever — no render would ever come to
replace it, and an `undo` returning to that revision would bring back the body's
JPEG believing it was showing a development.

`previews` therefore gains a column (migration 6):

```sql
origin INTEGER NOT NULL DEFAULT 0   -- 0: rendered by the pipeline
                                    -- 1: the preview the file carried
```

It is carried by the row, not deduced from a path convention, for the usual
reason: a convention can be read back two ways.

* `valid_preview` gains `AND origin = 0`. The question it asks — "does the head
  have its render?" — keeps exactly its previous answer.
* An embedded preview is anchored to the **initial revision**, the only one that
  exists at import; the `UNIQUE(asset_id, revision_id, kind)` key and the foreign
  key are satisfied without invention.
* [ADR 0075](0075-preview-cache-retention.md)'s retention window does not see it:
  it belongs to the **file**, not to a revision, so it does not age with the
  history and is not evicted with it. It dies with the asset, by the cascade.
  `retain_previews` and `remove_revision_previews` exclude `origin = 1`.

### 3. What a client displays, and what it knows about it

`Library::cached_preview` now answers "what can we show of the current
version?" and not "what is the head's render?". The nuance is the whole
decision, and it was settled while writing the code, against a first draft of
this section:

**The embedded preview is not a stopgap.** It *is* the thumbnail as long as the
photo has not been developed, and the pipeline takes over the moment there is a
retouch to show — that is, exactly when the user expects to see their thumbnail
change. The version first considered, where every cell looked at triggered a
render that replaced the embedded preview, cost 680 ms of CPU per photo
scrolled past, for life, and made the colour of every thumbnail change under the
user's eyes while scrolling.

A happy consequence: **Studio does not change by a line**. A client needs no
notion of provenance — a cell has an image or it does not, as before. §2's
`origin` column stays indispensable, but it serves the catalog in not lying, not
the client in knowing what to do.

Technically, the distinction reads off the version head: a preview is only
written on the initial revision, so as soon as there is a development the head
has moved and no embedded preview is there. Nothing to compare, nothing to
explain to the client.

A cell that does not yet have its image **is not empty**: it already carries its
filename, its rating, its label and its badges — including
[ADR 0079](0079-raw-jpeg-pairing.md) §6's `RAW+J`, drawn whether there is a
thumbnail or not. Only the image rectangle is missing, and filling it with a
neutral flat is a Studio detail, not an engine decision.

The consequence is the one wanted: **we pay to render what we look at**, not
what we import. Scrolling through a folder of a hundred photos renders a hundred
thumbnails; importing fifteen thousand renders none.

### 4. Import hands back control as soon as the catalog is written

Filling the catalog and filling the cache are two jobs, and only the first is
the import. `Library::import` therefore no longer blocks on thumbnails: it
returns its report when the assets exist — **~20 ms per file** — and the pass
leaves as a background job (`engine-api.md` §3.1), emitting its `PreviewReady`
events like any render.

And that pass is **parallel**. `generate_import_thumbnails`'s comment gave an
exact reason not to parallelize it: `preview()` serializes on the decode cache
and the stage cache, two mutexes held for the whole render — the catalog lock,
for its part, is already released in the meantime
([ADR 0023](0023-catalog-lock-narrowing-preview.md)). A thumbnail drawn from the
embedded preview touches **neither one**: no sensor decode, no stage. The
objection falls with its cause.

Measured on 16 CR2s, warm page cache, 16 cores: **128 ms per file serially,
24 ms with `rayon` — ×5.3**. The factor is not the core count, file reading and
the memory bandwidth of the 17.9 Mpx decodes doing their part.

`ImportOptions` gains `thumbnails: bool` all the same, `true` by default: it is
exactly the flag `ScanOptions` already carries, for the reason
[ADR 0065](0065-selective-import.md) §2 states — *"`thumbnails: false` exists for
the caller that displays nothing (the CLI)"*. It is not a preference in the
sense of [ADR 0078](0078-preferences-panel.md) §1: it bears on a given import,
not on the installation.

**The pass follows the grid's order, not the import's.** Its first second must
go to the photos one will see first, and the import's order has no reason to be
that one — importing a card of old photos files them at the bottom of a grid
sorted by capture date. Asking the catalog for the default query's first rows
costs **0.46 ms** since [ADR 0081](0081-grid-page-cost.md): the ordering is free,
so taking it is compulsory.

That said, **the first page is already not the problem**, and that is what allows
the pass to be a plain background job. The client warms it up by itself, and
better than the engine could: `AssetsAdded` triggers `reload`, which calls
`load_window`, which partitions the missing thumbnails **visible rows first** and
fills the queue that `dispatch_thumbnails` drains three at a time. Studio knows
its filter, its sort and its folder; the engine does not. A screen of a hundred
cells thus fills in ~4 s at 122 ms per thumbnail and three jobs in flight,
against ~23 s today.

One consequence to know without settling it here: `MAX_PREVIEW_JOBS = 3` was
sized against a path that held the decode-cache and stage-cache mutexes. §1
frees them, and that bound deserves revisiting — it is a Studio setting,
measurable once the rest is in place.

What stays true in every case, and is the real safety net: **none of this is
necessary for the grid to be usable**. §1 holds for the timer as for the pass. A
cancelled warm-up, a library imported with `thumbnails: false`, a cache erased by
hand — in all three cases the grid fills by being scrolled, at 122 ms per cell
and three jobs in flight.

### 5. A companion has no thumbnail to produce

A body set to RAW+JPEG writes two files, and the import records two assets.
[ADR 0079](0079-raw-jpeg-pairing.md) §5 takes the companion out of the grid with
a clause, and the details panel shows only **its name** of it (§6) — no Studio
screen displays a companion's thumbnail. `generate_import_thumbnails` produces
one anyway, for every imported file: on a folder set that way, **half the pass is
thrown away**.

The pass therefore skips assets whose `companion_of` is not null. It is a
decision **neutral by construction**: a library with no pairs skips nothing, and
§1 carries it entirely. Nobody loses, a frequent case gains a factor of two.

Two consequences follow, both covered by §3's lazy path:

* **Unpairing gives the JPEG back to the grid**
  ([ADR 0079](0079-raw-jpeg-pairing.md) §6). It then has no thumbnail, and the
  timer's queue produces it as for any visible cell that has none.
* **The explicit pairing pass** on an existing library
  ([ADR 0079](0079-raw-jpeg-pairing.md) §7) leaves already-produced thumbnails in
  place. They become useless without becoming wrong; erasing them would make an
  unpairing slow to recover a few tens of kilobytes.

And **the companion is not a better source either** for the master's thumbnail,
as one might have believed — it is on disk, full size, already in JPEG. Measured
on six real pairs, warm page cache:

| Source of the RAW's thumbnail | Cost per shot |
|---|---|
| the preview embedded in the CR2 | **119 ms** |
| the companion JPEG file | 161 ms |

The companion is a **higher-quality** encoding than the embedded preview — 5.2 to
10.6 MB against 1.3 to 3.2 — hence longer to decode (111 to 159 ms against 69 to
93), for the same 5184×3456 image. Reading it costs less than opening the RAW,
and that does not make up the difference. §1 therefore applies to the master
without exception, and the pair changes only one thing: the companion costs
nothing at all.

### 6. What the user sees, and what must be said

A body's thumbnail is not a neutral Leyline render: it carries the contrast, the
saturation and the balance the manufacturer applies. On a never-developed photo,
the grid will therefore show the Canon rendering **and the loupe the neutral
Leyline rendering** — the two differ, and that is the price.

It is owned, and it is the one Lightroom charges under the name "Embedded &
Sidecar". Refusing it would cost 2 h 50 on fifteen thousand files, the vast
majority of which will never be looked at closely.

What we do **not** pay, on the other hand, is flicker: the thumbnail changes
only at the first retouch, never as one's gaze passes. §3 explains why that
version was rejected, and what it would have cost.

## Consequences

Per file, on the seven CR2s measured:

| | today | after | ratio |
|---|---|---|---|
| import thumbnail | ~680 ms | **122 ms** | ×5.6 |
| full import (one photo) | ~700 ms | **~142 ms** | ×4.9 |
| full import, `thumbnails: false` | ~700 ms | **~20 ms** | ×35 |
| cache written per photo | 53–98 kB | 53–98 kB | unchanged |

On the corpus's 15,000 CR2s, the pass goes from **2 h 50 to 31 min** serially
and to **6 min** in parallel (×5.3 measured). But it is the line below that
counts most, since §4 takes it off the import path:

| 15,000 CR2 | today | after |
|---|---|---|
| before the catalog is usable | 2 h 50 | **~5 min** |
| before the grid is entirely warm | 2 h 50 | ~6 min more, in the background |
| to scroll a never-warmed grid | — | 122 ms per visible cell |

For a body set to RAW+JPEG, §5 adds to this. Per-file costs measured today —
700 ms for a CR2, **248 ms for a JPEG** (no sensor decode) — carried over to a
real folder of the corpus, `2022_04_17`, which holds 29 pairs:

| 29 pairs (58 files) | today | after |
|---|---|---|
| files handled by the pass | 58 | **29** |
| import duration | ~27.5 s | **~4.6 s** |

The overall factor there is **~6**, of which a factor of two comes from §5
alone.

The JPEG decode (76 ms) becomes the pass's dominant item, at 62 % of its time.
It is a full-resolution decode — 17.9 Mpx — to produce 256 px.

## Alternatives rejected

* **Parallelizing the pass as it stands**, without changing the source of the
  pixels. That is what the code's comment suggested, and it does not attack the
  right term: sixteen cores on a sensor decode is still some twenty minutes spent
  demosaicing to produce 256 px images. And it is precisely in that order that
  parallelization was blocked — by the decode-cache and stage-cache mutexes.
  Changing the source first (§1) removes the obstacle and makes the gain worth
  having: §4 therefore parallelizes, but only because §1 precedes it.
* **Producing every thumbnail before handing back control.** That is today's
  behaviour, and §4 refuses it: filling the catalog and filling the cache are two
  jobs, and making the first wait on the second has never served anyone. The grid
  stays usable without a warm-up — it is §1 that guarantees that, at 122 ms per
  visible cell, and not the pass.
* **Producing the four classes from the same JPEG decode.** Tempting (a single
  decode serves everything), measured, and rejected on the figure: 250 GB of cache
  on the corpus, for sizes nobody asked for.
* **Storing the preview under a fictitious revision, or by a path convention.**
  Both avoid §2's column and both make the catalog say something false.
  `previews`'s foreign key refuses the first; the second encodes a fact in a
  filename, where it is deduced instead of read —
  [ADR 0047](0047-xmp-sidecar-read.md) showed what a naming convention one does
  not solely control costs.
* **Taking the master's thumbnail from its companion JPEG**, when there is one —
  it is already on disk, full size, and already in JPEG. Measured on six real
  pairs: **161 ms against 119**. The companion is a higher-quality encoding than
  the embedded preview (5.2 to 10.6 MB against 1.3 to 3.2), hence longer to decode
  for exactly the same image. The idea would additionally cost a code path serving
  pairs alone.
* **Replacing the embedded preview with a render as soon as a cell is looked
  at.** The grid would always end up telling the truth about what the settings
  produce, and grid and loupe would agree. The price is twofold and it is
  prohibitive: 680 ms of CPU per photo scrolled past, for life, and the colour of
  every thumbnail changing once under the user's eyes while scrolling. §3 keeps
  the other branch — the embedded preview holds until the first retouch — because
  a catalog of undeveloped photos has nothing more to say than what the body
  rendered.
* **Keeping the embedded preview even after a retouch.** There, the grid really
  would lie: it would show something other than what the settings produce, and
  going back from the loupe to the grid would give two contradictory images of the
  same photo.

## What this ADR does not do

No pipeline pixel moves: no stage, no stage version, no revision.
`pipeline.md` §5.1 is not implicated — an embedded preview is not a render, and
the whole point of §2 is to write that in the catalog rather than leave it to be
guessed.

Nor does it touch the **full-resolution JPEG decode** that becomes the dominant
item. A scaled decode (JPEG's DCT allows 1/2, 1/4, 1/8 — 648×432 would be more
than enough for 256 px) would reduce it much further, and `zune-jpeg`, the
decoder the `image` crate embeds here, does not expose it. That would require one
more decoder in the tree: a dependency decision, unmeasured to date, and one that
does not have to be taken in the same move.
