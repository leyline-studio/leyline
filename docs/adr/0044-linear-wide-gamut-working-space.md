# ADR 0044 — A working buffer in linear light and wide gamut: unbounded Rec. 2020

**Status:** Accepted — 2026-07 — **applied**
**Completes:** [ADR 0015](0015-color-management-srgb.md) (sRGB end to end) and
[ADR 0027](0027-color-management-beyond-srgb.md) (widening at the output only)
**Builds on:** [ADR 0042](0042-versioned-stage-pipeline.md) and
[ADR 0043](0043-collapse-prerelease-render-history.md)

## Context

The engine's working buffer is described at the head of
`crates/leyline-engine/src/pixels.rs`:

> Interleaved RGB, `f32` samples, **sRGB gamma-encoded, clipped to [0, 1]**.

That is the invariant `stages.rs`'s twenty stages assume, and it costs three
distinct things — which must be separated, because they have neither the same
gravity nor the same remedy.

**1. The gamut is lost at rank 10, definitively.** `camera_profile::v1`
converts the sensor into linear sRGB through the DCP matrix and then
re-gamma-encodes (`lookup(to_srgb, …)`). A sensor sees well beyond sRGB: deep
blues, saturated foliage greens, fire reds. Those colours are clipped before the
first user setting runs, and no later setting can recover them. Without a DCP
profile, it is LibRaw that does the same clipping (`output_color = 1`).

**2. The highlight headroom is thrown away at exposure.** `gains::v1` converts
to linear light, multiplies, **`clamp(0.0, 1.0)`**, converts back. So +1 EV
followed by −1 EV does not return the original image: what went above white on
the first setting no longer exists on the second. A 14-bit RAW carries several
stops above rendering white; the pipeline destroys them at its fourth stage.

**3. The heavy mathematics runs on gamma-encoded values.** Contrast,
saturation, HSL, and above all everything that *blends* pixels — the Gaussian
blur of clarity, of texture, of sharpening, of noise reduction — operate on a
non-linear signal. Blending two encoded values does not give the blend of the
corresponding lights: that is the origin of the halos and hue drifts in
transitions. The clearest symptom is `pixels::luma()`, which applies Rec. 709
coefficients — defined in linear light — to encoded samples.

**What has changed since ADR 0027.** That ADR explicitly rejected internal
widening: "it would impose a new process version […] for a benefit that neither
V1 nor the items of §7/§8 ask for". Two premises of that reasoning no longer
hold. The benefit is now asked for — it is the only limit the project review
classes as "very high" in complexity because it touches the whole engine. And
the cost has been divided: ADR 0042 removed the complete copying of the
pipeline, so a new stage version is paid for in dozens of lines.

**What ADR 0042 never had to face.** Every evolution shipped so far was local to
one operator. A change of working space is not: it invalidates the input
assumption of all twenty stages at once. The "one frozen version per operator"
model must therefore answer a question it had not asked itself: **what does a
revision citing `gains::v2` (linear) and `hsl::v1` (gamma) mean?** §4 answers
it.

**An existing contract hole this ADR closes in passing.** The decoder's
configuration changes the pixels — `DecodeParams::camera_native` switches LibRaw
between its built-in sRGB conversion and a linear sensor output. It is decided
in four places (`preview.rs`, `export.rs`, `print.rs`) by the same expression
`camera_native: camera_profile.is_some()`, and is **pinned in no `stages`
map**. A revision therefore does not fully describe its rendering today.

## Decision

### 1. The working space becomes Rec. 2020, in linear light, D65 white point

The choice turns on three criteria, and Rec. 2020 is the only one that loses on
none:

* **White point.** D65, like sRGB, Display P3 and Adobe RGB. No chromatic
  adaptation therefore has to be inserted between the stages or at the output —
  whereas ProPhoto (D50) and ACEScg (D60) would impose one, in a place where
  every extra transform is an opportunity to lose reproducibility.
