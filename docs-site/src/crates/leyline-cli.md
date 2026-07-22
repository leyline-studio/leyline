# leyline-cli

> Command-line client for the Leyline engine.

## What it does

Exposes import, catalog queries, develop parameters, presets, reprocess
and export entirely through `leyline-sdk`.

## Why it's built this way

The CLI is a **thin client with no logic of its own** — it exists mostly
to prove the "API before GUI" principle of `docs/engine-api.md` §1:
everything Studio does, these commands already do, because both go
through the same `leyline-sdk` surface. If a feature can only be done in
Studio, that's treated as a gap in the SDK, not an acceptable difference
between clients (this was tracked and closed explicitly — see the CLI
parity work referenced in the roadmap).

## See also

* `docs/engine-api.md` §1
* `docs/adr/0011-engine-api-model.md`
