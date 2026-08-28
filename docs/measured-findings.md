# Measured findings, and what they changed

**Document:** `docs/measured-findings.md`
**Version:** 0.2
**Status:** A log of measurements and their follow-ups (a planning input, not a decision)

---

## The state of this document

> This document is a **log of measurements**, opened on **2026-08-02**, and of the
> decisions each one triggered. It settles nothing by itself: every item below
> required **its own ADR before a single line of code**, as the project's rule
> demands ("no code before architecture").
>
> It replaces neither [`specification.md`](specification.md) — which says what is
> delivered and what is excluded — nor [`roadmap.md`](roadmap.md), which says where
> the project stands. It feeds both. What it keeps that they do not: the raw
> numbers, the protocol that produced them, and the findings that turned out to
> be wrong.

---

# 1. Subject

The V1 scope and the V2 scoping are closed: what remains is no longer
missing functionality. The question therefore becomes **where the product is
weakest when nothing is missing**, which is not the same thing. Three axes stood
out, and only one was a feature gap.

| Axis | Nature of the problem | What the user feels |
|---|---|---|
| **A — Render accuracy** | An incomplete colour chain | The image "comes out" worse on opening, before any adjustment |
| **B — Performance** | Structurally redundant work, no GPU | Every slider drags |
| **C — Optional local AI** | A deliberate absence | High ISO and masking stay manual |

Axis A decides the verdict at first glance; axis B is paid for every
second of use; axis C is a horizon, not current work.

---

# 2. Axis A — Render accuracy

This is the most profitable axis: three precise gaps, all already identified in
the repository, none of which requires invention.

## A1 — Validate the DCP colorimetry, then apply the tables

**State.** The DCP path is delivered but **flagged experimental**
([ADR 0035](adr/0035-camera-profile-dcp.md)): its accuracy has never been
confronted with real Adobe `.dcp` files and their reference renders, and the
`ProfileHueSatMapData`, `ProfileLookTableData` and `ProfileToneCurve` tables are
not applied.

**Why it matters.** Those tables *are* the Adobe rendering. The matrix alone makes
a correct conversion; it is the `LookTable` that makes a file "look as though it
came out of its vendor's own converter". As long as they are missing, the A/B comparison is lost
in advance, whatever the quality of the rest of the pipeline.

**The split.** Two distinct stages, not to be confused:

1. **Validate what exists** — a comparison protocol against Adobe `.dcp` files and
   reference renders, a measurement of the discrepancy, then **lifting or confirming**
   the "experimental" label. Changes no pixel if there is no bug.
   **Done on 2026-08-02/03** — see the result below.
2. **Apply the missing tables** — this changes the render, and therefore requires a **new
   stage version** (`pipeline.md` §5.1), never a modification of
   the published stage. **Delivered on 2026-08-02**:
   [ADR 0062](adr/0062-dcp-illuminant-interpolation.md) for illuminant
   interpolation (`camera_profile::v2`) and
   [ADR 0063](adr/0063-dcp-tables.md) for `HueSatMap`, `LookTable` and
   `ProfileToneCurve` (`camera_profile::v3`).

**Prerequisite — and the blockage found on 2026-08-02.** `.dcp` files and
reference renders are needed for the available camera bodies (the ~17,000 real Canon 60D
CR2 files are the natural sample). **No `.dcp` exists on the development
machine**, and nothing installed on it supplies one.
A1.1 is therefore **blocked on an artefact to be brought in**, not on code.

Three ways to unblock it, in order of preference:

1. **Install a free RAW developer that distributes `.dcp` profiles** and take
   them — the simplest source, and it also gives a second implementation to
   compare against.
2. **Adobe's DNG Converter**, free, which installs the complete collection
   of per-camera profiles. DCP is an Adobe format; there is no avoiding the
   name here.
3. **A photographed ColorChecker chart**: the most honest validation, and
   the only one that depends on no other software — the rendered patches are compared
   with the chart's reference sRGB values. It needs no `.dcp` at all,
   but it does need a shot.

**A lead brought in on 2026-08-02:** collections of **linear profiles**
(`.dcp`) per brand circulate as downloads (for instance
`olivier-rocq.com/lightroom/profil-lineaire/`), and **Adobe DNG Profile Editor**,
free, makes them. A *linear* profile is a particularly well-chosen validation
case: having neither a `ProfileToneCurve` nor a look table, it
exercises the matrix path alone — exactly what Leyline implements, and nothing
of what it does not implement yet.

What that lead unblocks, and what it does not:

* ✅ **Done on 2026-08-02** — and the first real file found a
  blocking bug: Leyline could read **no** authentic `.dcp` at all. See below.
