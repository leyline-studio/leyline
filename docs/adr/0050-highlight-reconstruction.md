# ADR 0050 — Highlight reconstruction: a decoding mode, pinned by `input::v2`

**Status:** Accepted — 2026-07
**Followed by:** `input::v2`, which this ADR creates, is no longer the current
version. [ADR 0061](0061-demosaic-algorithm.md) makes the demosaicing algorithm
selectable (`v3`, bit-for-bit identical to `v2` at the neutral setting), and
then [ADR 0066](0066-sensor-white-level.md) takes the white level from the
sensor rather than from the photo's content (`v4`, deliberately **not**
identical to its predecessor). The highlight reconstruction decided here passes
through both unchanged.

## Context

The decoder has never been given an instruction about clipped highlights.
`crates/leyline-raw/src/shim.c` does not touch `params.highlight`, whose LibRaw
default is **0 — clip at white**. Every channel that saturated at the sensor
therefore comes out at the maximum value, and the information the other two
channels still carry is thrown away before the first slider.

What that costs is visible on any photo where one channel saturates alone,
which is the common case: a light blue sky (blue saturates first), skin in full
sun (red), a bright cloud. The area comes out as a flat white, and no
downstream setting can rebuild it — `Settings::highlights` brings a brightness
down, it does not reinvent a lost channel.

Competitors handle exactly this case, and have long done so: dcraw has exposed
it as `-H` for twenty years, RawTherapee makes it a module with four methods,
darktable has two (including its *guided laplacian*). It is Leyline's most
visible rendering flaw against them, and it does not come from a judgement
call: nobody had asked the question.

**What is not at issue.** The unbounded working space of
[ADR 0044](0044-linear-wide-gamut-working-space.md), which is what makes this
decision useful — a buffer clipped at white would have nowhere to put what is
rebuilt. Nor the output shoulder (`output_rendering`), which decides what
*becomes* of the headroom above white and not what fills it.

## Decision

### 1. Reconstruction is a decoder configuration, not an operator

LibRaw's modes operate on the data **before demosaicing**, where a saturated
pixel's neighbourhood is still a mosaic of distinct channels. That is the only
place the necessary information exists: after demosaicing, a clipped pixel is
surrounded by pixels already interpolated from clipped channels.

Writing our own reconstruction would therefore mean first exposing the raw
mosaic through the shim, then reimplementing — less well — an algorithm LibRaw
already ships, tested on thousands of bodies. LibRaw's mode is retained; §1 of
the rejected alternatives says why the question may one day be asked again, but
not here.

A direct consequence: it is a setting that **`input` pins**, since `input` is
precisely the stage carrying "the configuration asked of the decoder" (ADR 0044
§3, `docs/pipeline.md` §3.3). It creates no new stage and moves no rank.

### 2. Three modes, not nine

LibRaw's `params.highlight` accepts 0 to 9. Leyline exposes three:

| Setting | LibRaw | What it does |
| :--- | :---: | :--- |
| `clip` (default, neutral) | 0 | clip at white — the behaviour from before this decision |
| `blend` | 2 | blend the clipped and unclipped channels: recovers texture without drifting in colour |
| `rebuild` | 5 | rebuild the missing channel from the others: recovers the most, at the price of a hue risk in very saturated areas |

Mode 1 (*unclip*) is not exposed: it lets the highlights take on the
characteristic magenta cast of a channel left beyond the others, which looks
like a bug to any user who has not read dcraw. Levels 3 to 9 are one family
with a strength dial; 5 is the median value and the one dcraw documents as a
starting point. Exposing an integer from 3 to 9 would ask the user to guess
what the number means.

`clip` stays the default. That is not an aesthetic preference: it is the
project's rule — a setting's neutral value is the one that changes nothing —
and it is doubly necessary here, since changing the default would modify the
rendering of every already-imported photo.

### 3. Giving back the gain the decoder takes away

Measured on a real CR2: asking for `blend` or `rebuild` **darkens the whole
photo** by about a third, highlights included. That is not an implementation
flaw, it is how dcraw works, inherited by LibRaw: the normalization by the
white-balance multipliers divides by the **smallest** of them when clipping —
every channel then rises to 1 or above, and the strongest saturates — and by
the **largest** when rebuilding, so that no channel can exceed white. The gap
between the two is a global gain, identical for every pixel.

Leaving it as it is would be unacceptable: "recovering the highlights" would
look like an exposure slider, and the user would compensate by hand without
knowing why. `input::v2` therefore **gives it back**, multiplying the buffer by
the ratio `max/min` of the body's as-shot multipliers, which `leyline-raw`
exposes for that purpose (`RawMetadata::camera_multipliers`).

