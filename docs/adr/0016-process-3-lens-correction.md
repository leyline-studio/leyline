# ADR 0016 — Process version 3: geometric lens correction (Lensfun)

**Status:** Accepted — 2026-07

## Context

`settings_json` has declared `lens_correction` since schema 1
(`docs/pipeline.md` §3.2), and `docs/pipeline.md` §3.1 places lens correction
at the head of the render pipeline, ahead of white balance. Neither
`process 1` nor `process 2` renders it: `enabled: true` produces the same
result there as `false` (ADR 0013). `docs/specification.md` lists "Lens
correction (Lensfun)" within V1's scope — it had to be wired up.

The `leyline-lens` crate already matches the EXIF camera and lens strings
against the embedded Lensfun profile database (the `lensfun` crate, pure
Rust) and exposes a per-row backward mapping (`Correction::source_row`). What
remained was applying it to the pixels — which changes the rendering, and
therefore demands a new process version (§3.3).

## Decision

The engine introduces `process: 3`, defined in its own frozen module
(`process3.rs`), identical to `process 2` but for one difference: lens
correction is rendered instead of being a dead field.

When `lens_correction.enabled` is true and the caller supplies a `LensShot`
(camera make/model, lens make/model where known, focal length in mm — built
from the catalog's `Metadata` by `render::lens_shot`), the engine:

1. looks for a profile through `leyline_lens::find_profile`;
2. with no match (a lens unknown to the database, or no `LensShot` supplied),
   leaves the image untouched — EXIF is *best-effort*, and a correction is
   never guessed;
3. with a match, builds a `leyline_lens::Correction` for the focal length and
   the image's dimensions, then resamples every output pixel by bilinear
   interpolation at the source coordinate `Correction::source_row` gives
   (backward remapping, the same family as the rotation in
   `process2.rs`/`process3.rs`). Samples whose source falls outside the frame
   stay black — the same convention as rotation, there being no alpha channel
   in the working buffer.

Only geometric distortion is corrected in V1. Vignetting and transverse
chromatic aberration (TCA), which `lensfun::Modifier` can also compute, stay
outside `process 3`'s scope: a deliberate scope cut, not a limit of Lensfun.
Only the `"auto"` profile (matching by metadata) is handled —
`lens_correction.profile` has no other value used in V1.

`CURRENT_PROCESS` moves to 3: new revisions write `process: 3`. Existing
revisions declaring `process: 1` or `process: 2` go on being rendered by
their respective modules, unchanged forever.

## Consequences

* `render()` gains a `shot: Option<&LensShot>` parameter, ignored by
  `process1`/`process2`. The two real call sites (`preview::preview`,
  `export::export_version`) build it from `catalog.metadata(asset)` through
  `render::lens_shot`. Test and benchmark calls, with no synthetic EXIF
  available, pass `None`.
* `process3.rs` duplicates the unchanged operators of `process 2` rather than
  sharing them (the same choice as ADR 0013): the freezing of each process
  version stays guaranteed even if a future `process 4` changes a different
  operator.
* Two bilinear coordinate conventions coexist in `process3.rs`: that of
  `rotate`/`crop` (centres at `n + 0.5`, a choice of Leyline's own) and that
  of `lens_bilinear` (centres at integer coordinates, Lensfun's convention) —
  they are not interchangeable, and `lens_bilinear` is a dedicated sampler
  rather than an incorrect reuse of `bilinear`.
* One test bounds the rendering to `process 2`'s when `lens_correction` is
  off (bit-exact, the new step being a no-op), and an integration test with a
  real profile from the embedded database (Canon EOS 5D Mark III + EF 16-35mm
  f/2.8L II USM, already used in `leyline-lens`'s tests) verifies that the
  correction actually moves pixels.

## Alternatives rejected

* **Correcting distortion without a new process version, treating it as
  pre-processing outside the contract**: contradicts §3.3 — any step that
  changes the pixels an existing revision produces must be a new version,
  with no exception for its rank in the pipeline.
* **Vignetting and TCA in the same pass**: Lensfun exposes them
  (`apply_color_modification_*`, `apply_subpixel_distortion`), but TCA needs
  per-channel resampling (three coordinate maps instead of one) and
  vignetting needs a choice of computation space (linear or gamma) that is
  not trivial to validate without Lensfun reference images at hand —
  deferred to a later process version rather than approximated.
