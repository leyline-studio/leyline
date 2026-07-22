# leyline-lens

> Lens corrections for the Leyline engine, backed by Lensfun.

## What it does

Matches a photo's EXIF camera/lens strings against Lensfun's bundled
profile database and turns the match into a per-pixel backward coordinate
map for distortion, vignetting and TCA correction.

## Why it's built this way

This crate deliberately **does not resample images itself** — it only
produces the correction map; applying it to the actual pixel buffer stays
in the engine. That mirrors how `leyline-raw` decodes without knowing
about the engine's `Pixels` type: each crate around the engine does one
transformation and hands data back, rather than owning a slice of the
render pipeline itself.

Lens correction was rolled out incrementally across three process
versions — distortion, then vignetting, then TCA — each frozen the moment
it shipped, per the engine's "process version stays rendable forever"
rule.

## See also

* `docs/adr/0005-lensfun-littlecms.md`
* `docs/adr/0016-process-3-lens-correction.md`
* `docs/adr/0017-process-4-vignetting.md`
* `docs/adr/0018-process-5-tca.md`
