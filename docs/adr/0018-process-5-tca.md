# ADR 0018 — Process version 5: transverse chromatic aberration (TCA)

**Status:** Accepted — 2026-07

## Context

ADR 0016 (distortion) and ADR 0017 (vignetting) left transverse chromatic
aberration (TCA) explicitly out of scope. `lensfun::Modifier` computes it
through `enable_tca_correction` + `apply_subpixel_distortion`: a radial
position gain *per channel* (red, green and blue do not have exactly the same
magnification through the lens, hence the colour fringes near the edges of
the frame). Wiring it up changes the pixels produced, and therefore demands a
new process version (§3.3).

## Decision

The engine introduces `process: 5`, defined in its own frozen module
(`process5.rs`), identical to `process 4` but for one difference: the lens
correction step also corrects TCA, using the same profile already matched for
distortion.

The details:

1. `leyline_lens::Correction` (already used for distortion) also calls
   `enable_tca_correction` at construction, and exposes
   `tca_row(y, width) -> Vec<[(f32, f32); 3]>` — the three source coordinates
   (one per channel) for every pixel of the row, through
   `Modifier::apply_subpixel_distortion`. Unlike vignetting, the meaning of
   Lensfun's `reverse` is the same for distortion and TCA (`rescale_tca`
   passes `self.reverse` through unchanged): the two corrections therefore
   share the same `Modifier`/`Correction` instance, and a single profile
   lookup.
2. `process5.rs` applies TCA as an **independent second geometric pass**,
   right after the distortion pass: every channel of every output pixel is
   resampled separately at its own source coordinate, from the buffer
   *already* corrected for its distortion — **not** fused into a single
   combined distortion+TCA remapping. That is an accepted simplification (see
   "Alternatives rejected"): the crate's `apply_subpixel_distortion` computes
   only the TCA offset, without composing distortion into it; manually
   composing the two coordinate transforms into one pass is not attempted in
   this version.
3. No TCA calibration at that focal length (or no profile matched at all)
   leaves the image exactly as `process 4` would have rendered it — the same
   fallback guarantee as distortion and vignetting.

`CURRENT_PROCESS` moves to 5: new revisions write `process: 5`. Existing
revisions declaring `process: 1` through `4` go on being rendered by their
respective modules, unchanged forever.

## Consequences

* `process5.rs` duplicates the unchanged operators of `process 4` (the same
  choice as ADR 0013/0016/0017). `undistort` is split from the construction
  of `Correction` (now done once in `develop`) so that `correct_tca` reuses
  the same instance without rebuilding a `Modifier`.
* Two successive bilinear resamplings (distortion, then TCA) instead of one
  combined pass: a minor quality cost (double interpolation on the few pixels
  where both corrections are simultaneously active) against an implementation
  far simpler and safer than a manual composition of the coordinate
  transforms.
* A test with a real profile that has distortion data but no TCA (`Canon EF
  17-35mm f/2.8L USM`, from `lensfun`'s bundled database) verifies bit-exact
  parity with `process 4`: the TCA pass really is a no-op with no data. A
  second test with the Canon EF 16-35mm f/2.8L II USM profile (which has TCA
  data at 20mm) verifies that the correction changes the rendering beyond
  what distortion and vignetting already produce.

## Alternatives rejected

* **Composing distortion and TCA into a single resampling pass**: the more
  rigorous approach, but the crate's `apply_subpixel_distortion` does only
  the TCA part of the computation — fusing them would require manually
  recomposing the normalized coordinate transforms of both passes (mod_coord
  + mod_subpix), work that is non-trivial and risky to get right without
  Lensfun reference images at hand. Deferred to a possible future process
  version should the quality need justify it.
* **Ignoring TCA for good**: the V1 spec says only "Lens correction
  (Lensfun)" with no detail — distortion alone would have been enough to tick
  the box, but Lensfun exposes TCA and vignetting just as easily once the
  profile is matched; leaving them aside would have been a choice of
  laziness, not of engineering.
