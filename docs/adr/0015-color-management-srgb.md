# ADR 0015 — V1 colour management: pipeline and export frozen in sRGB

**Status:** Accepted — 2026-07
**Followed by:** the *internal* working space it froze has been replaced by
[ADR 0044](0044-linear-wide-gamut-working-space.md) (unbounded linear
Rec. 2020); what concerns the **output** was first widened by
[ADR 0027](0027-color-management-beyond-srgb.md).

## Context

`specification.md` includes "Colour management (LittleCMS)" and ADR 0005 chose LittleCMS for `leyline-color`, without ever saying what V1 concretely does with it. In practice the pipeline is already sRGB from end to end and it works: `leyline-raw` asks LibRaw for 8-bit sRGB output (`crates/leyline-raw/src/lib.rs`), and `process1`/`process2` (`pipeline.md` §3.3, ADR 0013) apply the sRGB transfer function to the tonal rendering. What was missing is the piece that makes that assumption verifiable outside the code: no exported file carries an ICC profile, so a colour-managed viewer has nothing but convention from which to guess the pixels' space.

## Decision

**V1 handles a single space, from decode to export: sRGB.** No working-space selection, no per-camera input profile, no configurable output-space conversion — this is the existing pipeline, documented as a decision rather than as an accident of implementation.

**`leyline-color` exposes a profile, not a library of transforms.** `srgb_icc_profile()` generates LittleCMS's canonical sRGB ICC profile (`lcms2::Profile::new_srgb`) once (`OnceLock`) and returns its raw ICC bytes. No `cmsTransform`, no device-profile handling: what LittleCMS brings in V1 is a correct reference profile rather than a hand-rolled one hard-coded in the binary, and nothing more.

**`leyline-export` embeds that profile in the formats that support it.** JPEG (the APP2 `ICC_PROFILE` segment, via `jpeg-encoder`), PNG (the `iCCP` chunk, via `png`) and TIFF (the `ICCProfile` tag 34675, via `tiff`) carry it natively. WebP (`image-webp`) and AVIF (`ravif`) expose no ICC-embedding API in their current versions: they come out without a profile, which is the web's accepted convention for those formats (implicit sRGB).

**Linking LittleCMS.** The `lcms2` crate (MIT) embeds `lcms2-sys`, which links dynamically to `liblcms2` if `pkg-config` finds it and otherwise falls back on a vendored build via `cc` — LittleCMS being MIT (unlike LibRaw, ADR 0004), neither option creates a dynamic-linking obligation.

## Consequences

* The implicit contract "everything is sRGB" becomes a tested fact: `leyline-color` checks that the generated profile has a valid ICC header and is deterministic; `leyline-export` checks that the profile's bytes really do end up in the encoded JPEG/PNG/TIFF files.
* A colour-managed viewer (a browser, macOS Preview, and so on) displays Leyline's JPEG/PNG/TIFF correctly even on a wide-gamut screen, without depending on the "no profile means sRGB" convention.
* No change to the pixel format nor to `process1`/`process2`: ADR 0012 (parallelism, bit-for-bit rendering) is not engaged, this being only a metadata embedding at export.
* A future, wider working space (ProPhoto or Adobe RGB internally) would remain a structural change in its own right — this ADR neither prepares it nor rules it out.

## Alternatives rejected

* **A static ICC profile embedded in the binary**: it would have avoided depending on LittleCMS for this one use, but would have reintroduced exactly what ADR 0005 rejected ("profiles of our own") and a file one has to trust without being able to regenerate it; generating it through LittleCMS costs a handful of lines and documents that the chosen library serves a purpose from V1 onwards.
* **Waiting for lens correction and shipping the Lensfun and LittleCMS features together**: lens correction (`leyline-lens`) additionally demands lens and body metadata at import (EXIF `LensModel`, not extracted today) and a new process version (ADR 0012 forbids changing the per-sample order of operations of an existing process) — a distinctly wider scope, to be handled in a separate ADR.
* **A complete ICC transform (a device input profile → sRGB through `cmsTransform`)**: LibRaw already produces 8-bit sRGB directly; adding an ICC conversion on top would duplicate work already done, with no measurable benefit for V1.