* ✅ **Done**: a neutral grey from the sensor comes out neutral through
  camera→XYZ(D50)→sRGB, on both real profiles. A permanent test, enabled by
  `LEYLINE_TEST_DCP`.
* ❌ **The comparison with the profiles' own vendor rendering.** It still requires
  that vendor's software to produce the reference. A downloaded profile does not
  replace it.

**The bug found.** An authentic `.dcp` is a *bare IFD* — a directory of
tags, with no image at all — carrying the version number `0x4352` where TIFF puts
42. The reader relied on `tiff::Decoder`, which requires both: the 42 and an
`ImageWidth`. The only existing fixtures being TIFF images with DCP tags
grafted on, they all passed while no real profile was
readable. That is the exact price of a test suite that talks only to
itself.

Note too: those profiles are the work of third parties, not the vendor's factory
profiles. They validate our reading and our algebra, not our fidelity to any
particular reference rendering.

**Result of 2026-08-03.** The reference arrived, in the form of an **independent
implementation's** render of the same RAW with the same profile and neutral
processing settings.
Once the level was normalised: **a median discrepancy of 0.0027 out of 1.0**, 90th percentile
0.0060, channel ratios within 0.007. The colorimetry agrees with an
independent implementation.

**The protocol is now reproducible without manual intervention**:
`rawtherapee-cli -s` without a sidecar renders with neutral values, and a minimal
`.pp3` fixes the input profile and the output space. The reference produced
that way reproduces a hand-made export to within 0.001.

### The level gap: a defect found, the gap not closed

There remained a **gain of ×1.185 in linear**: the reference implementation
renders brighter than Leyline, uniformly. Three things had been established —
independent of the camera profile, hence not of colour; a gain and not a curve;
and the sensor's white level as the lead.

**The lead produced a real defect — and not the explanation of the gap.** The
two results are distinct and must be read separately. An earlier revision of
this section concluded that the gap *was* explained; that conclusion did not
survive the fix, and is recorded here as wrong rather than deleted.

**The defect, fixed ([ADR 0066](adr/0066-sensor-white-level.md)).** Leyline was not
dividing by 16,383 as was believed: `adjust_maximum_thr`, a LibRaw setting
left at its default of 0.75, lowers the white level down to
**the brightest sample of the image at hand**. On four files
from one series, Canon 60D, ISO 100, the same exposure, the level chosen was
13,794 for one and 16,383 for the other three — a 19 % difference in brightness
depending on whether a reflection happened to fall in the frame. That is exactly what
`auto_brighten: false` was supposed to forbid.

The camera writes the answer into the file, and nobody was reading it. What the
two engines take for "white", on a Canon 60D:

| Source | ISO 100/125 | ISO 200…3200 | ISO 160/320/640/1250/2500 |
|---|---|---|---|
| LibRaw `maximum` — what Leyline divided by | 16,383 | 16,383 | 16,383 |
| LibRaw `linear_max` — camera metadata, **ignored** | 12,279 | 15,094 | 11,222 |
| The reference implementation's measured table | 13,480 | 15,200 | 12,550 |

The last two rows **share the same split into three ISO groups**: that is not a
coincidence, it is the same hardware behaviour seen from two sides. Leyline, for
its part, normalised by the theoretical 14-bit ceiling, the same for every file.
`input::v4` now reads the metadata — and a survey of 250 files from the corpus
showed along the way that it also follows **the aperture**, to within ~1 % of the
per-aperture scaling the reference implementation maintains by hand.

**What it cost, concretely.** A neutral render was 8 to 19 % too dark depending
on the sensitivity, and above all **a pixel saturated at the sensor did not come
out white**: at ISO 100 it arrived at 0.82. That is exactly the "the image comes
out worse on opening" that opens this document.

**The gap itself is not closed.** After the fix it goes from ×1.16 to ×1.03 on
the ISO 100 file, but **increases** from ×1.08 to ×1.13 on an ISO 400 file —
there where `v3` stretched white up to the brightest pixel of an image that had
no very bright one. The effective divisors of both engines are now known on both
sides, and **they do not explain** the factor of ~1.14 that remains. It is
therefore not the white level, and the question stays open.

**A third opinion is not available as things stand**: the other free engine's
default rendering applies a *scene-referred* tone mapping, whose S signature is
clear — a ratio of 1.43 in the midtones, 0.96 at white. Comparing it would
require disabling that module.

The "experimental" label stays: for the absence of a comparison with the
profile vendor's own rendering, and for that ~1.14 factor that is still
unattributed. What is settled is that **it is not colour** — the colorimetry
does agree.

