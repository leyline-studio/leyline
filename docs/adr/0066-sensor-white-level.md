# ADR 0066 — The white level comes from the sensor, not from the photo

**Status:** Accepted — 2026-08

## Context

The numbers in a RAW file mean nothing until something has designated **the
value that counts as "white"**. The whole rendering follows from it: dividing
by too high a level darkens the entire image and makes a pixel saturated at the
sensor come out grey.

Leyline had never made that choice. It took whatever LibRaw left in `maximum`,
naming it nowhere — and what LibRaw leaves there **depends on the photo**.

### What LibRaw does by default

`params.adjust_maximum_thr` is 0.75. Before normalization, LibRaw then lowers
`maximum` down to the brightest sample **of that particular image**, as soon as
it exceeds 0.75 of the format's ceiling. Measured on four files from one
series, Canon 60D, ISO 100, the same exposure:

| File | Brightest sample | White level retained |
|---|---|---|
| IMG_9040 | 13,794 | **13,794** — its own brightest pixel |
| IMG_9041 | 10,828 | **16,383** — the 14-bit ceiling |
| IMG_9042 | 2,807 | 16,383 |
| IMG_9044 | 1,641 | 16,383 |

Two photos of the same scene, taken one after the other, therefore end up
normalized by levels 19 % apart — depending on whether a specular reflection
fell inside the frame or not. The neutral rendering's brightness depended on
the image's content.

**That is exactly what `auto_brighten: false` forbids.** That field has carried,
since V1, the comment "the neutral rendering must not depend on the image's
content". The intent was right; it was circumvented one stage below, by a
setting nobody had looked at.

### What the body, for its part, knows

The file carries the answer. LibRaw reads from the Canon metadata a **linearity
margin** — the level beyond which the sensor stops responding proportionally —
and files it in `linear_max`. It is a value **written by the body for that
particular shot**, never derived from the pixels: that is what makes it usable
here.

It follows the sensitivity first, in three groups:

| ISO group (60D) | `linear_max` |
|---|---|
| 100, 125 | 12,279 |
| 200 … 3200 | 15,094 |
| 160, 320, 640, 1250, 2500 | 11,222 |

The split is **the same** as the one in the measured table RawTherapee
maintains on its side (`camconst.json`): two independent observers of the same
hardware behaviour. `identify` uses it, besides, to fill `maximum` — and
`unpack` then overwrites it with the format's ceiling.

And it also follows the **aperture**, which a survey of 250 real files from the
corpus showed after the fact — the body's metadata already carries what
RawTherapee has to model by hand in an `aperture_scaling` table:

| Aperture (60D, ISO 100) | `linear_max` | Ratio to f/4 | RT's `aperture_scaling` |
|---|---|---|---|
| f/1.8 | 13,926 | 1.134 | 1.140 |
| f/2.8 | 12,749 | 1.038 | 1.030 |
| f/3.2 | 12,632 | 1.029 | 1.015 |
| f/4 and beyond | 12,279 | 1.000 | 1.000 |

The two sources agree to ~1 %. That is this decision's decisive argument: **the
file already knows what a third-party table would have to learn body by body,
and it knows it shot by shot.**

## Decision

**The white level is the one the body wrote, and never the one the photo
contains.**

### 1. The rule

1. The file's linearity margin (`linear_max`), when the body writes one;
2. otherwise, the format's ceiling (`maximum`), as before.

And in both cases, **adjustment by content is switched off**
(`adjust_maximum_thr = 0`): without that, the second case would stay dependent
on the image for bodies with no metadata, that is, precisely where nothing can
be verified.

The level therefore no longer depends on anything but what the body recorded
for that shot — sensitivity, aperture — and never on what the photo contains.
Two photos from one series at last render alike.

### 2. A new version of the `input` stage

`input::v4`. The rendering changes, so the stage version changes
(`pipeline.md` §5.1) — an existing revision goes on rendering through `v3`,
unchanged, until someone reprocesses it.

Unlike `v2` and `v3`, **`v4` is not identical to its predecessor at neutral
settings**, and cannot be: redefining white is all it does. A photo reprocessed
into `v4` becomes 12 to 33 % brighter depending on what `v3` had retained for
it.

### 3. What the reference renders cannot freeze

The goldens render a synthetic buffer and never go through LibRaw: `v3` and
`v4` produce the same pixels there, to the bit. What `v4` changes cannot be
frozen there. So a decoder-configuration test fixes it
(`the_white_level_reaches_the_decoder_only_from_input_v4`), plus a test on a
real file behind `LEYLINE_TEST_RAW`. The same hole, the same remedy as the
highlight mode of [ADR 0050](0050-highlight-reconstruction.md).

### 4. What this decision does **not** settle

RawTherapee still renders brighter than Leyline: ×1.16 before, ×1.03 after on
an ISO 100 file, and the gap *widens* on an ISO 400 file (×1.08 before, ×1.13
after) because `v3` there stretched white up to the brightest pixel of an image
that had no very bright one.

After this correction there therefore remains **a roughly constant factor of
1.14 between the two engines, which is not the white level**: the effective
divisors on both sides are known and do not explain that gap. That is not a
reason not to correct what is corrected here — dependence on content is a flaw
in itself, independently of any comparison. It is a reason not to claim the
subject is closed.

## Consequences

* A pixel saturated at the sensor comes out **white**, which was not the case.
* A neutral rendering's brightness no longer moves from one photo to the next
  in the same series.
* `leyline-raw` exposes `WhiteLevel`, two named values; the type's default
  stays `FormatCeiling`, which the frozen `input` versions ask for explicitly
  since this ADR.
* The shim gains two functions, `linear_max` (reading, **before `unpack`**,
  which overwrites the value) and `user_sat` (writing).
* Existing libraries do not change appearance until a reprocess asks for it.
* **The fallback is real and tested**: over 250 CR2s from the corpus (60D and
  5D Mark IV), no file without a margin; the DNGs from an HTC 10 in the same
  corpus carry none and therefore fall back on the format's ceiling, which is
  exactly the intended path.

## Alternatives rejected

* **Taking over RawTherapee's `camconst.json` table** (GPL-3.0, hence legally
  possible here with attribution): it is a third-party database to maintain, to
  extend body by body, and to take over again with every new measurement on
  their side — when the survey above shows that the file's metadata already
  says the same thing, aperture included, to within ~1 %, for every body and
  with nothing to maintain. The author's decision, besides: take nothing over
  from another project.
* **Keeping adjustment by content** (LibRaw's default): that is the flaw
  corrected here.
* **Setting `adjust_maximum_thr` higher** rather than switching it off: it
  moves the threshold without removing the dependence — the same photo with and
  without a reflection would still render differently, a little less often.
* **Correcting without a new stage version**: it would break `pipeline.md`
  §5.1, which is the project's dearest promise.
