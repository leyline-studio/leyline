# leyline-engine

> Orchestration engine: jobs, events and rendering.

## What it does

This is where the develop renderer lives: `render` turns a decoded RAW
image and a revision's settings into pixels, following the process-version
contract of `docs/pipeline.md` §3.3. Editing goes through `EditSession`,
which owns the settings-coalescence policy of `docs/engine-api.md` §10.1
on top of the catalog's revision mechanics. Jobs and their events (import,
export, thumbnail generation, …) are also orchestrated here.

## Why it's built this way

**Every past process version stays rendable forever.** Each one is frozen
in its own module (`process1`, `process2`, …) rather than mutated in
place — a develop recipe written against process version 3 must render
identically years later, even after process version 6 exists. This is the
reproducibility contract at the heart of the non-destructive model
(`docs/pipeline.md`), and it's why the engine's internals look like a
stack of versioned modules instead of one evolving renderer.

The engine has no GUI dependency at all (see
[Architecture](../architecture.md)) — Studio, the CLI, and the SDK are
just clients of it.

## See also

* `docs/pipeline.md` — process versions, reproducibility contract
* `docs/engine-api.md` — jobs/events model, edit sessions
* `docs/adr/0007-git-develop-model.md`, `0011-engine-api-model.md`,
  `0012-rayon-data-parallelism.md`
* `docs/adr/0016`–`0018`, `0028`–`0036` — the process-version history
  (lens correction, vignetting, TCA, local adjustments, tone curve, HSL,
  clarity/dehaze, print module, …)
