# leyline-preview

> Preview and thumbnail cache management.

## What it does

Manages previews as plain PNG files under the library's `Cache/`
directory (`docs/catalog.md` §21). The catalog stores only their
*metadata* (§19) — this crate never touches SQLite.

## Why it's built this way

Because the cache holds no data the engine can't regenerate, **the whole
cache can be deleted at any time without losing anything** — a direct
consequence of the non-destructive constraint applied to derived data,
not just to the original RAW. This crate is internal to the engine, which
maps its `PreviewError` onto `leyline_core::LeylineError` once it knows
which asset was involved, so callers outside the engine never see a
preview-specific error type.

## See also

* `docs/catalog.md` §19, §21 — cache layout, what the catalog stores about
  previews
