# ADR 0072 — Denoising finally knows which sensor it came from (`noise_luminance::v3`, `noise_color::v3`)

**Status:** Accepted — 2026-08

## Context

[ADR 0046](0046-edge-preserving-denoise.md) replaced a blur with a real
operator — à-trous wavelets, soft thresholding — and stated, in its §7, what it
did not do:

> **No per-body, per-ISO noise profile.** The threshold is a uniform white-noise
> model, not the sensor's measured variance at that sensitivity.

That is item **A3** of [`measured-findings.md`](../measured-findings.md), the
last one on the "rendering accuracy" axis to have stayed open. The present ADR
settles it.

### What "uniform white noise" costs

`v2`'s threshold is `k · BASE · σ_l`: three constants, the same for every file
in the world. But a sensor's noise is neither uniform nor constant, and it
varies along **two axes** that this threshold ignores, both of them.

**Sensitivity.** Between ISO 100 and ISO 12800, the noise standard deviation of
a Canon 60D at mid-grey goes from 0.0015 to 0.0131 — a factor of **nine**. A
single threshold can therefore only be right at one sensitivity: it destroys
detail for nothing below, and does not reach the noise above. Concretely, "30"
on the slider does not mean the same thing from one photo to the next, and the
user spends their time rediscovering that file by file.

**The signal level.** Photon noise is Poissonian: its variance **grows with the
light received**, and its *relative* standard deviation decreases. A constant
threshold is therefore too weak in the shadows — where the noise shows — and too
strong in the highlights, where there is almost nothing to remove and where it
eats texture.

### What noise actually is

The standard model, the one darktable, DxO and the literature (Foi et al.)
measure, is **Poissonian-Gaussian**: for a raw value `x` normalized on
`[0, 1]`,

```
var(x) = a · x + b
```

`a` carries photon noise (proportional to the signal, and proportional to the
sensitivity), `b` read noise (constant, independent of the signal). Two numbers
per channel and per sensitivity are enough to describe a sensor — that is
little, and it is measurable.

## Decision

### 1. The measurements come from darktable's database, under its licence

Leyline embeds the darktable project's `data/noiseprofiles.json` table: **434
bodies**, 7,842 `(ISO, a, b)` triples per channel, measured one by one by that
project's contributors since 2014.

**The licence permits exactly that.** darktable is published under
**GPL-3.0-or-later**, Leyline under [GPL-3.0-only](../../LICENSE): a work under
"v3 or later" enters a work under "v3" without difficulty, that being the very
meaning of the clause. The attribution and the original licence are recorded in
the embedded file's header and in [`architecture.md`](../architecture.md)
§External bricks, on the same footing as Lensfun or LittleCMS.

**What we cannot do ourselves**, and this is the underlying reason: measuring a
profile requires a series of controlled exposures **per body and per
sensitivity**. The development corpus covers two bodies; the database covers
434. A RAW developer whose denoising were profiled only for its author's two
cameras would not be a RAW developer. Measuring stays possible — the table's
format is upstream's, and a missing body is added to it with the same two
numbers per channel and per sensitivity — but it is a complement, not the base.

**The table is reduced, not transformed.** The `name` and `comment` fields are
removed, the numbers rounded to six significant digits (the buffer is `f32`,
which carries seven), and ISO duplicates — several contributors having measured
the same body — are resolved by **the first entry in upstream file order**, so
that the rule is a rule and not an accident of iteration. The file keeps one
line per body, which makes it diffable against upstream, and its header pins the
origin commit (`e333310b`, 2026-08-03): the provenance is verifiable, not
declared.

### 2. The table is frozen with the stage version

This is the point that decides all the rest. A table that evolved under a
published stage version would change the rendering of an existing revision —
which is exactly what [`pipeline.md`](../pipeline.md) §5.1 forbids.

So: **the table belongs to the version**. `kernel::v3` incorporates it via
`include_str!("../../../data/noise_profiles_v1.json")`, on the same footing as a
constant. Updating the measurements, or adding bodies, produces a
`noise_profiles_v2.json` **and** new stage versions: never a modification of the
existing file. The cost is one more 774 KB file per update, and it is owned —
that is the exact price of the promise.

**By contrast, what this decision brings to light.** The Lensfun database is
embedded in the binary the same way (ADR 0016) and is **pinned by nothing**: the
day the embedded version changes, a revision citing `lens::v1` will render
differently. That is a real hole in §5.1, discovered while writing this
document, outside its scope, and recorded here so it is not rediscovered a third
time.

