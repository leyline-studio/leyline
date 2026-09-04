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
* a camera set to RAW+JPEG no longer doubles the library ([ADR 0079](adr/0079-raw-jpeg-pairing.md));
* a catalog can span several volumes ([ADR 0085](adr/0085-named-roots.md)) — a root is an identity carried by a marker inside the folder rather than a remembered path, so `catalog.db` still holds no absolute path; an unplugged volume makes its photographs **offline, not missing**, with the grid, the filters and the search still working while anything needing pixels fails with an error naming the root;
* the RAW decoder became a term of the reproducibility promise instead of an unstated assumption ([ADR 0086](adr/0086-decoder-in-the-promise.md)) — `pipeline.md` §5.1 lists it, `make check` refuses a decoder nobody has accepted, and the limits of that guard are written down rather than implied.

Then a run of five slices read straight off Lightroom Classic, each closing a gap a photographer coming from it would hit on the first afternoon: the tethered-capture **bar** rather than a receiver ([ADR 0087](adr/0087-tethered-capture-bar.md)), the presets panel beside develop ([ADR 0058](adr/0058-preset-provenance-and-shelf.md) §1–§4), `Auto` and black and white ([ADR 0088](adr/0088-auto-tone-and-black-and-white.md)), the profile browser ([ADR 0089](adr/0089-camera-profile-browser.md)), and the `Effects` panel — the vignette a photographer *adds*, and grain ([ADR 0090](adr/0090-effects-vignette-grain.md)).

Then the second socket. [ADR 0073](adr/0073-external-mask-detectors.md) had built one for the half of the AI axis that produces *settings*; [ADR 0102](adr/0102-paid-extensions-and-the-pixel-boundary.md) settled the shape of the other half — an extension that must produce **pixels** produces a new asset, never a stage — and [ADR 0107](adr/0107-derived-assets-and-the-pixel-socket.md) builds it: `leyline-derive`, a processor called with two 16-bit TIFFs, and a derived file that replaces the **decode** rather than the development, so white balance, exposure, tone, masks and crop are all still settings on what comes back. No processor ships with Leyline, and a library that has never seen one opens, renders and exports what one produced.

Assisted culling was decided in August ([ADR 0084](adr/0084-assisted-culling.md)), built as a crate that nothing used, and left there deliberately; it is now wired end to end — `Library::cull` measures the cached thumbnails of a shoot and returns a **proposal**, `leyline cull` prints it, Studio's Library ▸ Assisted Culling… narrows the grid to exactly what it proposes. Nothing is written until the photographer presses the reject key themselves, which is §2 and is not negotiable: a wrongly rejected photograph does not look wrong, it looks absent.

Two decisions are taken and **not yet built**, which is the state ADR 0109
was in for a while and is the ordinary order of work here. A develop session
holds the whole catalog's lock while it lives, and it should hold a claim on
its version instead ([ADR 0120](adr/0120-edit-session-claim.md)) — a local
defect worth fixing on its own, and a prerequisite for the second: a tablet
that does not develop but **drives** the machine that does
([ADR 0121](adr/0121-remote-engine-boundary.md)). One catalog, two screens,
no copy — so none of the costs a synchronisation carries, and `pipeline.md`
§5.1 never crossed, because one machine always computes.

What [`readme.md`](readme.md) lists as still open remains the reference: DCP colorimetry not validated against the profile vendor's own render, and two finishing touches to local-adjustment tooling left out of scope by [ADR 0049](adr/0049-local-adjustments-clients.md).

---

## Long term

Plugins, an SDK stable in the semver sense, HDR, panorama, optional local AI.

Each of these subjects is a whole feature, and will need its own ADR before a single line of code.

---

## Related documents

* [`v2-scope.md`](v2-scope.md) — architectural scoping of post-V1 features.
* [`v2-implementation-plan.md`](v2-implementation-plan.md) — recommended sequencing: dependencies, effort, risk.
