# ADR 0107 — Derived assets: a socket for the extensions that make pixels

**Status:** Accepted — 2026-09

## Context

[ADR 0102](0102-paid-extensions-and-the-pixel-boundary.md) settled the
*shape* of an extension that has to produce pixels: it produces a **new
asset**, never a stage. It deliberately settled nothing else, and said so
— "no engine work is required by this ADR. A pixel extension imports its
output like any other file."

That sentence is true and insufficient, for the reason
[ADR 0073](0073-external-mask-detectors.md) already gave about masks:

> Nothing is therefore missing in the engine. What is missing is **the
> socket**: by what gesture a Studio user obtains a sky mask without going
> through an export, a third-party tool and a manual import.

Word for word the same hole, one boundary further along. Today a user who
wants an AI denoise exports a TIFF, finds the file, runs a program, finds
its output, imports it, and then re-develops from zero because the
imported file is a finished picture and nothing of the development
survived. Every one of those six steps is the thing this project builds
software to remove.

So the question this ADR answers is the one the parity survey left open:
**is there a socket for a pixel processor, as ADR 0073 built one for
detectors, or does a plain import suffice?** Decided 2026-09-01: a socket,
and the reasons are ADR 0073's own — the gesture is worth building, anyone
can write a processor in twenty lines against a two-file protocol, and the
socket has a value for the free project that does not depend on any paid
product ever existing.

## Decision

### 1. A crate apart, and nothing of the extension in the engine

The socket lives in a **new open crate**, `leyline-derive`, and follows
`leyline-detect` line for line:

```
Studio ─→ SDK ─→ leyline-derive ──(process)──→ a processor, whatever it is
```

That crate processes nothing. It holds **the contract, the discovery and
the invocation**: `leyline-core` is the only Leyline crate beneath it, and
`leyline-sdk` re-exports it, because
[`architecture.md`](../architecture.md) §Inside Studio requires Studio to
declare a single Leyline dependency and that rule does not bend for an
accessory.

The engine does the two things a separate process cannot: it renders the
image that is handed over, and it imports what comes back. Neither is an
extension surface — the engine still has none.

### 2. A processor is an executable that turns an image into an image

```
processor --image <in.tif> --operation <id> --out <out.tif>
```

* **input**: a 16-bit RGB TIFF, full resolution, in the exchange space of
  §3;
* **output**: a 16-bit RGB TIFF, **the same dimensions**, in the same
  space. Different dimensions are a refusal, not a resize: a processor
  that returns another geometry has broken the contract that lets the
  result inherit a development. Fewer than sixteen bits is a refusal too,
  and for a subtler reason: an image library widens an 8-bit file without
  a word, so the answer would look right and hold 255 of every 256 levels
  less than it should — a silent loss in the one place this socket exists
  to avoid one;
* **the rest**: exit code `0`, and `stderr` to say why when it is not
  `0` — passed through verbatim to the user, exactly as ADR 0073 §2
  decided for detectors.

The four consequences ADR 0073 §2 drew hold here unchanged and are the
justification: the processor touches neither catalog nor library, it need
not link `leyline-sdk` (two processes exchanging two TIFF files are not a
combined work, and the licence boundary is crossed by an `execve`),
anyone can write one, and one that crashes does not take Studio with it.

The timeout is **ten minutes**, not the detector's two: a neural denoise
of a 30 Mpx frame on a CPU is minutes of honest work, where a
segmentation on a 1024 px preview is seconds. It is still a safety net
against a wedged process, not a performance target.

### 3. The exchange space: linear, and the price of sixteen bits

The image handed over is **linear Rec. 2020 — the working space
([ADR 0044](0044-linear-wide-gamut-working-space.md)) — with white at 1.0 mapped to
65535**, floored at 0 and clipped at 1.

Linear because a denoiser that is handed a finished JPEG is denoising a
tone curve's output, and because everything downstream of the cut still
has to run on light. Sixteen bits because it is a container every image
library on earth reads, and because the alternative — 32-bit float TIFF —
buys headroom from a much smaller set of tools.

**The price, stated rather than hidden**: a value above white does not
survive the round trip. After the cut of §4 those values exist — a
highlight reconstruction ([ADR 0050](0050-highlight-reconstruction.md))
puts them there on purpose — and a derived asset has lost them. It is the
same trade every 16-bit interchange makes, and it is a real reason to
prefer the original file for a photograph whose highlights matter more
than its noise.

### 4. The cut: everything before rank 20, and nothing after

The processor receives the buffer as it stands **before rank 20** — that
is, `input` (rank 0) and `camera_profile` (rank 10) applied, and nothing
else.