* **Real primaries.** Rec. 2020 is defined on real monochromatic wavelengths;
  ProPhoto draws ~13 % of its volume from imaginary primaries, where
  "saturating" has no physical meaning and where per-channel operators behave
  badly.
* **Coverage.** Rec. 2020 amply encloses the gamut of common photographic
  sensors, which is exactly the need — a still larger space adds nothing but
  empty room.

XYZ is rejected separately: per-channel operators (white balance, saturation,
HSL) make no sense in it, and one would have to convert both ways at every
stage.

### 2. The buffer stops being bounded above

`Pixels`'s new contract:

> Interleaved RGB, `f32` samples, **Rec. 2020 in linear light, values ≥ 0 with
> no ceiling**.

The floor stays: a negative value (a colour outside the sensor's gamut after the
matrix) is clipped to 0. The ceiling disappears — it was what destroyed the
highlight headroom, and keeping it would have left two thirds of the problem in
place.

That decision has a precise price, which must be named rather than discovered at
implementation time: **every stage must define its behaviour above 1**, and that
does not transpose mechanically. Three cases are already identified:

* `hsl::v1` goes through an RGB↔HSL conversion whose lightness `l` assumes
  [0, 1];
* `dehaze::v1` estimates a *dark channel* normalized on the same assumption;
* `whites_blacks::v1` remaps the ends of a range that no longer has an upper
  end.

Each of those operators is **re-derived**, not ported.

### 3. Two always-active stages frame the pipeline

An unbounded linear buffer is neither what the decoder produces nor what a
screen accepts. Both conversions become stages in their own right, versioned and
frozen like the rest:

* **`input`, rank 0.** Carries the decoder's configuration (linear, not
  gamma-encoded output) *and* the matrix to linear Rec. 2020: the DCP matrix
  when a profile is active (ADR 0035), the decoder's otherwise. It is that
  stage that closes the contract hole noted above — the `camera_native` choice
  stops being a caller's decision and becomes a pinned property of the
  rendering.
* **`output_rendering`, rank 900.** Brings the unbounded buffer back to a
  display signal in [0, 1]: a parametric shoulder on the highlights, then
  encoding into the output space. Soft proofing and ADR 0027's ICC conversion
  apply **afterwards**, unchanged.

