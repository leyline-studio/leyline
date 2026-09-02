# ADR 0115 — The colour a file says it is

**Status:** Accepted — 2026-09

## Context

Every non-RAW file Leyline decodes is treated as sRGB. `source::color()`
returns `SourceColor::Srgb` for a JPEG, a PNG, a TIFF and — since
[ADR 0114](0114-heif-reading.md) — a HEIC, and `input` decodes the sRGB
transfer function and rotates sRGB primaries into the working space.

The files say otherwise. A phone writes **Display P3**, in a JPEG as readily
as in a HEIC; a camera's in-body JPEG is often **Adobe RGB**; an editor's
export carries whatever it was working in. Each of them says so, in an
embedded ICC profile or — for HEIF — in the container's `nclx` box. Leyline
reads neither, so a photograph tagged P3 is rendered as though its numbers
meant sRGB: the primaries are assumed narrower than they are, and every
saturated colour lands in the wrong place. Nothing warns anybody, because
nothing looks broken — it looks slightly dull.

ADR 0114 named this and deliberately did **not** fix it for HEIF alone: the
defect is the non-RAW path's, it has been there since V1, and fixing it for
the newest format would have left two rules decided by which format arrived
last. This ADR fixes it for all of them at once.

## Decision

### 1. The source's colour stops being assumed and starts being read

`SourceColor` gains a third case: a file that **says** what it is, carrying
the two things a colour space is — where its primaries sit, and how its
numbers encode light.

```rust
pub enum SourceColor {
    Camera { .. },              // unchanged
    Srgb,                       // unchanged: a file that says nothing
    Tagged { to_xyz: Matrix3, transfer: Transfer },
}
```

`Srgb` keeps its meaning and its place: an untagged file is not a mystery, it
is sRGB by the convention every decoder in the world applies. What changes is
that a *tagged* file is no longer flattened onto it.

### 2. One conversion, straight into the working space

The rotation goes **source primaries → the working space**, in one matrix,
never source → sRGB → working space.

This is the load-bearing point. sRGB's gamut is smaller than Display P3's;
passing through it clips every colour outside it, permanently, before the
pipeline that was built to hold them ever sees them. [ADR 0044](0044-linear-wide-gamut-working-space.md)
chose an unbounded Rec. 2020 space precisely so a sensor's colours survive to
the end — routing a P3 file through sRGB on the way in would throw away, at
the first step, exactly what that decision protects.

The matrix is built the way ADR 0044 already builds one, with a Bradford
adaptation from the profile's D50 connection space to the working space's
D65 white. A file that carries `nclx` needs no adaptation at all: its
primaries come with their own white point.

### 3. We read the profile ourselves; the render stays free of LittleCMS

An RGB ICC profile that a camera, a phone or an editor writes is a *matrix/TRC*
profile: three primary colorants and one tone curve. Those are read here —
`rXYZ`/`gXYZ`/`bXYZ` and `rTRC` — and turned into the matrix and transfer
above. LittleCMS stays exactly where it is, at the **output** (export, soft
proofing), and does not enter the develop pipeline.

That boundary is a decision, not an omission. [ADR 0086](0086-decoder-in-the-promise.md)
had to make LibRaw part of `pipeline.md` §5.1's promise — the pixels of a
revision depend on the decoder's version, so the decoder is pinned with
everything else. Putting LittleCMS inside a *stage* would make the same true
of it: every golden entry from that day on would depend on an lcms release,
and a colour-engine upgrade would become a rendering change. One such
dependency is the price of reading RAW files; a second one, for a matrix
multiplication we can do ourselves in fifty lines, is not.

What that costs: a profile that is **not** matrix/TRC — a LUT-based one, a
CMYK one — cannot be reduced this way. Those fall back to `Srgb`, which is
exactly today's behaviour for every file, so nothing gets worse; and the
files that carry one are not the files this ADR is about.

### 4. The transfer functions understood, and the one refused

sRGB's curve, a plain gamma, and the ICC's parametric forms (types 0–4) are
read. A sampled `curv` table is read at its own resolution.

**PQ and HLG are refused**, and fall back to `Srgb` rather than being
approximated: they are HDR transfer functions, and HDR is excluded from V1 by
`specification.md` §4. An HDR HEIC rendered through an SDR curve would be
wrong in a way that looks like a bug in the develop panel rather than a
missing feature, so this ADR would rather keep it wrong in the way it is
wrong today, visibly, than half-implement a decision nobody has taken.

### 5. `input::v6`, and what an already-developed photograph does

A new version of the stage every revision pins. At `SourceColor::Camera` and
at `SourceColor::Srgb` it renders **bit for bit** what `v5` renders — the
same code, called rather than copied. Only the new case is new.

A revision written before today keeps `input: 5` and therefore keeps
rendering as it did: **a photograph already developed does not change under
its owner**. Reprocessing (`pipeline.md` §4.5) is how a person asks for the
correction, one photograph or one selection at a time, and it produces a new
revision like any other reprocess.

This is the first time the pinning rule protects a *fix* rather than a
rewrite, and it is worth naming: the rule does not exist to keep old
renderings good, it exists to keep them **stable**. A P3 photograph developed
last month was developed against the wrong colours, and the person who did it
tuned their sliders against what they saw. Changing it silently would undo
their work; offering the reprocess lets them redo it deliberately.

## Consequences

* Every non-RAW format gains correct primaries at once — JPEG, PNG, TIFF and
  HEIF — which is what ADR 0114 §4 said this ADR would be for.
* `input` gains version 6 at rank 0 and the same working space; the golden
  manifest gains one entry per case and **moves none**, and the new entries'
  digests are *identical* to the v5 ones, because every golden case renders a
  camera source. That identity is the proof that the version bump changes
  nothing it was not meant to change.
* `leyline-color` gains an ICC **reader** beside its transforms: primaries,
  white point, tone curve. No new dependency.
* `docs/pipeline.md` §3.3 gains the row, and the "non-RAW sources" paragraph
  stops saying they are all normalised to sRGB.
* An HDR HEIC still renders as though it were SDR. That is unchanged, now
  deliberate, and pointed at the ADR that will have to decide it.

## Alternatives rejected

* **Converting to sRGB at decode**, leaving `input` untouched. The smallest
  change, and it clips the gamut at the first step — §2. It would also have
  hidden the decision inside the decoder, where `pipeline.md` cannot see it:
  the colour of a source is a property the *stage* documents.
* **Using LittleCMS for the source transform.** More correct in the general
  case, and it puts a second external library inside the reproducibility
  promise — §3. Refused for the same reason the pipeline builds its own
  matrices today.
* **Applying the correction to existing revisions.** Rejected in §5: it would
  change photographs people have already developed, against sliders they set
  by eye. The pinning rule is the mechanism that makes a fix opt-in, and this
  is the case it was written for.
* **Guessing P3 for untagged files from a phone's EXIF.** Tempting — an
  iPhone JPEG without a profile is still P3 in practice. Rejected: it would
  make the render depend on a camera-model table, i.e. on metadata that says
  nothing about the pixels, and it would be wrong for every file exported
  from an editor that stripped the profile deliberately.
