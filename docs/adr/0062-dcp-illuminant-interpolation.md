# ADR 0062 — Interpolating a DCP profile's calibration illuminants

**Status:** Accepted — 2026-08
**Followed by:** `camera_profile::v2`, which this ADR creates, is no longer the
current version: [ADR 0063](0063-dcp-tables.md) applies the profile's tables
(`HueSatMap`, `LookTable`) in `v3`. The mired interpolation decided here is
unchanged, and feeds those tables.

**Amends:** [ADR 0035](0035-camera-profile-dcp.md) (the simplification in
§Decision), [ADR 0037](0037-dcp-parsing-dependency.md)

## Context

A DCP profile is calibrated under **two illuminants**: typically `Standard
Light A` (tungsten, 2850 K) and `D65` (daylight, 6500 K). It therefore carries
two sets of matrices, and the DNG spec says to **interpolate between them
according to the scene's temperature** — a photo under tungsten must be
developed with the tungsten calibration.

Leyline **averages the two matrices**, whatever the light. That is a
simplification the `dcp.rs` module has documented since ADR 0035, and which ADR
0035 accepted for want of knowing what it cost.

It was measured on 2026-08-02, on the two real Canon profiles the project has.
The difference between the average and the correct calibration, in linear sRGB
output over `[0, 1]`:

| Sample | Canon 60D | Canon 5D Mark IV |
|---|---|---|
| neutral grey | 0.0000 | 0.0001 |
| light skin | 0.016 | 0.012 |
| sky | 0.032 | 0.019 |
| saturated red | **0.044** | 0.028 |

**The neutral axis is intact** — which is what let the error go unnoticed — but
0.044 is 11 levels out of 255. It shows on a flat area, and it is a systematic
bias, not noise.

Both profiles do declare both illuminants, so the "average" case is the common
case, not an edge case.

## Decision

**A profile keeps both sets of matrices, and the matrix is resolved at render
time, not at parse time.**

### 1. The formula

The DNG spec's, verified against the DNG SDK's reference code as RawTherapee
takes it up (`rtengine/dcp.cc`):

```
mix = (1/T − 1/T₂) / (1/T₁ − 1/T₂),  clamped to [0, 1]
M   = mix · M₁ + (1 − mix) · M₂
```

The interpolation is done on **the inverse of the temperature** — in mireds,
the quantity in which a colour difference is perceptually linear. Interpolating
over kelvins would give a wrong result in the middle of the interval, and that
is the error one naturally makes.

The illuminants' temperatures come from the DNG SDK's table: illuminant 17
(`Standard Light A`) → **2850 K**, 21 (`D65`) → **6500 K**. Those are the
reference code's values, not the exact physical ones (2856 K for illuminant A):
they are the ones needed, since the aim is to produce the same blend as the
reference.

### 2. Where the scene's temperature comes from

That is where Leyline has a shortcut other implementations do not, and it must
be said: **our `settings.white_balance` already carries a temperature in
kelvins**. When the revision names one, that is it, with no detour and no
approximation.

That leaves the **"as shot"** case (`white_balance: None`), which is the state
of every freshly imported photo — hence the majority case, not an edge case.
There the temperature is derived from the body's multipliers that
`SourceColor::Camera { multipliers }` already carries, along the path the spec
describes: camera neutral → XYZ → *xy* coordinates → temperature, the last step
by Robertson's isotherm table (31 entries, published colorimetry, taken as it
is from the DNG SDK).

The neutral → *xy* conversion is iterative: it needs the matrix to find the
white point, and the white point to choose the matrix. A few passes suffice,
and the DNG SDK caps their number — we do the same.

**When nothing is available** — neither a named temperature nor multipliers —
we fall back on D65 rather than on the average, and we document it. A
calibration at one end of the interval is a defensible choice; an average is
not one, as it corresponds to no real light.

### 3. `camera_profile::v2`

The pixels change, so it is a new stage version. Existing revisions cite `v1`
and go on rendering exactly as today (`docs/pipeline.md` §5.1), averaging
included.

`DcpProfile` stops exposing a single matrix resolved at read time; it carries
what the file contains, plus a method that resolves for a given temperature.
That is a change of shape, not only of value: the matrix is no longer a
property of the profile, it is a property of the (profile, light) pair.

### 4. The stage cache's trap

`camera_profile` depended on the `camera_profile` key alone. It now depends
**also on `white_balance`**, and its `Stage::reads` must say so
([ADR 0041](0041-interactive-preview-rendering.md) §3). Without that, changing
the temperature would reuse a checkpoint computed under the old one, and would
put wrong pixels on screen — silently.

That is exactly the kind of omission `reads` makes possible, and the reason its
documentation says a missing key is a correctness bug and not a performance
one.

## Consequences

* One more stage version, hence one more entry in the reference renders, with
  the previous ones unchanged.
* The rendering of a tungsten photo with a DCP profile changes — for the
  better, and only after reprocessing to `camera_profile::v2`.
* Robertson's table enters `leyline-color`. Thirty-one lines of published data,
  with no new dependency.
* ADR 0035's "experimental" caveat does not move: it bears on agreement with
  Adobe's *rendering*, which nothing here verifies.

## Alternatives rejected

* **Keeping the average.** Measured, it costs up to 11 levels out of 255 on
  saturated colours, and corresponds to no physical light.
* **Always taking the nearest illuminant** without interpolating: it avoids
  Robertson's table, but it makes the rendering jump from one profile to the
  other at a threshold crossing, when the temperature slider is continuous.
* **Resolving at parse time with the revision's temperature**, keeping a single
  matrix: it looks simpler, but it forces the file to be re-parsed on every
  move of the white-balance slider, and makes a "profile" object depend on a
  photo. The shape would sit poorly with the meaning.
* **Using illuminant A's exact temperature (2856 K)** rather than the DNG SDK's
  2850 K: more physically correct, and wrong for what we are after —
  reproducing the reference's blend.
