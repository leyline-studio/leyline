# ADR 0025 — One export request: `ExportRequest`/`ExportRecipe`

**Status:** Accepted — 2026-07

## Context

Since its first draft, `engine-api.md` §12 documented a target single-request
shape (`ExportRequest { versions, preset, destination }` → one
`Library::export(request) -> Result<JobId>`), marked "still to come": the
surface actually shipped had diverged into five entry points on `Library` —
`export` (one version, synchronous), `export_batch` (an ad-hoc recipe,
synchronous), `export_with_preset` (a stored preset, synchronous),
`export_async` (an ad-hoc recipe, a job), `export_with_preset_async` (a
stored preset, a job). Each synchronous/job pair duplicated the same
validation, preset resolution and per-version looping logic — already
factored once internally by `ADR 0024` (`export_batch_with_preset`) but left
invisible to callers (the CLI, Studio), who had to choose between five method
names along two independent axes (ad-hoc recipe vs. preset; synchronous vs.
job) instead of making one decision.

The initial draft, however, provided only a mandatory `preset:
ExportPresetId` field — no ad-hoc recipe — and a single `export` always
returning a `JobId`. Neither premise matched real use: the CLI (`leyline
export` without `--preset`) and Studio's export dialog ("Web", an unsaved
recipe) need an ad-hoc recipe as often as a stored preset; and a scripted use
(CLI, SDK) wants to export a batch and get the report back directly, without
having to subscribe to events for a one-off call.

## Decision

A single request replaces the five methods:

```rust
pub enum ExportRecipe {
    Adhoc(ExportSettings),
    Preset(ExportPresetId),
}

pub struct ExportRequest {
    pub versions: Vec<VersionId>,
    pub recipe: ExportRecipe,
    pub destination_dir: PathBuf,
}

impl Library {
    pub fn export(&self, request: &ExportRequest,
                  progress: impl FnMut(u64, u64)) -> Result<ExportReport>;
    pub fn export_async(&self, request: ExportRequest) -> JobId;
}
```

`ExportRecipe` replaces the draft's forced `preset: ExportPresetId` field
with a two-variant union — the same "ad-hoc recipe or stored preset" that
`export_batch`/`export_with_preset` already distinguished by method name, now
carried by the type rather than by the caller's choice between two call
sites. An `ExportRecipe::Preset` is resolved once, under a short lock, before
the per-version loop — a preset modified mid-request therefore does not
retroactively change the versions already exported, the same guarantee
`export_with_preset` already offered.

`export` stays synchronous (a correction to the draft, which provided only a
`JobId`): the CLI and a scripted SDK use have no need of the event mechanism
for a one-off export, and `export_async` remains the entry point for Studio,
which wants to return immediately and follow `JobProgress`/`JobFinished`.
`export_async` builds on `export` (the same relation as before this ADR
between `export_batch` and its job), so ADR 0024's per-version narrowing of
the catalog lock applies identically to both forms.

## Consequences

* `Library` goes from five export methods (`export`, `export_batch`,
  `export_with_preset`, `export_async`, `export_with_preset_async`) to two
  (`export`, `export_async`) plus `export_presets`/`create_export_preset`,
  unchanged. `leyline-engine` being internal (§13), this rename is not a
  breaking change in the semver sense — but it touches the repository's three
  clients: the CLI, Studio, and the integration tests, all updated in the
  same change.
* The free functions `export::export_version`/`export::export_batch` (used
  directly by `leyline-engine`'s integration tests over a `&mut Catalog`) do
  not change: they remain the low-level core, not the façade.
* No catalog schema change and no change to the `process1`–`5` formulas: only
  the shape of the request and the number of entry points.

## Alternatives rejected

* **Keeping the five methods and adding `ExportRequest` as a sixth
  convenience form**: rejected — it would duplicate the surface instead of
  reducing it, the opposite of the goal; none of the five existing methods
  brought a capability the single request does not cover.
* **A mandatory `preset: ExportPresetId`, as in the initial draft**:
  rejected, cf. Context — the ad-hoc recipe is a real case, already used
  everywhere (the CLI by default, Studio's dialog), not a hypothetical need
  to anticipate.
* **`export` always returning a `JobId`, as in the initial draft**: rejected
  — a one-off scripted call (CLI, SDK) has no need of the event mechanism;
  the synchronous form already existed (`export_batch`,
  `export_with_preset`) and had no reason to disappear.
