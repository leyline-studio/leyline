# ADR 0017 — Process version 4: vignetting correction (Lensfun)

**Status:** Accepted — 2026-07

## Context

ADR 0016 introduced `process: 3`, which corrects a lens's geometric
distortion through the Lensfun profile matched from EXIF, but explicitly left
vignetting and transverse chromatic aberration (TCA) out of scope.
`leyline-lens` already matches a profile; `lensfun::Modifier` can also
compute a per-pixel vignetting gain (`enable_vignetting_correction` +
`apply_color_modification_f32`) from that same profile, the aperture and a
focus distance. What remained was wiring it up — a change of pixels, hence a
new process version (§3.3).

## Decision

The engine introduces `process: 4`, defined in its own frozen module
(`process4.rs`), identical to `process 3` but for one difference: the lens
correction step devignettes as well as correcting distortion, using the same
already-matched profile.

The details:

1. `LensShot` gains an `aperture_f: Option<f32>` field (the EXIF aperture,
   already extracted by `leyline-raw` — only the wire through to `LensShot`
   was missing), filled by `render::lens_shot` from `Metadata::aperture`.
2. `leyline-lens` exposes `Vignetting`, a second type built with its own
   `lensfun::Modifier` — **not** `Correction`'s: Lensfun's `reverse` flag has
   the opposite meaning for distortion and for vignetting (`true` corrects
   distortion but *simulates* vignetting, per the `lensfun` crate's
   documentation), so the two corrections cannot share one instance.
3. Subject distance is not in the EXIF (no consumer camera records it
   reliably): `leyline-lens` assumes 1000 m, the value Lensfun itself uses as
   the "effectively infinity" bucket — the least wrong choice for non-macro
   photography. Assumed, documented, and never guessed case by case.
4. Vignetting is a physical light falloff that is *multiplicative in linear
   light*: `process4.rs` therefore makes the round trip through the same
   transfer tables as `linear_gains` (white balance/exposure) rather than
   multiplying directly in gamma, unlike a naive implementation that would
   apply the gain to the gamma samples as they are.
5. No known aperture (`aperture_f: None`), or no vignetting calibration for
   that focal length and aperture, leaves the image exactly as `process 3`
   would have rendered it.

`CURRENT_PROCESS` moves to 4: new revisions write `process: 4`. Existing
revisions declaring `process: 1`, `2` or `3` go on being rendered by their
respective modules, unchanged forever.

## Consequences

* `process4.rs` duplicates the unchanged operators of `process 3` (the same
  choice as ADR 0013/0016): the distortion function (`undistort`) is split
  from the profile lookup so that `devignette` reuses the same `Profile`,
  matched once, rather than repeating the lookup twice as a mechanical copy
  of `process3::correct_lens` would have.
* One test verifies bit-exact parity with `process 3` when `lens_correction`
  is off or the aperture is unknown (the new step being a no-op in both
  cases), and a test with the real Canon EOS 5D Mark III + EF 16-35mm f/2.8L
  II USM profile (already used in `leyline-lens` and `process3`) verifies
  that vignetting changes the rendering beyond what distortion alone already
  produces.
* TCA stays out of scope — a further scope cut, not a limit of Lensfun.

## Alternatives rejected

* **Applying the gain directly in gamma**: simpler, but physically wrong —
  Lensfun's vignetting gain is calibrated in linear light; applying it after
  gamma would change the tonal response in a way that depends on the pixel's
  level, not on its position alone.
* **Guessing subject distance from the metering mode or the focal length**:
  no reliable heuristic exists without real EXIF data; 1000 m (Lensfun's own
  convention) is already the most defensible default.
* **Sharing a single `Modifier` between distortion and vignetting**: not
  cleanly possible because of the opposite meaning of `reverse` between the
  two passes (see point 2 above) — inverting the gain by hand (`1.0 / gain`)
  would have worked but reads worse than a second, dedicated `Modifier`.