### 3. The threshold becomes a per-pixel threshold

ADR 0046's operator does not change — à-trous decomposition, soft thresholding,
residual never thresholded. What changes is the threshold, which stops being a
number and becomes a function of the pixel:

```
t_l(i) = k · SIGMAS · σ_l · √( max(a · L_i + b, 0) )
k = strength / 100
```

`L_i` is the luma plane, computed **once** before the decomposition: it is the
signal estimator, and it does not need to be better than that — an error of ±σ
on `L` moves `√(a·L + b)` by far less than the ratio of 9 between two
sensitivities. `σ_l` stays ADR 0046 §3's per-scale profile
(`0.890, 0.201, 0.086, 0.041`), which describes how the transform distributes
white noise across its levels — the measured model says *how much* noise there
is, the per-scale profile says *where* it goes.

`b` can be negative in the database (a fitting artefact on certain bodies; the
60D gives some at every sensitivity): the variance is therefore clamped to zero
before the square root, which simply makes the model purely Poissonian where the
fit intended it that way.

**The two constants.** `SIGMAS_LUMA = 6` puts the slider at mid-travel
(`strength = 50`) on **3 σ**, the textbook value for a soft threshold, and
leaves it room to go to double. `SIGMAS_CHROMA = 10` stays more aggressive, for
the reason that already held in `v2` — a photo's chrominance is smooth almost
everywhere — but **not** in the 2.5 ratio that `v2` expressed through its `BASE`
values (0.05 and 0.12): part of that ratio is now carried by the measured σ
itself, which comes out about 1.7 times larger on a chrominance plane than on
luma (`chroma_terms` §5). Counting it twice would flatten real colour. Those two
constants are frozen on the same footing as the table.

**On a reduced preview** (ADR 0041), the noise has already been averaged by the
reduction: `n × n` averaged pixels divide its standard deviation by `n`. The
measured σ is therefore multiplied by the `scale` factor before serving as a
threshold, which is the only way for the preview and the export to show the same
denoising. The number of levels continues to follow `levels_at_scale`
(ADR 0046 §5); the two corrections are independent and both necessary.

### 4. The stages move up to the head of the pipeline (ranks 5 and 6)

A model measured on the sensor's numbers no longer means anything once exposure,
contrast, the tone curve and clarity have gone by. At ranks 170 and 180, `v2`
worked on an image where nothing any longer connected a pixel's value to the
light received by the photosite. A profile there would be decoration.

`noise_luminance::v3` therefore takes **rank 5** and `noise_color::v3` **rank
6** — between `input` (rank 0) and `camera_profile` (rank 10), the only place in
the pipeline where the buffer is still a **linear** transformation of the
sensor's counts. The rank is a property of the version, not of the operator
([ADR 0042](0042-versioned-stage-pipeline.md) §3): it is precisely that
mechanism that makes this move possible without touching `v1` or `v2`, which
stay at ranks 170 and 180 for the revisions that cite them.

Three consequences, in the order in which they matter:

* **Denoising precedes geometry** (`lens` at rank 20 resamples, `rotate` and
  `perspective` too). That is the right place: after a resampling, the noise is
  no longer independent from one pixel to the next and no per-pixel model
  describes it any more.
* **The display axis is abandoned.** `v1` and `v2` worked under `in_display`
  (ADR 0046 §4) because a constant threshold in linear light would be enormous in
  the shadows and negligible in the highlights. The threshold is no longer
  constant: it follows the signal, which the display curve only approximated. Two
  fewer non-linearities per render, and the model applied where it was measured.
* **The stage cache changes hands.** The noise slider was the fourth before the
  end; it becomes the second after the start. Moving *that* slider now replays
  the whole pipeline, while **every other slider** finds the denoising — the
  pipeline's most expensive stage — already done in the cache. No checkpoint has
  to be added for that: the first of ADR 0041 §3's is taken *before* rank 40, so
  after ranks 5 and 6, and it therefore already captures the denoised buffer.

### 5. Carrying the model over into the buffer's space

The database's coefficients are measured on the sensor's raw values, channel by
channel, **before white balance**. Rank 5's buffer is not that space: it is a
known linear transformation of it, in two steps.

**The body's white balance.** LibRaw multiplies channel `j` by
`g_j = m_j / min(m)`, where `m` are the shot's multipliers
(`SourceColor::Camera::multipliers`). Multiplying a sample by `g` multiplies its
variance by `g²`, hence:

