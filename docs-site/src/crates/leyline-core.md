# leyline-core

> Shared types, identifiers and errors for the Leyline platform.

## What it does

The innermost crate of the workspace — every other crate depends on it,
and it depends on nothing but `serde`. It owns the identifiers
(`AssetId`, `FolderId`, `JobId`, …), [`LeylineError`], and [`Settings`],
the develop-pipeline settings type.

## Why it's built this way

These types are part of the stable SDK contract (`docs/engine-api.md`
§13), so they live at the bottom of the dependency graph where nothing
else can leak into them. Keeping the only external dependency to `serde`
means the crate compiles fast and can be depended on by literally
everything without dragging in RAW/SQLite/GUI toolchains.

## See also

* `docs/engine-api.md` §13 — the SDK contract these types are part of
* `docs/adr/0001-rust.md`, `docs/adr/0008-version-as-library-unit.md`