**Risk.** Low on point 1, medium on point 2: interpolating the
`HueSatMap` tables is precision work, where a mistake goes unnoticed
on a test image and leaps off the screen on skin.

## A2 — Expose the choice of demosaic algorithm — **delivered on 2026-08-02**

**Initial state.** `params.user_qual` was neither exposed nor chosen: we took
LibRaw's default. [ADR 0050](adr/0050-highlight-reconstruction.md) §143 explicitly
leaves the question open.

**Why it matters.** Other developers offer AMaZE, LMMSE, DCB; the choice shows
on fine detail and repetitive patterns (moiré, foliage, fabric). It is a
quality lever **already present in the dependency**, needing only to be driven.

**Constraint.** Demosaicing is upstream of the whole pipeline: exposing it
changes the render, so it is a new version of the `input` stage, and the
value chosen must be written into the revision. A default that changed without a
stage version would break `pipeline.md` §5.1.

**Risk.** Low. The work is wiring and validation, not
algorithmics.

**Delivered** by [ADR 0061](adr/0061-demosaic-algorithm.md): four named
values (`ahd` by default, `vng`, `dcb`, `dht`), written into the revision,
carried by `input::v3`. AMaZE and LMMSE are absent for want of being present in
the linked library — offering them would have been offering a choice that falls back
silently on AHD.

## A3 — A noise profile measured per camera and per sensitivity — **delivered on 2026-08-04**

**Initial state.** Already listed as "decided, not implemented"
([ADR 0046](adr/0046-edge-preserving-denoise.md) §7), to be settled by its own
ADR.

**Why it mattered.** Denoising worked knowing nothing about the
sensor: three constants, the same for every file in the world. Between
ISO 100 and ISO 12800, the actual standard deviation of a Canon 60D's noise varies by a
factor of **nine** — a single threshold can therefore only be right at one
sensitivity. And photon noise grows with the light received, which a constant
threshold also ignores: too low in the shadows, too high in the highlights.

**Delivered** by [ADR 0072](adr/0072-measured-noise-profile.md). Four decisions,
three of which this document had not anticipated:

1. **The question of the data was the right one, and its answer is a licence.**
   darktable's measured table — 434 camera bodies, 7,842 `(ISO, a, b)` triples —
   is published under GPL-3.0-or-later, hence usable as is in a
   GPL-3.0-only project. Measuring ourselves would have given two camera bodies.
2. **The table is frozen with the stage version**, failing which an update
   of the measurements would change the render of an existing revision (§5.1). Writing
   that rule brought to light that **the Lensfun database, for its part, is pinned by
   nothing** — a real hole in §5.1, recorded in the ADR, not closed.
3. **The two stages change rank** (170/180 → 5/6): a model measured on
   sensor counts means nothing after exposure, the tone
   curve and clarity. It is the first time a stage version has changed
   rank, something ADR 0042 §3 permitted without anything having exercised it yet.
4. **The threshold becomes a per-pixel threshold** — `k · 6σ_l · √(a·x + b)` — instead
   of a per-scale constant.

**What it costs: nothing.** The same machine, the same 3 Mpx image, the same sliders
(luminance 40, chroma 30): **98 ms for `v1`, 231 ms for `v2`, 219 ms for
`v3`** — the measured profile is *free*, and even slightly profitable.
The explanation is in §4 of the ADR: `v3` works in linear light and therefore
abandons the two round trips to the display axis that `v2`
paid for on every render, which amply finances the per-pixel square root.

**What it does not do.** Parity with learned denoisers stays out
of reach (ADR 0046 §7 still holds): a measured threshold does not reconstruct
detail, it only knows which detail not to destroy.

---

# 3. Axis B — Performance

## The starting point, measured

[ADR 0041](adr/0041-interactive-preview-rendering.md) quantifies the problem on a
real CR2 of **3888×2592** (10 Mpx), reference machine i9-9900K, 16 threads:

* `Small` preview, neutral: **0.97 s**
* `Small` preview, every slider active: **2.39 s**

Current cameras are at 45–60 Mpx, that is, **4 to 6×** those times.

## B1 — The stage cache of ADR 0041 §3 — **delivered on 2026-08-02**

**Initial state.** ADR 0041 decides **three** things: the proxy at display
resolution (§1), radius scaling (§2), and a **cache of intermediate
states** (§3). The first two were delivered; the third was
not — only `DecodeCache` existed, which avoids re-decoding and not
re-computing, something ADR 0041 itself ranks among the insufficient alternatives.
The roadmap nonetheless ticked phase 7.

