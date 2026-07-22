# leyline-sdk

> Stable public API of the Leyline engine.

## What it does

Everything Studio can do, the CLI and any script can do — because they
all call exactly this surface. Re-exports the core types (ids, errors,
`Settings`) from `leyline-core` plus the engine's public operations.

## Why it's built this way

`leyline-engine` stays free to change its internals at every version;
`leyline-sdk` is the semver boundary (`0.x` until V1) — the one crate
whose API is a promise, not an implementation detail. This is what makes
"API before GUI" (`docs/engine-api.md` §1) a checkable rule rather than
an aspiration: if a capability isn't in `leyline-sdk`, no client — not
even Studio — can use it, which forces new engine features to be
designed as a public API surface from the start instead of leaking out
through GUI-specific back doors.

## See also

* `docs/engine-api.md` §1, §13 — API-first principle, the SDK contract
* `docs/adr/0011-engine-api-model.md`