```
a_j ← g_j · a_j        b_j ← g_j² · b_j
```

(The form of `a` follows from the change of variable: `var(g·x) = g²(a·x + b)`
and `x = y/g` give `g·a·y + g²·b`. It is the computation darktable does by
dividing the pixel by `wb` before evaluating its model.) The highlight
reconstruction mode changes nothing about that gain: LibRaw there divides by
`max(m)` instead of `min(m)`, and `input::v2` renders exactly that ratio
([ADR 0050](0050-highlight-reconstruction.md) §3).

**The colorimetric matrix.** Without a DCP profile, `input` has already applied
`M = camera_to_rec2020`; with a profile, the buffer is still camera-native and
`camera_profile` (rank 10) will do the conversion later. In the first case, a
linear combination of independent variables gives
`var(y_k) = Σ_j M_kj² · var(x_j)`, whence, under the grey assumption (`x_j ≈
y_k`, which `M`'s normalization makes coherent on a neutral):

```
a'_k = Σ_j M_kj² · a_j      b'_k = Σ_j M_kj² · b_j
```

In the second, the transformation is the identity and the model applies as it
stands. The stage knows which of the two cases it is in — that is what
`ctx.camera_profile.is_some()` says, and `input::v2` already makes its decision
on that same boolean.

**From there to the two planes processed.** Luma is `Σ w_k y_k` with
`pixels::luma`'s Rec. 2020 weights, hence `a_L = Σ w_k² a'_k`. Channel `k`'s
chrominance is `y_k − L`, hence `a_C,k = (1 − w_k)² a'_k + Σ_{j≠k} w_j² a'_j`.
The same formulas on `b`. Nothing there is hand-tuned: every coefficient
descends from the definition of the plane it describes. Those weights are
`pixels::luma`'s, hence Rec. 2020's: in the "DCP profile" case, where the buffer
is still camera-native, they are applied to channels that are not theirs. It is
the operator itself that already makes that choice — `v1` and `v2` extract the
same luma — and the resulting error on a **threshold** is in no way comparable to
what the profile brings.

**What is not carried over, and why.** The white level. Our values are
normalized by the body's linearity margin
([ADR 0066](0066-sensor-white-level.md)), the database's by darktable's white
constant — a ratio of the order of 1.1 on a 60D, hence ~10 % on σ. That is
derisory next to the factor of 100 the sensitivity scale covers, and next to the
uncertainty of the fit itself. It is said here so that nobody has to deduce it
from a silence.

### 6. Body matching, and interpolation in sensitivity

The lookup takes the shot's EXIF make, model and sensitivity. The catalog
**already strips the make from the model** (`exif::without_brand`, and LibRaw
does it on its side): `Canon` / `EOS 60D`, which is exactly the database's form.
The comparison is made up to case and spaces, and nothing more — an approximate
match on a body name would give another sensor's profile, which is worse than no
profile.

**Between two measured sensitivities, the coefficients are linearly
interpolated** in ISO, and clamped at the ends of the scale. The database is
dense (the 5D Mark IV has 29 entries there, from 50 to 102,400): the
interpolation only fills in thirds of thirds of a stop, never a hole.

**RAW sources only.** A JPEG has already been through the body's denoising and
its curve: the model no longer describes anything there. `SourceColor::Srgb`
therefore falls into §7's fallback, like an unknown body.

**Verified on real files, and not only on hand-written strings.** A failing match
breaks nothing: it falls back, silently — exactly the defect ADR 0035 let
through by reading only its own fixtures. A test ignored by default
(`LEYLINE_TEST_RAW`) therefore starts from a CR2 of the corpus, pulls its
metadata through the real import path and **requires** the table to answer. Both
available bodies pass: Canon EOS 60D and Canon EOS 5D Mark IV.

### 7. With no match, a default model — not a mute stage

The lens stage, lacking a profile, corrects nothing (ADR 0016 §2). That does not
transpose here: an unknown lens means "no correction was asked for", whereas an
unknown body would make whoever owns a camera missing from the database lose
**denoising itself** — and the version pinned for every new revision is `v3`.

The fallback is therefore a model, explicit: `a = 0`, `b = σ₀²` with **σ₀ =
0.003** — a signal-independent noise, of the order of what the database gives for
an APS-C SLR around ISO 800. It is exactly `v2`'s assumption, put back into
linear light: the unknown body recovers the behaviour from before this ADR,
neither more nor less. The constant is defined in the buffer's units and is
**not** carried over by §5: carrying over an invented number would not make it
truer.

### 8. What this decision does not change

* **The settings surface.** Two `0..100` sliders
  (`noise_reduction.luminance`, `noise_reduction.color`), `settings_json`
  unchanged, `schema` not incremented. No client — Studio, CLI, SDK — has a field
  to add. As for ADR 0046, they are the same values, better spent.
* **Existing revisions.** `v1` and `v2` are untouched and stay recorded at their
  ranks. A revision citing them renders identically, forever; the user who wants
  profiled denoising reprocesses their photo, which creates a new revision.
* **The revision learns nothing about the sensor.** Make, model and ISO are
  properties **of the file**, not of the user's intent: they come in by the same
  path as `SourceColor` and the `LensShot`, that is, an argument of `render`, and
  not through `settings_json`. A preset therefore stays applicable from one body
  to another — and that is the decisive reason not to materialize the coefficients
  in the revision.

## Consequences

* **The slider finally means something.** At an equal setting, an ISO 100 photo
  is barely touched and an ISO 6400 photo is frankly denoised: the sensor carries
  the difference now, not the user.
* **The measurements file weighs 774 KB in the binary**, loaded and parsed once
  per process, at the stage's first use (`OnceLock`) — never at application
  startup.
* **The measured threshold costs nothing.** The `denoise/` bench compares the
  three pinned versions, at identical settings (luminance 40, chroma 30) on the
  3 Mpx synthetic frame: **98 ms for `v1`, 231 ms for `v2`, 219 ms for `v3`** — a
  complete render, not the operator alone. `v3` is therefore slightly *faster*
  than `v2`: one square root per pixel and one more σ plane, against the two
  round-trip passes to the display axis that §4 removes, `display()` and
  `linear()` calling `powf` on every sample. The trade is favourable, which was
  not foreseen.
* **The golden manifest gains entries, none of them moves.** The `noise_*::v3`
  variants are added to it, one of them exercising the real match (Canon EOS 60D
  at ISO 3200) and one the fallback. Blessing stays additive.
* **`render` takes one more argument** (`sensor: Option<&SensorShot>`), and
  [`engine-api.md`](../engine-api.md) documents it. It is the eighth argument of
  a function that carried seven: grouping those resolved inputs into a single
  structure is a simplification to be made, and it has no place in the same change
  as this one.
* **`docs/pipeline.md` §3.3 gains two lines**, and the stage table's order stops
  being the order of their historical numbering: two stages now appear there
  twice, at two different ranks. It is the first time a version has changed rank,
  and ADR 0042 §3's mechanism therefore serves what it was written for.
* **A3 is closed** in [`measured-findings.md`](../measured-findings.md), and C1's
  argument ("A3 gives part of the gain AI denoising aims at, with none of its
  questions") becomes verifiable.

## Alternatives rejected

* **Measuring the database ourselves.** Two bodies against 434, for months of
  controlled exposures and a protocol to validate. The protocol stays useful — it
  is the means of adding a missing body — but it cannot *be* the base.
* **The variance-stabilizing transform (generalized Anscombe)**, which darktable
  applies. More correct in theory: it makes the noise uniform, which allows a
  constant threshold and a clean analysis. In practice it adds two non-linear
  transformations per render and a bias to correct on the inverse, for a deviation
  from the per-pixel threshold that stays small next to §5's uncertainty on the
  white level. The per-pixel threshold also keeps ADR 0046's operator literally
  intact, which makes this version legible next to the previous one.
* **Materializing the coefficients in the revision.** It would make the render
  pure from `settings_json` — and would make a preset dependent on the body it was
  created on, which would break [`presets.md`](../presets.md) for zero gain: the
  file is necessary to the render anyway, and it carries its own EXIF.
* **Keeping ranks 170 and 180.** It would have avoided any move — and applied a
  model measured on sensor counts to an image that had been through exposure, the
  tone curve and clarity. A correct profile in the wrong place is a wrong profile.
* **An in-house table à la `camconst`**, measured body by body as RawTherapee
  does for white levels. The same dead end as the first alternative, plus a
  database to maintain alone.
* **Fixing `v2` in place.** Forbidden by ADR 0042 §1, and that is also what makes
  the present ADR inexpensive.