That position is not a taste, it is the first place in the pipeline where
the buffer has a **single meaning**. Before rank 10 it does not: `input`
leaves the samples camera-native when a DCP is present and rotates them
into Rec. 2020 when there is none (`stages/input/v4.rs`), so a file
handed over at rank 0 would be in one of two spaces depending on a
setting — unusable as an interchange format, and a trap for the first
processor author.

What that buys, and it is the whole point of the design:

* **the development survives**. White balance (rank 50), exposure, tone,
  HSL, local adjustments, sharpening, crop — everything the user had set
  is still a *setting* on the derived asset, still live, still adjustable.
  The derived file replaces the **decode**, not the development;
* **it is what a linear DNG is**, which is what Lightroom's Denoise
  writes, for the same reason and without this project having to write
  DNG ([ADR 0101](0101-refused-modules.md) refuses that);
* **it is testable**. A processor that copies its input to its output must
  produce a derived asset that renders like its parent — bit for bit on
  the test image, and never further than the one 8-bit level the
  exchange's sixteen bits leave as slack (§3). That invariant is a test in
  this repository, and it is the sharpest statement of what "replaces the
  decode" means.

What is paid: the **camera profile is baked**. A derived asset cannot be
re-profiled, because the file it holds is already past rank 10. That is
the cost of handing a third-party program a file in a space it can name.

### 5. What the derived asset is, in the catalog

An ordinary asset, imported by the ordinary path — with three facts made
explicit rather than left to chance.

**Lineage.** `assets.derived_from` (migration 11) points at the parent,
`ON DELETE SET NULL`, indexed like every other cascade
([`catalog.md`](../catalog.md) §Cascading foreign keys). It is
*informational*: unlike `companion_of`
([ADR 0079](0079-raw-jpeg-pairing.md)), it does **not** hide the row from
the grid. A denoised frame is a photograph the user will look at and
choose between, not a sidecar of another one.

**Metadata.** Inherited from the parent, whole. The derived file is the
same shot — same body, same lens, same aperture, same second — and
re-reading EXIF from a TIFF this program wrote would produce a worse
answer to a question already answered. It is the one place where
[`catalog.md`](../catalog.md) §"An asset is purely factual" is satisfied
by copying rather than by reading, and the reason is that `derived_from`
makes the identity of the shot a recorded fact.

*Not* inherited: rating, flags, keywords, collections. The derived
photograph is judged on its own.

**The initial revision** is the parent's current revision, with exactly
three changes:

* `input` is pinned at **v5** with `source_encoding: "linear_workspace"`
  (§6) — the file is already in the working space;
* `camera_profile` is dropped: it is baked into the pixels, and leaving
  it would apply it twice;
* the profiled noise stages (`noise_luminance`, `noise_color`,
  [ADR 0072](0072-measured-noise-profile.md)) are set neutral. Two
  reasons, and the second is the load-bearing one: running the wavelet
  denoiser on top of a neural one is denoising twice, and those stages
  read `SourceColor` — a measured profile is keyed to sensor samples the
  derived file no longer holds. Dropping them is what keeps the §4
  invariant exact instead of approximate.

Everything else is copied verbatim.

### 6. `input::v5`, and the capability rule again

Two things change in the decoder's configuration, so they are pinned by a
stage version, as every change to that configuration has been since
[ADR 0044](0044-linear-wide-gamut-working-space.md):

1. **A non-RAW source is decoded at its native bit depth.** Until v4,
   `source::decode_native` normalised every JPEG, PNG and TIFF to 8 bits
   (`into_rgb8`), which would have crushed a 16-bit derived file back to
   256 levels and made the whole exchange pointless. It also, quietly,
   crushed every 16-bit TIFF anyone had ever imported. Revisions pinned
   at v1–v4 keep the truncation, because that is what they were rendered
   with;
2. **`input.source_encoding`** — `"srgb"` (the default, and what v1–v4
   always assumed) or `"linear_workspace"`, which skips both the transfer
   decode and the primaries rotation because the samples are already
   where the pipeline wants them.

`source_encoding: "linear_workspace"` on a revision pinning `input` below
v5 is **refused by `validate()`**, by name — the capability rule of
[ADR 0048](0048-range-masks.md) §5, applied for the fourth
time. A setting a pinned version cannot express is never silently
ignored.

`"linear_workspace"` on a camera-native source is refused the same way:
the sensor's samples are not in the working space, and a revision saying
they are would render wrong rather than error.

### 7. Discovery: one manifest per processor

Identical to ADR 0073 §3, in the folder beside it —
`<user config>/Leyline/processors/<id>.json`:

