# ADR 0063 — Applying a DCP profile's tables

**Status:** Accepted — 2026-08

**Completes:** [ADR 0035](0035-camera-profile-dcp.md), [ADR 0037](0037-dcp-parsing-dependency.md), [ADR 0062](0062-dcp-illuminant-interpolation.md)

## Context

A DCP profile carries four things. Leyline applies **one**: the matrices. The
other three — `ProfileHueSatMapData`, `ProfileLookTableData`,
`ProfileToneCurve` — have been neither read nor applied since ADR 0035, which
flagged it as a known gap.

Yet they are the ones that carry the *rendering*. The matrix performs a
colorimetrically correct conversion; it is the look table that makes a file
"look as though it came out of Lightroom". While they are missing, comparison
against a reference converter is lost in advance, whatever the quality of the
rest of the pipeline.

Three real profiles were inventoried on 2026-08-02, tags read directly:

| | Canon 60D *(linear)* | Canon 5D IV *(linear)* | Canon 60D *(RawTherapee)* |
|---|---|---|---|
| `HueSatMapDims` | absent | 90×25×1 | 90×30×1 |
| `ProfileToneCurve` | 2 points | 2 points | 8192 points |
| `LookTableDims` | 36×8×16 | 36×8×16 | 90×30×30 |

Two lessons from that inventory, both contrary to what one assumes:

**A "linear" profile is not a matrix-only profile.** It has a complete look
table; what is linear is its *tone curve*, reduced to the two points (0,0) and
(1,1). The identity is literally readable in the tag's four floats.

**The dimensions vary widely from one profile to the next** — from 4,608 to
81,000 entries. Nothing can be hard-coded.

## Decision

**The three tables are read and applied, in the order and the space the DNG
spec imposes, by a new `camera_profile::v3` version.**

### 1. The order, which is not the one you would guess

Verified in the reference code as RawTherapee takes it up
(`rtengine/dcp.cc`, `applyStep1`/`applyStep2`):

```
camera RGB
  → HueSatMap                (early, before the matrix)
  → forward matrix → XYZ(D50)
  → linear ProPhoto RGB
  → LookTable                (in HSV)
  → ProfileToneCurve
  → working space
```

**The look table comes before the tone curve**, not after. That is the reverse
of what the names suggest, and of what I would have written without checking.

**Everything happens in ProPhoto RGB**, not in our Rec. 2020 working space (ADR
0044). The tables are defined against ProPhoto; applying them elsewhere would
give wrong colours while looking as though it worked. So we convert, apply, and
come back.

### 2. The highlights, and the conflict with ADR 0044

The tables are defined over HSV bounded to `[0, 1]`. Our working buffer is
**deliberately unbounded above white** (ADR 0044 §1): that is where a RAW's
highlight headroom lives, and preserving it was that decision's whole point.

Applying a bounded table would crush that headroom. The rule retained is the
reference code's: **the table is computed on the clipped value, and is written
only if the sample was already within `[0, 1]`.** A sample above white passes
through unchanged.

That is a compromise, and it must be named: a highlight does not receive the
profile's "look". The alternative — clipping in order to apply the table —
would lose information the whole pipeline is built to keep.

### 3. Interpolation

A table is a sampled HSV cube: hue × saturation × value, three deltas per entry
(a hue shift, a saturation factor, a value factor). The interpolation is
**trilinear**, with hue **cyclic** — the 360th degree neighbours the 0th, and
treating hue as an open axis produces a visible seam on the reds.

A table at `val = 1` has only one plane: the interpolation degenerates on that
axis, which is the case for the three profiles inventoried and must therefore
work.

### 4. Two tables, two illuminants

`HueSatMapData1` and `HueSatMapData2` correspond to the two calibration
illuminants. They are blended **by the same mired weight** ADR 0062 computes
for the matrices — a profile must not interpolate its matrices under one light
and its tables under another.

### 5. `camera_profile::v3`

The pixels change, hence a new stage version; `v1` (averaging) and `v2`
(interpolation, without tables) stay frozen. A profile with no table at all
renders in `v3` exactly as in `v2` — which the reference renders must show.

## Consequences

* The rendering of a photo with a DCP profile changes markedly, and comes
  closer to what a reference converter produces. That is the point.
* `DcpProfile` grows: up to 81,000 floats per table, with two tables possible.
  A profile is loaded once per render, not per pixel.
* The conversion to ProPhoto and back enters `leyline-color`.
* ADR 0035's "experimental" caveat can be lifted **if** a comparison against a
  reference rendering confirms it — which stays blocked for want of an
  available reference converter (`measured-findings.md` §A1).

## Alternatives rejected

* **Applying only the tone curve**, the simplest of the three: it is the one
  neutralized in linear profiles, and therefore the only one whose absence does
  not show. Doing the opposite of the right choice.
* **Applying the tables in our Rec. 2020 space** without going through
  ProPhoto: it saves two matrices per pixel, and gives wrong colours with the
  appearance of working — the worst failure mode.
* **Clipping to `[0, 1]` in order to apply the tables everywhere**: it would
  make the look's consistency a higher priority than preserving the highlights,
  against the grain of ADR 0044.
* **Nearest-neighbour interpolation** instead of trilinear: visible as banding
  on a sky gradient, for a saving of no consequence at this scale.