Those two stages are the only ones whose `active` is always true. That is an
explicit extension of ADR 0042 §2 ("a neutral stage does not run and does not
appear"): they have no neutral value, since there is no rendering without an
input and an output. Their mandatory presence in the `stages` map is precisely
what makes a revision self-describing as to its working space, with no global
counter.

`output_rendering` exposes **one** user setting, `highlight_rolloff` (0–100): 0
clips hard at white, and rising values lengthen a shoulder above a fixed knee.
The default value is set once and frozen with the stage version — it is a
rendering default, not an application preference. The output ICC profile, for
its part, stays outside `settings_json` (ADR 0027): it describes the
destination, not the revision.

### 4. The buffer's space is a declared property of every stage version

`Version` gains a field describing the space it requires and produces. Three
rules follow:

1. **`plan()` refuses a mixed plan.** A set of stages that do not agree on a
   space fails the render with an explicit error, on the same footing as an
   unknown version (`UnknownStage`, `docs/pipeline.md` §3.4). An incoherent
   pipeline is never rendered "as best we can".
2. **`pin()` chooses the compatible version, not the most recent.** When a
   stage leaves its neutral value on an existing revision, it receives the most
   recent version **within the space that revision already declares**. Editing a
   2026 photo in 2036 therefore never switches its space as a side effect.
3. **Changing space is a reprocessing.** A revision's migration goes through
   the existing mechanism (`docs/pipeline.md` §4.5): it creates a **new**
   revision, and the old one stays renderable identically.

No global versioning axis reappears: the space is not a field of the revision,
it is a readable consequence of the stage versions it cites.

### 5. The transition itself: collapsing in place, not doubling

There will be **no** `v2` of the twenty operators beside twenty `v1`s. The
existing bodies are rewritten in place in the new space, and the reference
renders recaptured — exactly ADR 0043's stance, for exactly its reason:

> Leyline not having been published, those versions committed us to nobody.

No revision in the world cites `gains::v1`: keeping its code would freeze a
rendering nobody ever obtained, and would double the engine's surface to protect
a user who does not exist.

What *is* delivered, however, and in full, is §4's mechanics — space
declaration, refusal of a mixed plan, compatible pinning — exercised by the
two-version fixture stage (`stages/fixture.rs`) that has already played that
role since ADR 0043. After publication this paragraph is dead: a change of space
will then be a new version of every stage concerned, which §4 makes possible and
expensive. **That is the reason to do it now.**

### 6. The output stays 8-bit for the time being

`Rendered` does not change. A 16-bit export, which will become tempting once the
linear buffer is in place, belongs to a separate ADR: it touches
`leyline-export` and the formats, not the working space.

One gain arrives anyway without being asked for: ADR 0027's ICC transform now
starts from a linear float instead of already-quantized 8-bit sRGB pixels. The
same public surface, a better conversion.

### 7. Migration order: the mechanics first, the space next

> **Done, in two slices.** Step 1 (commit `23f1252`): the framing stages, the
> space declaration and the refusal of a mixed plan, in the then-current space —
> the fifteen reference fingerprints did not move, compared one by one. Step 2:
> the switch itself, with §7.2's verifications recorded in the consequences
> below.

Like ADR 0042 §7, nothing moves without proof — but the proof is not the same
here, and it must be said plainly: **the rendering is going to change.**
Bit-for-bit equality is therefore a criterion for the first half of the work
only.

1. **A step with proven equality.** Introduce `input`, `output_rendering`, the
   space field and the refusal of a mixed plan **in the current space** (sRGB
   gamma). `tests/golden/renders.json`'s fingerprints do not move: that is what
   establishes that the mechanics are neutral.
2. **A step with a justified difference.** Switch the space, re-derive the
   bodies, recapture the manifest. Every difference is justified on synthetic
   test images *and* on a set of real RAWs, operator by operator — not by a
   global fingerprint that would say only "it changed".
3. **Recalibrate the settings.** A contrast or saturation slider does not have
   the same effect on a linear signal; the curves' constants are adjusted so
   that the same value produces a comparable result, otherwise every shipped
   preset lies.

## Consequences

* **The default rendering changes for every photo.** That is the point to own:
  no image renders as it did. Acceptable only before publication, which is why
  the ADR is written now rather than after.
  What was measured on real files, once the switch was made:
  * a **JPEG imported and re-exported with no retouching comes out to the bit**
    — a maximum difference of zero over 30 Mpx. sRGB → linear → Rec. 2020 →
    operators → Rec. 2020 → sRGB is the exact identity when nothing is set,
    which verifies both matrices and both transfer functions at once;
  * on a RAW with a wide luminance range (an interior plus a sunlit window),
    the clipped pixels of the neutral rendering go from **1.71 % to 0.01 %**:
    the window comes back with its roof and its trees instead of a white shape;
  * the neutral rendering is about 20 % brighter on average. That is not an
    effect of the space but of the transfer function: the output is now encoded
    in sRGB, whereas LibRaw applied its default BT.709 curve to pixels the
    export nevertheless labelled "sRGB". An inconsistency corrected in passing.
* **`pixels::luma()` must be redone twice**: its coefficients will at last apply
  to linear light (which they assumed), and the right coefficients in the new
  space are Rec. 2020's (0.2627 / 0.6780 / 0.0593), not Rec. 709's. Every
  luma-driven operator — highlights/shadows, clarity, texture, sharpening,
  colour grading, noise reduction — therefore changes behaviour, including where
  its formula has not moved.
* **The table round trips disappear.** `gains::v1` and `camera_profile::v1`
  today do two interpolated `lookup`s per sample and per channel, solely to
  enter and leave linear light. In linear space, white balance and exposure
  become a multiplication again. The gain is to be measured with the existing
  criterion benches, but the direction is settled: less work, not more.
* **Memory consumption does not move** — the buffer was already `f32`. Only the
  absence of a ceiling changes what it can hold.
* **`docs/pipeline.md` is amended**: §3.1 (the two framing stages in the chain),
  §3.3 (the stage table, +2 entries, ranks 0 and 900), §3.4 (the refusal of a
  mixed plan joins the failure modes) and §5.1 (the working space is part of
  what a revision pins).
* **ADR 0015 and ADR 0027 receive a cross-reference note.** Neither is annulled:
  ADR 0015 correctly described V1, and ADR 0027's widening *at the output* stays
  exact and useful — it simply becomes the second half of a story of which this
  is the first.
* **The output rendering must choose between recovering and keeping white.** A
  shoulder that brings the headroom below white necessarily moves white itself:
  that is arithmetic, and no formula escapes it. The shoulder retained is a
  quadratic that reaches white at a *finite* input (`2 − knee`) rather than
  asymptotically — an asymptote would mean that nothing in the image is ever
  white again, which on a white subject reads as a grey veil and not as
  recovered highlights. The slider goes from "hard clipping" (the previous
  behaviour) to a shoulder of about 0.6 EV.
* **The main risk is the behaviour above 1**, not the space-change matrix. A
  matrix is verified on three patches; an operator designed for [0, 1] and fed
  4.0 produces the plausible-but-wrong, which no equality test detects. Hence
  §7.2's requirement: justification per operator, on real images.
* **The decoder's contract hole is closed**: `camera_native` stops being an
  expression repeated in four modules and becomes a pinned property of
  `input::v1`.

## Alternatives rejected

* **Staying in gamma-encoded sRGB (the ADR 0015/0027 status quo).** It was the
  correct decision while the pipeline was short and unpublished. It no longer is
  once the blending operators (clarity, texture, dehaze, noise reduction, local
  adjustments) have all shipped: they are the ones that pay for gamma, and they
  are now the majority of the pipeline.
* **Linear but bounded to [0, 1].** It would fix the gamut and the mathematics,
  not the highlight headroom — and would leave the pipeline worse in one regard:
  clipping at 1.0 in linear light is far more brutal than in gamma, because it
  concentrates the useful range where the eye is least sensitive. A half-change
  of this magnitude is not made twice.
* **Linear ProPhoto (ROMM), like Adobe.** A wider gamut, but D50 imposes a
  chromatic adaptation on every output, and its imaginary primaries make
  per-channel operators ill-defined over a real part of the volume. The only
  strong argument is "it is what Lightroom does" — a compatibility argument,
  when nothing here needs to be compatible with Lightroom.
* **ACEScg (AP1).** Excellent for scene-referred work, but D60, no photo tool
  expects it, and its extra gamut serves no photographic sensor.
* **Linear XYZ.** An infinite gamut, but every per-channel operator would demand
  a round trip to an RGB space — that is, exactly the cost being removed from
  gamma.
* **A working-space field at the revision level.** More legible in
  `settings_json`, but it is a global counter in disguise — what ADR 0042 §2
  removed — and the revision stops being self-describing stage by stage. An
  engine could then read a space contradicting the versions cited; deriving the
  space from the versions makes that contradiction impossible to write.
* **Automatically inserting conversion stages between neighbouring spaces.** It
  would make mixed plans legal — at the price of round trips that destroy
  precisely the benefit sought (every conversion to sRGB re-clips the gamut) and
  of a rendering nobody designed or validated. Refusing is more honest than
  rendering as best we can.
* **Keeping the twenty `v1`s in gamma sRGB and shipping twenty `v2`s.** That
  will be the obligation after publication, and §4 makes it practicable. Before
  publication, it means doubling the engine's surface to freeze renderings no
  revision cites (ADR 0043).
* **An implicit, non-adjustable output rendering.** A fixed shoulder in the code
  would technically have sufficed, but the user who has just gained several
  stops of headroom must be able to decide how they come back into the image:
  it is the setting that makes the benefit visible rather than theoretical.