```json
{
  "id": "leyline-assist",
  "label": "Leyline Assist",
  "command": "/opt/leyline-assist/leyline-assist",
  "args": ["derive"],
  "operations": [
    { "id": "denoise", "label": "AI denoise" }
  ]
}
```

The list is named `operations`, and **the code reads that name** — ADR
0105 §4 is the reason the sentence is here: the detector manifest's list
was documented under one name and read under another for four weeks, and
the first real detector was silently ignored. A manifest that is
unreadable, incomplete, or whose command does not exist is ignored, and
`leyline processors` says which one was ignored and why.

A processor is an accessory: no manifest, no menu entry. Studio launches
identically with none, which is every installation today.

### 8. Conformance, because ADR 0105 paid for that lesson

`leyline derive-check --from <dir>` runs a processor against a generated
image and verifies the **protocol**, never the quality of a denoise: it
ran, the output is a readable 16-bit RGB TIFF, the dimensions match the
input, and the result is not bit-identical to the input — a processor
whose model failed to load and fell back to a copy looks exactly like a
working one otherwise.

Same posture as `detect-check`: it says what it checked on every pass, so
that a pass means something.

### 9. What this does not settle

* **The licence.** Whether and how a pixel extension is sold is *not
  decided* and is not decided here. ADR 0102 §5 fixed the mechanism a
  licence would use if there is one — offline, minisign, verified inside
  the extension — and everything above works identically for a free
  processor, a paid one, and one somebody writes this afternoon in
  Python. Nothing in this repository knows the difference, which is ADR
  0069 §2 held to.
* **Which processor.** Choosing a denoising model, converting it, and
  measuring it belongs to the processor's own repository, exactly as
  ADR 0073 §5 decided for detectors' weights.
* **A float exchange.** §3 states what 16 bits costs; if a processor ever
  needs the headroom, that is another `--format` and another ADR.
* **Chaining processors.** A derived asset can be derived again — nothing
  forbids it, `derived_from` records the chain — but no interface
  encourages it and no invariant is claimed beyond one hop.

## Consequences

* **One more crate, open and small**: `leyline-derive`,
  [`architecture.md`](../architecture.md) lists it beside
  `leyline-detect`.
* **One stage version**, `input::v5`, and therefore one line in
  [`pipeline.md`](../pipeline.md) §3.3 and a golden run that must show
  every existing case unchanged: no revision anywhere pins v5 until a
  derivation writes one.
* **One migration**, 11, adding `derived_from` and its index.
* **A 16-bit TIFF import stops being truncated** — for anyone, not only
  for derived files — but only in revisions written from now on, since
  the behaviour is pinned by `input`. A photograph imported last year
  renders today exactly as it did.
* **A derivation is expensive in memory, and deliberately not streamed.**
  The full-resolution buffer, its 16-bit encoding and the answer are all
  live at once — of the order of a gigabyte on a 30 Mpx frame, the same
  order as one export in flight ([ADR 0068](0068-concurrent-export-batch.md)).
  One photograph at a time is the design: a batch of these is a job, not a
  menu entry.
* **`pipeline.md` §5.1 does not move.** No new render path, no stage
  whose result depends on software that may be absent: a derived asset is
  a file in the library, and a library that has never seen a processor
  opens, renders and exports it identically.

## Rejected

* **No socket at all — the extension imports its own output.** It is what
  ADR 0102's consequences implied, and ADR 0073 §Alternatives already
  refused the same thing for detectors: two processes on one `catalog.db`
  contend for a SQLite lock this project spent two ADRs narrowing
  ([0023](0023-catalog-lock-narrowing-preview.md),
  [0024](0024-catalog-lock-narrowing-export.md)), and the extension would
  have to learn the data model to write a revision.
* **A documented convention and a CLI import verb, with no crate.** Half
  the cost, and it leaves the gesture in Studio unbuilt — which is the
  entire defect being fixed.
* **Handing over the finished render** (what export produces, 16-bit
  sRGB). Simpler by a wide margin, and it throws the development away:
  the derived asset would arrive with neutral settings and the user would
  redo every slider. It also denoises a picture that has already had a
  tone curve applied to it.
* **Handing over the buffer at rank 0.** §4: two possible spaces
  depending on whether a DCP is set, which no protocol document can
  paper over.
* **Writing a DNG** — what Adobe does, and what ADR 0101 refuses for good
  reasons that have not changed: a DNG that is *almost* right is worse
  than no DNG.
* **A `derive` stage that calls the processor at render time.** ADR 0102
  §2, and it is worth restating in the ADR that builds the socket: a
  revision citing it could not be rendered without the extension, which
  makes a purchase the condition of seeing a photograph already
  developed.
