# leyline-color

> Color management (ICC) for the Leyline engine.

## What it does

V1's scope here is fixed, not general: the render pipeline
(`leyline-engine`'s `process1`/`process2`) already outputs in sRGB — LibRaw
is asked for sRGB output on decode, and the tone step applies the sRGB
transfer function. This crate's only job is to make that assumption
explicit and machine-checkable, by generating the canonical sRGB ICC
profile through LittleCMS (rather than shipping one as a binary blob) for
exporters to embed.

## Why it's built this way

Generating the profile at runtime via LittleCMS, instead of vendoring a
static `.icc` file, keeps the profile's provenance auditable and avoids
another binary asset in the repo. See
`docs/adr/0015-color-management-srgb.md` for why sRGB-only was the right
scope for V1, and `docs/adr/0027-color-management-beyond-srgb.md` /
`0035-camera-profile-dcp.md` for where this expands next (wider gamuts,
camera/DCP profiles).

## See also

* `docs/adr/0015-color-management-srgb.md`
* `docs/adr/0027-color-management-beyond-srgb.md`
* `docs/adr/0035-camera-profile-dcp.md`, `0037-dcp-parsing-dependency.md`
