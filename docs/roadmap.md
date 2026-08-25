# Roadmap

The state of the construction phases. What each scope contains in detail is in [`specification.md`](specification.md).

**Phases 0 through 8 are complete.** The work in progress is no longer about filling gaps in the V1 scope, but about consolidating what exists.

---

## Phase 0 — Documentary foundations ✅

Vision, architecture, conventions. The whole documentation laid down before the first line of Rust, as the project's motto requires.

## Phase 1 — Workspace ✅

Cargo workspace, split into crates, build and test conventions.

## Phase 2 — Reading images ✅

RAW, JPEG, PNG and TIFF decoding, and display.

## Phase 3 — Catalog ✅

SQLite catalog and thumbnails.

## Phase 4 — Pipeline ✅

Non-destructive develop pipeline, `settings_json`, render versions.

## Phase 5 — Leyline Studio ✅

Desktop interface: browser, filters, rating, develop module, collections, dialogs, menus.

## Phase 6 — Export ✅

JPEG, TIFF, PNG, WebP, AVIF, export presets and batch export.

## Phase 7 — Optimisations ✅

Rayon parallelism ([ADR 0012](adr/0012-rayon-data-parallelism.md)), Criterion benchmarks, and the whole of [ADR 0041](adr/0041-interactive-preview-rendering.md): preview rendering at display resolution, radius scaling, and a cache of intermediate states on the preview path (−78 % on an end-of-pipeline slider).

The **export** path, which ADR 0041 deliberately leaves out of its optimisations, has been measured since 2026-08-03 (`benches/export.rs`): decoding, full-resolution rendering and per-format encoding, all linear in pixels. The figures, and the two follow-ups they point to — AVIF encoding speed, overlapping encoding with rendering inside a batch — are in [`measured-findings.md`](measured-findings.md) §B2.

## Phase 8 — Distribution ✅

Per-platform installer and FR/EN internationalisation ([ADR 0019](adr/0019-distribution-i18n.md)).

## Versioned stage pipeline ✅

The eleven duplicated `processN.rs` modules (14,968 lines, 70 to 93 % duplication) are replaced by independently versioned stages, then the pre-publication render history is collapsed. In the order the ADRs impose:

1. ✅ Capture reference renders from the engine as it then stood, and commit them.
2. ✅ Refactor towards composed stages ([ADR 0042](adr/0042-versioned-stage-pipeline.md)) — ~1,800 lines of versioned stages (`crate::stages`), a registry, and a `process: N` → stage versions expansion table.
3. ✅ Prove bit-for-bit equality against those renders, for all eleven versions — the 77 cases in `golden_renders.rs` passed without re-blessing.
4. ✅ Collapse the history ([ADR 0043](adr/0043-collapse-prerelease-render-history.md)) — the project never having been published, those eleven versions bound no one: a single version per operator (`v1`), the expansion table removed, and §2 of ADR 0042 delivered in the same move — a revision records its own `stages` map, the `process` field disappears. Development catalogs from before are not migrated (ADR 0043 §5).

---

## In progress

The post-V1 scoping is closed, and so is the measurement campaign that followed it: [`measured-findings.md`](measured-findings.md) leaves **nothing open in this repository** — its axes A (render accuracy) and B (performance) are delivered or rejected with their reasons, and its axis C depends on a detector that lives outside the repository ([ADR 0073](adr/0073-external-mask-detectors.md)).

Since then, the work has been about what a **first publication** still lacks:

* the render follows the slider while a setting is dragged, and the display proxy is cached ([ADR 0074](adr/0074-live-preview-while-dragging.md), [ADR 0076](adr/0076-proxy-cache.md)); the preview cache fits inside a bounded window ([ADR 0075](adr/0075-preview-cache-retention.md));
* an installed version knows that a newer one exists, without transmitting anything and without ever installing itself ([ADR 0077](adr/0077-application-updates.md)), and the catalog is backed up before any migration;
* the Preferences panel exists, along with the admission rule that decides what is allowed into it ([ADR 0078](adr/0078-preferences-panel.md));
* a camera set to RAW+JPEG no longer doubles the library ([ADR 0079](adr/0079-raw-jpeg-pairing.md)).

What [`readme.md`](readme.md) lists as still open remains the reference: DCP colorimetry not validated against the profile vendor's own render, and two finishing touches to local-adjustment tooling left out of scope by [ADR 0049](adr/0049-local-adjustments-clients.md).

---

## Long term

Plugins, an SDK stable in the semver sense, HDR, panorama, optional local AI.

Each of these subjects is a whole feature, and will need its own ADR before a single line of code.

---

## Related documents

* [`v2-scope.md`](v2-scope.md) — architectural scoping of post-V1 features.
* [`v2-implementation-plan.md`](v2-implementation-plan.md) — recommended sequencing: dependencies, effort, risk.