**What it was worth.** Every render started again from the decoded buffer: moving
`sharpening` (the last stage, ~13 ms) replays dehaze, clarity, texture, HSL and the
local adjustments identically. That is **the** structural difference from
the established developers, which replay only downstream of the edited node.

**Why first.** The design was done and accepted — checkpoints
before the expensive stages, `(stage index, upstream settings fingerprint, buffer)`,
~30 MB per session, the preview path only. The cache is
**purely derived**: throwing it away at any moment changes no pixel. Hence
**no reproducibility risk, no new stage version, no new
dependency**. It is the best gain/risk ratio in this whole
document.

**Delivered.** Measured at **−78 %** on an end-of-pipeline slider (~60 ms → ~14 ms,
a 1024×683 map, `--release`). Two things the implementation learned and which
are recorded in ADR 0041: the cache lives on the `Library`, not on the edit
session — the develop view renders through `Library::preview` — and a checkpoint
threshold designates a position, not an exact rank, failing which three of the four
points are never taken. The real prerequisite was teaching each stage
which settings it reads (`Stage::reads`), something no ADR had laid down.

## B2 — The export and print path — **measured on 2026-08-03**

**State.** ADR 0041 explicitly excludes export and printing from its
optimisations: full resolution, no stage cache, **bit for bit
identical**. That was the right trade-off for an ADR centred on the interactive path —
but it left the export path without a single figure.

