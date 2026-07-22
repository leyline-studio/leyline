# leyline-export

> Image export: encode rendered pixels to files.

## What it does

Encodes finished 8-bit RGB pixels to JPEG, TIFF, lossless WebP, AVIF, and
lossless PNG (`docs/specification.md`, `docs/engine-api.md` §12). Nothing
more — rendering, scaling and catalog bookkeeping belong to the engine.

## Why it's built this way

`ExportSettings` doubles as the `settings_json` of export presets
(`docs/catalog.md` §27): a preset is exactly a named, stored instance of
what this crate consumes, rather than a separate concept the catalog and
the exporter both have to agree on independently.

Pixels arrive already in sRGB (`docs/adr/0015-color-management-srgb.md`).
JPEG, PNG and TIFF get an explicit sRGB ICC profile embedded via
`leyline_color::srgb_icc_profile`, so color-managed viewers render them
correctly instead of relying on the "assume sRGB" convention that trips up
some viewers.

## See also

* `docs/engine-api.md` §12
* `docs/catalog.md` §27 — export presets
* `docs/adr/0025-unified-export-request.md`
