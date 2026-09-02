# ADR 0111 — Adaptive chromatic aberration: measuring the lens instead of looking it up

**Status:** Accepted — 2026-09

## Context

[ADR 0018](0018-process-5-tca.md) corrects transverse chromatic aberration
from Lensfun's calibration: the database says how far red and blue are
displaced at this focal length, and the stage resamples each channel from its
own coordinate. When it works it is the best answer available, because a
calibration measured on a bench beats anything inferred from one photograph.

It works when the lens is in the database. The code already knows when it is
not — `Correction::tca_matched()` exists precisely to say so, and
`lens::v1::correct_tca` then returns the buffer untouched. That is honest, and
it is also the whole gap: **for an uncalibrated lens Leyline corrects nothing
at all**. Adapted lenses, manual lenses, third-party lenses, anything older or
rarer than the database's coverage — and Lensfun's TCA coverage is far thinner
than its distortion coverage, so this includes lenses whose distortion *is*
corrected.

The fix is not more data. It is to measure the aberration in the photograph
that has it: lateral CA is a per-channel radial magnification, it leaves a
signature on every high-contrast edge away from the centre, and that signature
is measurable to a fraction of a pixel.

## Decision

### 1. Two numbers in `LensCorrection`, and they are ordinary settings

`LensCorrection` gains `tca_red` and `tca_blue`, **percent of the radius**,
range `[-1, 1]`, neutral 0:

```
sample channel c of the output pixel at radius r from the frame centre
at radius r × (1 + tca_c / 100)
```

Percent of the radius and not pixels of displacement, because the number is
stored once and applied at every resolution: a proxy, a preview and a full
export must correct the *same* aberration. A dimensionless ratio does that; a
pixel count would silently mean something different in each.

They are settings like any other: written by an `EditSession`, stored in
`settings_json`, undoable, part of a preset, and re-applied identically
forever. Which is the second decision.

### 2. The measurement never runs inside the render

`Library::estimate_tca` decodes the photograph, measures, and returns two
numbers. **It renders nothing that is kept, and no stage ever calls it.** A
client writes the numbers through an ordinary edit, exactly as
[ADR 0088](0088-auto-tone-and-black-and-white.md)'s Auto writes five tone
values.

This is not a stylistic echo of ADR 0088, it is the only shape that works.
An analysis called from inside the stage would measure the buffer it was
handed — a 1024-pixel proxy for the loupe, the full decode for the export —
and two measurements of the same photograph would not agree to the last
digit. The render would then be a function of the resolution it was asked
for, and `docs/pipeline.md` §5.1 would be false. Measuring once and storing
the result makes the render a pure function of settings again, and it is also
what lets a photographer nudge the number afterwards, or copy it to the
forty other frames from the same lens.

It also keeps the rule [ADR 0084](0084-assisted-culling.md),
[ADR 0088](0088-auto-tone-and-black-and-white.md) and
[ADR 0105](0105-detector-conformance-and-cli.md) share, and that the memory of
this repository states as *an automatic tool is a choice*: never a default,
never an import step, always a press.

### 3. How it is measured

On the **decoded image before any stage runs** — the frame the sensor
produced, which is the frame the aberration lives in and the frame the
correction is applied in. Not on a developed preview: a preview is post-crop
and post-rotation, and a radial magnification about a *cropped* frame's centre
is a different quantity.

And at **full size**, not at the half size everything else in this engine
reaches for when it wants speed: LibRaw's half-size path takes red and blue
from their own sites inside the Bayer cell, which displaces the two channels
against each other by half a cell — by construction, and in the exact quantity
being measured here. A tenth of a second saved for a measurement made on the
artefact instead of the aberration.

The algorithm, entirely deterministic:

1. Sample points on a fixed stride grid, keeping only radii beyond 35 % of the
   corner distance — lateral CA grows with radius, and near the centre there is
   nothing to measure.
2. Keep the strongest radial green gradients (a fixed count, ties broken by
   index, so the same image always yields the same set).
3. At each sample, take a seven-tap profile along the radial direction for
   green and for the channel, normalize both to zero mean and unit norm — the
   channels differ in exposure and in the colour of what they cross — and
   solve one Lucas-Kanade step for the sub-pixel displacement `d`.
4. Fit `d = k·r` through the origin by least squares, twice, dropping samples
   beyond two sigma the second time. `k` is the magnification error, and
   `100·k` is the stored percentage.

Step 3 is the load-bearing one, and it took two wrong turns to write —
neither of which reading the code would have found, and both of which the
synthetic round trip did. Normalizing each profile to zero mean and unit norm
before matching them, the obvious way to neutralize the exposure difference
between two channels, returns **a tenth** of the true displacement; fitting a
gain *and an offset* (`other ≈ a·green + b`) returns four fifths of it. Both
fail for the same reason: over a short window a translation of a locally
straight profile **is** an offset, so any fit free to absorb an offset absorbs
the answer with it. Differentiating first removes the offset outright and
leaves a gain, which a translation cannot hide in. What then carries the signal
is the profile's curvature — which is also why a straight ramp is rejected
rather than believed: it genuinely holds no displacement information.