**Measured.** `crates/leyline-engine/benches/export.rs`, i9-9900K 16 threads
(ADR 0041's reference machine), `--release`. An export's three costs are
weighed separately, because they do not behave alike:

| Step | 10 Mpx | 45 Mpx | Parallel? |
|---|---|---|---|
| LibRaw decoding, full size | **0.85 s** | 2.76 s at 30 Mpx (measured on a 5D IV) | yes |
| Rendering, a neutral revision | 0.045 s | **0.19 s** | yes (~8.7×) |
| Rendering, a full edit | 1.12 s | **4.33 s** | yes (~8.7×) |
| WebP encoding | — | **0.61 s** | **no** |
| JPEG encoding | — | **0.87 s** | **no** |
| TIFF encoding | — | **1.62 s** | **no** |
| PNG encoding | — | **2.55 s** | **no** |
| AVIF encoding | 5.05 s | **21.8 s** | yes (~7.5×) |

Everything is **linear in pixels**: ×4.3 of area gives ×3.9 on rendering, ×4.3
on AVIF, ×3.3 on decoding. Nothing collapses as size grows, and
nothing benefits from an economy of scale either.

**What that gives end to end.** A 30 Mpx file, a full edit, JPEG:
2.8 s of decoding + 2.9 s of rendering + 0.6 s of encoding ≈ **6.3 s**. The batch of
500 files mentioned above therefore takes **~52 minutes**. In AVIF, the same batch
goes to **~3 hours**, encoding alone becoming three quarters of the time.

**Three findings, in the order in which they matter:**

1. **AVIF is out of the ordinary** — 25× the cost of JPEG at equal size. `ravif`'s
   encoding speed is fixed at `speed(6)` in the code, with nothing
   exposing or documenting it. It is the only setting in this document that could
   divide a time by three without touching a pixel of the render.
2. **The fast encoders are single-threaded** (JPEG, PNG, TIFF, WebP: user
   time ≈ real time), while the batch processes **one file at a
   time**. On 16 cores, each encoding therefore leaves 15 cores idle — on
   the order of 10 to 15 % of the time of a JPEG batch. Overlapping the encoding of file
   *n* with the rendering of *n+1* is the obvious structural gain, and it changes
   no pixel: it is scheduling, not computation.
   **Corrected on 2026-08-04 ([ADR 0068](adr/0068-concurrent-export-batch.md)):**
   that estimate aimed at the right symptom but far too small. Measured on the batch
   itself rather than file by file, the export uses only **289 % of 1,600 %** —
   thirteen cores are missing, not the ones of an encoding. Processing several photos at
   a time returns 2.81×, where overlapping two stages of one file would have
   returned only a fraction of the 10 to 15 % announced here.
3. **Rendering dominates and is already parallel** (~8.7× on 16 threads). There is
   no waste to recover there without changing the operators themselves —
   and B1's stage cache is forbidden here by the §5.1 promise.

### What the export is worth, measured against others

Absolute figures do not say whether the export is slow — only how long it
takes. The same machine, the same files, the same JPEG q90 output, RAW → file
end to end (decoding included), against **two independent CPU implementations**
of the same task, on 2026-08-03:

* on the **neutral** path, Leyline is the fastest of the three, and by far the
  most frugal — 6.8 s of CPU where the closest one consumes 17.7 for the same
  file;
* **loaded with comparable settings** (white balance, exposure, contrast,
  highlights, shadows, blacks, vibrance, denoising, sharpening, rotation,
  cropping), the gap closes to **2 %** — 5.79 s against 5.69 on a 30 Mpx file —
  still with ~18 % less CPU.

**There is therefore no performance deficit to catch up on export.** This
document assumed the opposite when it was opened, and the assumption is what
the measurement removed.

### The AVIF exception, and what it really costs

Another engine exports the same 30 Mpx to AVIF four times faster — for a file
eight times heavier. It is therefore not the same work, and a raw comparison
proves nothing here.

What does prove something is moving our own slider. The same
export, `ravif` set to three speeds:

| `with_speed` | Time | File |
|---|---|---|
| 6 (the value fixed today) | 14.7 s | 0.98 MB |
| 9 | 11.1 s | 0.99 MB |
| 10 | **6.6 s** | 1.12 MB |

Going from 6 to 9 returns **25 % of the time for 1 % of weight**; going to 10 returns
**55 % of the time for 14 %**. A value fixed in the code therefore decides that
trade-off alone, with nobody able to see it or change it. It is the best
gain/effort ratio left in this whole document.

**What is not measured:** printing (the PDF path), and the memory cost
of a 45 Mpx pipeline, which will decide the depth of the pipelining considered at
point 2.

**Follow-ups, each with its own ADR:** exposing the AVIF encoding speed —
**delivered, [ADR 0067](adr/0067-avif-encode-speed.md)**: `avif_speed` in
`ExportSettings`, the default moved from 6 to 9 (28 % of the time and 53 % of the CPU returned
for 1.3 % of weight on the complete path), `--avif-speed` in the CLI and a
field in Studio — then pipelining the export batch — **delivered too, and
finding 2 above was wrong by an order of magnitude**,
[ADR 0068](adr/0068-concurrent-export-batch.md): it is not single-threaded
encoding that leaves cores idle, it is one photo's pipeline,
which uses only **289 % of 1,600 %** on sixteen threads. Processing 4 photos at a
time gives **2.81×** (and not 10 to 15 %), 6 give 3.41×, 8 regress. Neither
of the two touches reproducibility: the first concerns only the
codec, the second only the order of execution — verified, the twelve files of a
batch are byte-for-byte identical at every degree.

## B3 — GPU: reopening the question, on the preview path alone

**State.** [ADR 0012](adr/0012-rayon-data-parallelism.md) rejected wgpu — "higher
gains but inter-GPU determinism not guaranteed" — deferring to a later
exploration. ADR 0041 refused to reopen it, referring to an ADR
of its own **after the CPU optimisations had been measured**.

**The argument that stays valid.** Inter-GPU determinism is real: it is what
`pipeline.md` §5.1 refuses to let into the result.

**The argument that makes the question reopenable.** The §5.1 promise bears on the
**render**, that is, on what export and printing produce. The preview
is already a separate path, already not bit-for-bit with the export (a reduced proxy,
scaled radii), and already purely derived. **A preview-only GPU path,
with the CPU authoritative at export, would therefore not touch the promise** — that is
exactly the separation ADR 0041 already established for other reasons.

**Entry condition.** Reopen only **after B1**, with the post-cache
measurements in hand, as ADR 0041 requires. If B1 is enough to make
interaction fluid, the GPU no longer justifies itself at the price of a dependency and
a second render path to maintain.

**The answer on 2026-08-03: the condition is not met, and the GPU does not
justify itself.** Three measurements say so:

* **B1 made interaction fluid** — ~14 ms on an end-of-pipeline
  slider, that is, below the threshold at which the eye sees latency. There is no longer any
  annoyance to remove on the path where the GPU would be permitted.
* **On export, the GPU is expensive rather than forbidden.** This document
  first read §5.1 as a prohibition; [ADR 0080](adr/0080-the-promise-and-its-boundary.md) §3
  corrected that reading. §5.1 forbids changing the pixels of a *published*
  stage version, not publishing new ones: a GPU operator is a new stage
  version like any other, declaring its backend, with no silent CPU fallback.
  The obstacle is therefore the cost — one new version per operator ported,
  kept forever — and not the promise.
* **There is no deficit to catch up**: at comparable settings, Leyline
  exports as fast as the two independent implementations measured in B2, both of
  them on the CPU too. What a GPU would be meant to catch up with does not use
  one either.

And where time really is wasted — 15 cores idle during each
encoding — the answer is scheduling, not a second processor. **To be
taken up again if interaction ever becomes the painful point**, not before.

## B4 — Where the remaining headroom is — **measured on 2026-08-28**

**The question.** With B1, B2 and B3 closed, is there anything left, or is the
CPU path at its ceiling? Asked plainly, and answered by measurement rather than
by opinion.

### Parallel efficiency: largely spent

The bench binary run directly (not through `cargo`, whose start-up pollutes the
ratio), `--profile-time 10`, wall clock against CPU time, i9-9900K / 16 threads:

| Bench | Wall | CPU | Parallelism |
|---|---|---|---|
| `stages/local_brush_64_dabs` | 10.2 s | 138.6 s | **13.6×** |
| `stages/tone_sliders` | 10.1 s | 117.2 s | 11.6× |
| `stages/sharpening` | 9.6 s | 106.3 s | 11.1× |
| `stages/local_radial` | 10.1 s | 108.8 s | 10.8× |
| `stages/neutral` | 10.6 s | 114.1 s | 10.7× |
| `sections/full` | 10.1 s | 100.2 s | 9.9× |
| `denoise/v1_gaussian` | 10.0 s | 96.9 s | 9.7× |
| `denoise/v2_wavelet` | 10.1 s | 91.3 s | 9.1× |
| `denoise/v3_profiled` | 10.2 s | 92.8 s | 9.1× |
| `stages/dehaze` | 10.0 s | 89.3 s | 8.9× |

**Nothing is single-threaded**, and the spread is narrow: 56 % to 85 % of the
sixteen cores. The floor is the wavelet denoiser and dehaze at ~9×, the ceiling
the local brush at 13.6×.

That bounds the prize. Denoising is **70 % of a full edit** (183 ms of the
261 ms a full edit adds above the neutral floor, 3 Mpx), so lifting it from 9×
to the 13× the best stage reaches would return **about 20 % of a full edit** —
real, but neither transformative nor cheap: it is memory-traffic work on a
multi-pass transform, which is where parallel efficiency goes to die.

### SIMD: the lever is empty, and that is measured, not assumed

The workspace sets no `target-cpu`, so every binary is built for baseline
x86-64 — SSE2, on a CPU that has AVX2 and FMA. That looks like free money.

It is not. Rebuilt with `-C target-cpu=native`, run **back to back** against
the baseline binary on the same machine minutes apart:

| Bench | baseline | `target-cpu=native` |
|---|---|---|
| `sections/full` | 321.6 ms | 330.3 ms |
| `denoise/v2_wavelet` | 239.7 ms | 244.1 ms |
| `sections/tone` | 61.4 ms | 62.6 ms |

**~2 % slower**, consistently. The likely cause is the classic one on this
generation: all-core AVX2 downclocking costs more than the wider vectors
return, on code the compiler already auto-vectorises to SSE2. Whatever the
cause, the lever does not move, and it would have cost the reproducibility
promise a conversation for nothing.

### The measurement that nearly went wrong, and the rule it leaves

The first pass at the SIMD comparison put `target-cpu=native` **15 % slower**,
which would have been a striking and entirely false finding. The baseline it
was compared against was this document's own morning figure — and the same
baseline binary, re-run in the afternoon, had gone from 288 ms to 321 ms on
`sections/full`. **The machine had drifted 12 % during the day**; the
difference attributed to AVX2 was mostly thermal.

The rule this leaves is operational and applies to every future entry here:
**on this machine, any perf claim below ~10 % must come from back-to-back
runs**, never from a figure recorded earlier — however carefully that figure
was recorded. The same trap appeared twice on the same day: Criterion's stored
baseline, undated and silently overwritten, also offered a false 46 %
regression on `sections/full` that turned out to be a default denoise version
changing under it.

### Verdict

The CPU path is **at its ceiling for cheap wins**. What is left, in order:

1. **Denoise and dehaze parallel efficiency**, ~9× → ~13×: bounded at ~20 % of
   a full edit, and the hardest kind of work in this list;
2. nothing else with a measured lever. SIMD is empty, the GPU is closed by B3,
   export has no deficit to catch up (B2), import went 695 → 13 ms per file
   ([ADR 0082](adr/0082-embedded-preview-at-import.md)–[0083](adr/0083-scaled-jpeg-thumbnail-decode.md)),
   the stage cache took 78 % off an interactive slider (B1) and batching took
   2.81× on export ([ADR 0068](adr/0068-concurrent-export-batch.md)).

The honest summary is that performance is no longer where this project's next
gain lies.

---

# 4. Axis C — Optional local AI

## What does not change

The absence of AI in V1 was **the contract**, and this document does not disown it.
[`specification.md`](specification.md) §4 classes AI as a deliberate exclusion, while
leaving a door open: "an optional local AI remains conceivable in the very long
term" — a door [`roadmap.md`](roadmap.md) §Long term repeats. The present
axis **scopes that door**, it does not open it.

## The non-negotiable conditions

None of these conditions is a preference: each follows from a principle
already written. An AI project that violates a single one is to be refused.

1. **Entirely local.** No remote inference, no network call, ever
   — Local First ([`vision.md`](vision.md)).
2. **Optional and uninstallable.** Leyline must stay complete and coherent without
   the models. No core feature may depend on them.
3. **Zero telemetry.** Nothing leaves the machine, not even a usage counter.
4. **Determinism, or an explicit admission.** This is the hard point. A model produces a
   result that depends on the inference backend, the precision and the hardware —
   which is exactly what `pipeline.md` §5.1 refuses. Two ways out, to be settled
   in the ADR: either **the model's weights and the backend are pinned in the
   stage version** on the same footing as a constant, or the AI result is
   **materialised once** (a rasterised mask, stored in the revision) and
   it is that frozen result, not the model, that the pipeline replays.
   **The second way out is the only one that holds** the promise as it is
   written today.
5. **A GPL-3.0-compatible licence**, the model weights included. Many published models
   are not, and that is an eliminating criterion ahead of everything else.
6. **Weights distributed separately.** An installer cannot swell by
   several hundred megabytes for an optional feature.

## C1 — AI denoising

**The gap.** Learned denoisers have become *the* quality
differentiator at high ISO. Wavelet denoising
([ADR 0046](adr/0046-edge-preserving-denoise.md)) does not play in that
category, and no setting will bring it there.

**The particular difficulty.** Denoising acts **on the pixels**, so
condition 4 bites hard: it is impossible to "materialise once" a
denoising the way a mask is materialised, without storing an intermediate image
— which the non-destructiveness contract precisely avoids. It is the most
difficult subject in this whole document, and the only one for which I see no clean
solution at this stage.

**Recommendation.** Do not attack it first. A3 (a measured noise profile)
gives part of the gain, with none of these questions — and **it is delivered**
([ADR 0072](adr/0072-measured-noise-profile.md)).

**The verdict is better stated since [ADR 0073](adr/0073-external-mask-detectors.md):**
a denoiser produces **pixels**. It can therefore be neither materialised
once like a mask, nor cross the boundary of ADR 0069, nor enter the
render path without taking §5.1 with it. It is not "not now",
it is "not through this door".

## C2 — Automatic masks (subject, sky, background)

**The gap.** This is what people actually use for local retouching
since 2021. Leyline has geometric masks + range masks
([ADR 0048](adr/0048-range-masks.md)): the state of the art from before that shift.

**Why it is the right first candidate.** A mask **is** materialisable:
the model runs once, produces a mask, that mask is stored in the
revision, and the pipeline never replays the model again. Condition 4 is
satisfied by construction, with no compromise. The masking infrastructure
already exists ([ADR 0029](adr/0029-process-6-local-adjustments.md),
[ADR 0049](adr/0049-local-adjustments-clients.md)): the AI would be only **one
more mask source**, next to the brush and the gradient.

**How it attaches** — settled on 2026-08-04 by
[ADR 0069](adr/0069-closed-extension-boundary.md), which starts from the same finding as
the paragraph above and draws the licence consequence from it: since the
model **produces a setting** and renders nothing, the tool that runs it can
live in a closed, separate crate, a client of the SDK — without cloning the repository, without
removing anything from free editing, and without any closed code
entering the render path. The free version renders everyone's masks;
what is sold is the tool that *proposes* them.

**The socket was delivered on 2026-08-04** ([ADR 0073](adr/0073-external-mask-detectors.md)),
in the form chosen that day: **automatic detection** (a button,
"the sky", "the subject") rather than click-to-select. Three things
are worth remembering:

* **The open half was already there.** ADR 0070 had delivered `Mask::Coverage`
  and `store_mask_coverage`, ADR 0071 the overlay. Only the
  gesture was missing — and the engine did not move a line.
* **A detector is an executable, not a plugin.** It takes a PNG, returns a
  16-bit grey PNG, does not open the library, takes no lock and does not
  link the SDK: the licence boundary is crossed by an `execve`, which
  is the hardest point one can reach. A happy and
  unsought consequence: anyone can write one in twenty lines, so
  the socket has a value of its own for the free project.
* **The licence of the weights eliminates, and it had to be looked at first.** SegFormer
  ADE20K (NVIDIA) and RMBG-1.4 — the two easiest models to find —
  are non-commercial. U²-Net (Apache-2.0), BiRefNet (MIT) and the
  MMSegmentation zoo (Apache-2.0) pass.

**Still open.** The detector itself: the choice of model per detection, its
conversion to ONNX, its measurement — all of that lives in its own repository. And, on
the day it comes, the key system (out of scope of ADR 0069 §5).

---

# 5. Recommended sequencing

The order follows the **felt gain / risk** ratio, not difficulty.

| # | Item | Why here | New stage version? |
|---|---|---|---|
| ~~1~~ | ~~**B1** — stage cache~~ | **Delivered on 2026-08-02, −78 %** | No |
| ~~2~~ | ~~**A1.1** — validate DCP~~ | **Done on 2026-08-02/03**: container bug fixed, algebra validated against an independent implementation (median discrepancy 0.0027); "experimental" kept for the unexplained ×1.185 gain | No |
| ~~3~~ | ~~**A2** — demosaic choice~~ | **Delivered on 2026-08-02** ([ADR 0061](adr/0061-demosaic-algorithm.md)) | Yes (`input::v3`) |
| ~~4~~ | ~~**A1.2** — apply the DCP tables~~ | **Delivered on 2026-08-02** ([ADR 0062](adr/0062-dcp-illuminant-interpolation.md), [ADR 0063](adr/0063-dcp-tables.md)) | Yes (`camera_profile::v2`, `v3`) |
| ~~5~~ | ~~**B2** — measure the export~~ | **Measured on 2026-08-03**, see §3: the `export.rs` bench, two possible follow-ups identified | No |
| ~~6~~ | ~~**A3** — noise profile~~ | **Delivered on 2026-08-04** ([ADR 0072](adr/0072-measured-noise-profile.md)): darktable's table frozen with the stage, a per-pixel threshold, ranks 5 and 6; measured cost nil | Yes (`noise_*::v3`) |
| ~~7~~ | ~~**B3** — preview GPU~~ | **Rejected on 2026-08-03**: B1 made interaction fluid, the GPU is forbidden at export by §5.1, and the comparison shows there is nothing to catch up (see §3) | — |
| ~~8~~ | **C2** — AI masks | **Socket delivered on 2026-08-04** ([ADR 0073](adr/0073-external-mask-detectors.md)); the detector remains, outside this repository | No (a materialised mask) |
| 9 | **C1** — AI denoising | A distant horizon; the determinism question unresolved | To be settled |

**Hard dependencies:** A1.2 after A1.1; B3 after B1 (required by ADR 0041).
Everything else can ship in any order.

**Still open as of 2026-08-04:** nothing, in this repository. Axes A and B are
closed — A1, A2 and A3 delivered, B1 and B2 too, B3 rejected with its reasons — and
of axis C, everything that could enter it has entered: C2's socket is
delivered, the detector that plugs into it lives elsewhere, and C1 has no way out (see
below). What remains is no longer a project but three findings: the
comparison with the profile vendor's own rendering, for want of its
software; the
pinning hole in the Lensfun database that ADR 0072 brought to light; and the
quality gap, accepted, with learned denoisers.

---

# 6. Before any line of code

Every item in this table requires an ADR of its own. The present document
stands in for none of them.

| Item | What the ADR must settle |
|---|---|
| A1.2 | Table interpolation, order of application, a new stage version |
| A2 | The default algorithm, the values exposed, writing into the revision |
| A3 | **Settled** by [ADR 0072](adr/0072-measured-noise-profile.md): darktable's table under GPL-3.0-or-later, frozen with the stage version, a per-pixel threshold, ranks 5 and 6 |
| B1 | Nothing — [ADR 0041](adr/0041-interactive-preview-rendering.md) §3 is already the ADR. **Implemented on 2026-08-02** |
| B2 (follow-ups) | The AVIF encoding speed exposed and its default; overlapping encoding and rendering in a batch, and what becomes of the report's order |
| B3 | The preview-only scope, the backend, and what becomes of §5.1 in the text |
| C2 | **Settled** by [ADR 0073](adr/0073-external-mask-detectors.md): automatic detection, a detector = a separate executable, the licence of the weights eliminating |
| C1 | The six conditions of §4 above — and first of all §4.4, which a denoiser cannot satisfy |

---

## Related documents

* [`specification.md`](specification.md) — what is delivered, what is excluded.
* [`roadmap.md`](roadmap.md) — the actual state, phase by phase.
* [`pipeline.md`](pipeline.md) §5 — the reproducibility promise, which every
  item in this document must respect or explicitly amend.
* [`v2-implementation-plan.md`](v2-implementation-plan.md) — the previous
  sequencing document, now an archive.
