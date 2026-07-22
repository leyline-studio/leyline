# leyline-raw

> RAW file decoding for the Leyline engine.

## What it does

Decodes RAW files, backed by LibRaw. Decoding is deterministic: identical
file + identical `DecodeParams` → identical pixels, every time
(`docs/pipeline.md` §5) — a precondition for the whole non-destructive
model, since the engine re-decodes from the original RAW on every render
instead of caching a mutated copy.

## Why it's built this way

LibRaw is used exclusively through its **LGPL-2.1 branch** (not the
GPL-incompatible CDDL one) and linked **dynamically** — never statically —
to honor the LGPL's substitution obligation (`docs/adr/0004-libraw.md`).
LibRaw is treated strictly as an implementation detail: no type in this
crate's public API exposes it, so the decoder could be swapped for another
backend (e.g. `rawler`) without touching any other crate in the workspace.

This is also why the Windows installer has to bundle `libraw_r-23.dll` and
its own runtime dependencies next to the `.exe` rather than statically
linking it away — see `packaging/windows/build-nsis.sh`.

## See also

* `docs/adr/0004-libraw.md` — LGPL branch choice, dynamic linking
* `docs/pipeline.md` §5 — determinism contract