The result is exactly what the function must be: the midtones come back where
clipping put them — measured to within 0.1 % on the same file — and what has
been rebuilt lands **above white**, where ADR 0044's unbounded buffer keeps it
until `output_rendering` decides its fate. It is an operation on the highlights,
not on the exposure.

A file with no recorded white balance receives no compensation: no multipliers,
hence no ratio to give back — and no invented correction.

### 4. `input::v2`, and a `v1` that does not move

The decoding mode is part of the rendering, hence of `docs/pipeline.md` §5.1's
promise. It needs a new stage version:

* `input::v1` goes on asking exactly what it asked and **ignores** the setting;
* `input::v2` reads the setting and passes it to the decoder; its conversion to
  the working space is a copy of `v1`'s (ADR 0042: duplication is the price of
  freezing);
* new revisions pin `input: 2`, old ones keep `input: 1`.

The `INPUT_DECODE` table that maps a version of `input` to its decoder
configuration sees its signature go from `fn(bool)` to `fn(&Settings, bool)`.
That is not an edit to a published version in ADR 0042 §1's sense: what the
freeze protects is **what `v1` asks of the decoder**, and `v1` asks the same
thing as before while ignoring its new argument.

### 5. A non-neutral mode on a revision pinned at `input: 1` is **refused**

It is [ADR 0048](0048-range-masks.md) §5's case, word for word: a 2026 revision
pins `input: 1`, the user asks for `rebuild` on it in 2027, the pinning rule
keeps `v1`, and `v1` knows nothing of the setting. The mode would disappear in
silence.

`Settings::validate()` therefore refuses the combination, and the message names
the remedy: reprocess the photo (`docs/pipeline.md` §4.5), which creates a
revision pinned at `input: 2`. It is the second application of the general rule
ADR 0048 §5 drew out — **a setting a pinned version cannot express is a
validation refusal, never a lost value** — and the first that concerns not a
pixel operator but the decoder.

### 6. What reproducibility covers here

The reconstruction is deterministic: the same input bytes, the same parameters,
the same pixels. It does however depend on the **version of LibRaw**, as all
decoding has since day one — which `docs/pipeline.md` §5.2 already files under
"changing platform". This decision does not widen the ungaranteed zone: it adds
to it a parameter whose effect is visible, where decoding was already entirely
inside it.

### 7. Out of scope

* **The choice of demosaicing algorithm** (`params.user_qual`), today left at
  LibRaw's default. The same family of question — a decoder parameter `input`
  would pin — but entirely different judgement calls; its own ADR.
* **An in-house reconstruction from the mosaic** (§1 of the alternatives).
* **A strength dial for `rebuild`** (levels 3 to 9). Addable later with no new
  decision: it would be one more mode in the same enumeration.

## Consequences

* **The most visible rendering flaw against darktable and RawTherapee
  disappears**, for a three-valued setting and one stage version.
* **ADR 0044's unbounded buffer finally serves what it promised**: what
  `rebuild` raises above white travels through the whole pipeline and it is
  `output_rendering`'s shoulder that brings it back — the two decisions compose
  exactly as foreseen, without either having to know the other.
* **`leyline-raw` exposes one more piece of data, and one only**: the as-shot
  multipliers, for §3's gain. No other stage reads them.
* **One more reference render**, which freezes §3's compensation — the only
  part of this decision a synthetic buffer can exercise, the mode itself living
  in the decoder.
* **The decode cache stays correct without being touched**: it is indexed by
  `(asset, DecodeParams)`, so two modes are two entries.
* **The project's third real stage version** (after 0046 and 0048), and the
  first on a framing stage — which exercises the fact that `input` pins
  something other than pixels computed by us.
* **One more setting above white, but no schema changed**: an optional field
  whose neutral value is absence (`docs/pipeline.md` §3.4).

## Alternatives rejected

* **Rebuilding it ourselves from the raw mosaic.** It would mean exposing the
  Bayer data through the shim, handling non-Bayer patterns (X-Trans), and
  reimplementing a proven algorithm. The day demosaicing becomes a project
  choice (out of scope, §6), the question will be asked again in a frame where
  it makes sense; today it would add risk and gain nothing.
* **Rebuilding after demosaicing, in a stage of our own.** Appealing because it
  would stay in code frozen by us rather than in LibRaw — but the necessary
  information no longer exists at that point (§1). One would get a smoothing of
  white areas, not a reconstruction.
* **Turning `blend` on by default.** A better rendering for almost any photo,
  and unacceptable: it would change the rendering of what exists, which the
  publication rule forbids (§5.1).
* **Exposing LibRaw's nine modes.** An enumeration whose members the user
  cannot predict is not a setting, it is a form.
* **Passing the mode through a render option rather than through the
  revision's settings.** It would change the pixels without being recorded in
  the revision — exactly what ADR 0044 §3 corrected for `camera_native`.