Fitting through the origin is the model, stated as a constraint rather than
discovered: an aberration that displaces the centre of the frame is not
lateral CA, it is a decentred sensor, and a fit that could express it would
mostly express noise.

**A linear model, and nothing more.** Real lateral CA has a cubic term; a
bench calibration measures it and Lensfun stores it. One photograph does not
carry enough signal to separate the two terms robustly, and a slider a user
can reason about has one number per channel. This is the smallest genuinely
useful thing, and the larger one already exists for the lenses that have it.

### 4. `lens::v2`, and the two corrections compose in one resampling pass

A new version beside the frozen `v1` ([ADR 0042](0042-versioned-stage-pipeline.md) §1),
same rank 20, same working space. At `tca_red = tca_blue = 0`, v2 skips the
manual map entirely and renders **bit for bit** what v1 renders — the property
that made [ADR 0096](0096-sharpening-masking.md)'s bump safe rather than
merely legal.

When both a Lensfun calibration and manual coefficients are present they do
**not** run as two passes. Lensfun gives a source coordinate per channel; the
manual magnification scales *that* coordinate about the frame centre, and the
single existing per-channel resample reads from the composed position. One
interpolation, not two — which matters, because two resamples of the same
pixels is the one thing a correction meant to sharpen colour edges must not
do.

The composition order follows from §3: the manual coefficient was measured in
the source frame, so it applies to the source coordinate.

### 5. The stage runs for the coefficients alone

`lens`'s `active` predicate becomes `enabled || tca_red != 0 || tca_blue != 0`,
and inside, the Lensfun half still runs only when `enabled`. A photographer
with an uncalibrated lens — the case this ADR exists for — can correct its
colour fringing without switching on a distortion correction that has no data
to work from.

The capability rule applies unweakened
(`stage-version-capability-rule`, [ADR 0096](0096-sharpening-masking.md) §3):
a non-zero coefficient on a revision pinning `lens` v1 is **refused by
`validate()`**, naming the version it needs and the reprocessing that gets
there. A slider that quietly does nothing is worse than an error.

### 6. What the measurement says when it has nothing to say

Below a floor of usable samples, `estimate_tca` returns **zero and says how
many samples it found**, rather than a number fitted to noise. A photograph of
fog has no edges to measure, and the honest answer is "not this one" — the
clients print the sample count beside the result so the answer is legible
rather than mysterious.

## Consequences

* `lens` gains version 2 at rank 20; the golden manifest gains a `lens_tca`
  case and the entries the two lens-active cases render through today — and
  moves nothing, because v1 is untouched and v2 at neutral coefficients is v1.
* `docs/pipeline.md` §3.3's table gains the version, and its `settings_json`
  example the two fields.
* Three clients: `leyline develop <v> lens-tca <red> <blue>` and
  `leyline auto-tca`, two sliders and a *Measure* button in Studio's lens
  panel, `TcaEstimate` through the SDK façade.
* **An uncalibrated lens stops being a lens Leyline cannot help.** That is the
  whole point, and it is measurable: `tca_matched()` is false for most adapted
  and third-party glass, and was silently the end of the story. Measured on
  real frames from a Canon 60D and its kit zoom at 18 mm: **+0.034 %, +0.034 %,
  +0.020 %** red on three frames of one scene — about eight tenths of a pixel
  at the corner, and reproducible frame to frame, which is what a lens
  aberration should be. One second per photograph, full decode included.
* Lensfun stays the better answer where it has data, and stays first: nothing
  about this ADR weakens [ADR 0018](0018-process-5-tca.md), and a calibrated
  lens whose owner never touches the sliders renders exactly as before.

## Alternatives rejected

* **Measuring inside the stage, at render time.** Rejected in §2: the
  measurement would depend on the buffer size the render was asked for, so the
  same revision would render differently in the loupe and in the export, and
  §5.1's promise would be false. This is the decision the rest of the design
  follows from.
* **A separate `tca` stage instead of `lens::v2`.** Tempting, because a new
  stage leaves the golden manifest alone ([ADR 0103](0103-red-eye-correction.md)'s
  measured lesson) while a new version does not. Rejected: it is the *same*
  operation as the TCA `lens` already performs, differing only in where the
  coefficients come from, and two stages would resample the same pixels twice
  at adjacent ranks — [ADR 0096](0096-sharpening-masking.md)'s "a parameter
  pretending to be a stage", with an interpolation loss attached.
* **A cubic term, or per-channel free-form radial polynomials.** Rejected in
  §3: one photograph cannot separate the terms robustly, and the honest place
  for a higher-order model is a bench calibration, which is what Lensfun is.
* **Auto-applying the measurement at import.** Rejected, and this repository
  has refused it four times now (ADR 0084 §2, ADR 0088, ADR 0105 §2, and the
  culling wiring): an automatic tool proposes, a photographer presses. A
  measurement written by an import is a correction nobody chose and nobody can
  find afterwards.
* **Fitting the displacement with an intercept.** Rejected in §3: an intercept
  models a decentred sensor, which is not what this stage corrects, and mostly
  absorbs noise from the samples nearest the centre.
