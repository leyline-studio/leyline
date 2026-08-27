# ADR 0067 — AVIF encoding speed is a setting, and its default was the wrong one

**Status:** Accepted — 2026-08

## Context

`leyline-export` encodes AVIF with `ravif`, and passes it a constant:

```rust
ravif::Encoder::new()
    .with_quality(f32::from(settings.quality))
    .with_speed(6)
```

That `6` is the project's only encoder setting nobody can see or change. It was
never chosen: it is an example value, arrived with the code around it. And yet
[survey B2](../measured-findings.md) showed that AVIF costs **25× JPEG** at
equal size — it is the format where an encoder dial weighs the most, and the
only one where it is hidden.

### What the dial actually does

The complete export path — `leyline export --format avif`, decoding, rendering
and encoding included — on a photo from the real corpus, 18.0 Mpx, quality 90:

| `avif_speed` | Time | CPU | File |
|---|---|---|---|
| 4 | 12.03 s | 97.6 s | 1,562 kB |
| **6 — today's constant** | **8.61 s** | **52.5 s** | **1,616 kB** |
| 8 | 8.29 s | 52.2 s | 1,620 kB |
| 9 | 6.24 s | 24.8 s | 1,637 kB |
| 10 | 2.26 s | 10.9 s | 1,803 kB |

Two things come out of it.

**1. Point 6 has nothing particular to defend.** Moving to 9 gives back **28 %
of the time and 53 % of the CPU for 1.3 % of the weight**. No reasonable
judgement prefers 6 to that bargain; and the constant was never chosen to
settle one anyway.

**2. The right compromise depends on the photo.** The same survey, encoding in
isolation (pixels already through a JPEG, hence easier to compress), reverses
the sign: at 9 the file was **smaller** than at 6 (1,517 kB against 1,718 at
18 Mpx; 1,973 against 2,008 at 12 Mpx). And the price of speed 10 varies from
0 to 14 % of the weight depending on the image — B2's table, measured on
another photo, gives +14 %, the one above gives +10 %.

That is precisely the argument against a constant: someone exporting a check
batch wants speed 10, someone preparing an online gallery does not, and the
right setting depends on top of that on what is in the photo. A number written
into the code decides for everyone.

### What the dial does not do

It does not change the image. The quality aimed at stays that of
`with_quality`; the speed decides only the encoder's search effort — hence the
weight obtained at that quality, not the image. Verified rather than asserted —
the PSNR between each speed's result and speed 6's: **46.3 to 50.8 dB**, above
the threshold of discernment, and speed 10 is no further from 6 than speed 1 is
(49.0 dB). Against the source, the six speeds are within 0.001 dB of one
another.

That is what places this decision outside `pipeline.md` §5.1's scope: the
promise bears on **the rendering** — the pixels the pipeline produces, frozen
by stage versions. The encoder is downstream, and receives those pixels already
computed. No stage and no stage version is at issue here.

## Decision

**AVIF encoding speed becomes a field of `ExportSettings`, and its default
moves from 6 to 9.**

### 1. The field

`ExportSettings.avif_speed`, an integer from 1 to 10, refused outside that
interval by `validate()` as `quality` already is. Ignored by every other
format, exactly as `quality` is by the lossless formats — the structure
describes an export recipe, not a codec.

It is **always serialized**, including in a JPEG preset. That is deliberate: a
preset written today pins its speed, so it will produce the same file when the
default moves again. The noise in the JSON is the price of that property, and
`format` as well as `quality` are already written unconditionally.

### 2. The default: 9, not 10

9 is the best bargain one can impose on someone who asked for nothing:
**1.3 % more weight, 28 % less time and 53 % less CPU**. A weight that moves by
a hundredth changes nobody's decision; an export half as expensive in CPU
does.

10 goes much faster still (~4× the default), but its cost in weight varies from
0 to 14 % depending on the image, and AVIF is chosen *for* its compactness —
someone accepting 25× a JPEG's time does so in order to obtain a small file.
Taking 14 % of that benefit back by default, without their asking, would decide
for them. §1's field gives the decision back: 10 is one word away.

### 3. What it changes for already-saved presets

An existing AVIF preset does not carry the field, so it receives 9 on read:
**its next exports will be different files** — faster, roughly the same weight,
visually identical (§Context). That is accepted: an export is a derived
artefact, regenerable at will, and nothing in the catalog depends on it. No
revision, no photo and no develop setting is touched.

## Consequences

* An AVIF batch exports **~28 % faster for half the CPU** without anyone
  changing a setting, and ~4× faster for whoever puts the dial at 10.
* The field travels through all three clients: `--avif-speed` in the CLI
  (export and preset creation), a field in Studio's export dialog, and the SDK
  by plain re-export.
* `docs/catalog.md` §27 gains the field's description in `settings_json`.
* **The project's only hidden encoder setting disappears.** Should another
  appear, the question to ask is this one: is it a judgement call the user
  might want to settle differently?

## Alternatives rejected

* **Changing the default alone, without exposing the field.** It would correct
  the most visible point and leave the real flaw in place: a time/weight
  judgement, which depends on the use and on the image, decided once and for
  all in the code.
* **A default of 10.** Rejected in §2: it takes back a variable share of the
  benefit one comes looking for in choosing AVIF, without the user asking.
* **A generic `speed` field for every format.** No other encoder in the project
  has the notion; a field four formats in five ignore would promise a setting
  that does not exist.
* **Inferring the speed from the image's size** (fast on large ones, slow on
  small ones). An unwritten heuristic that would take the decision back from
  the user in another form, and a harder one to predict.
