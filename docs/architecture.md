# Architecture

This document answers: **how the project is divided, and why that way**. The *why* of the project itself is in [`vision.md`](vision.md); what it does, in [`specification.md`](specification.md).

---

## The guiding principle

**The engine is unaware that the graphical interface exists.**

Studio, the CLI and the SDK are three clients of the same engine, as equals. None has a special pass: what Studio can do, the CLI and a Rust script can do too, because all three go through the same surface. A feature that existed only in Studio would be the sign of a mistake in the split.

That constraint has a real cost — the API has to be designed before the screen — and a counterpart: the engine stays testable without an interface, replaceable without rewriting the interface, and usable by people who will never open Studio.

---

## The crates

Thirteen crates, each with a single responsibility.

| Crate | Responsibility |
|---|---|
| `leyline-core` | Shared types, identifiers, errors, `Settings`. Depends on nothing. |
| `leyline-engine` | Orchestration: jobs, events, rendering, edit sessions. The heart. |
| `leyline-raw` | Decoding RAW files (and JPEG/PNG/TIFF at import). |
| `leyline-catalog` | The SQLite catalog: libraries, assets, versions, revisions. |
| `leyline-preview` | Preview and thumbnail cache. |
| `leyline-color` | Colour management (ICC), reading DCP profiles. |
| `leyline-lens` | Lens corrections: distortion, vignetting, chromatic aberration. |
| `leyline-tether` | USB tethered capture through libgphoto2. |
| `leyline-map` | Reading offline MBTiles tiles for the map view. |
| `leyline-export` | Output encoding: JPEG, TIFF, PNG, WebP, AVIF, and print-to-PDF. |
| `leyline-detect` | The contract, discovery and invocation of **external mask detectors** — executables that turn an image into a coverage ([ADR 0073](adr/0073-external-mask-detectors.md)). Detects nothing itself. |
| `leyline-derive` | The contract, discovery and invocation of **external pixel processors** — executables that turn the develop buffer into another image ([ADR 0107](adr/0107-derived-assets-and-the-pixel-socket.md)). Processes nothing itself. |
| `leyline-cull` | Technical quality measures and burst fingerprints for **assisted culling** ([ADR 0084](adr/0084-assisted-culling.md) §4): focus, clipping, perceptual fingerprint. No model, no weights, no I/O — it computes numbers and decides no verdict. The engine turns them into a proposal. |
| `leyline-sdk` | The engine's stable public surface. The semver contract. |
| `leyline-cli` | The command-line client. |
| `leyline-studio` | The desktop application (Slint). |

---

## Direction of dependencies

```
Studio  →  SDK  →  Engine  →  Core
```

`Catalog`, `RAW`, `Color`, `Lens`, `Tether`, `Map`, `Preview`, `Export`, `Derive` and `Cull` are consumed by `Engine`.

`leyline-detect` stands apart: it depends only on `leyline-core`, the engine does not know it exists, and it is the SDK that re-exports it to clients. A mask detector has no business on a render path (ADR 0073 §2).

`leyline-derive` depends only on `leyline-core` too, and the engine *does* consume it — the one difference between the two sockets, and it is not an inconsistency. A detector's answer becomes a setting the client writes; a processor's answer has to be **rendered before** and **imported after**, and both of those are the engine's own work ([ADR 0107](adr/0107-derived-assets-and-the-pixel-socket.md) §1). Nothing of the processor reaches a render path either way: what it produces is a file in the library.

**No circular dependency is admitted.** The rule is mechanically verifiable: `cargo tree` must stay a tree.

Two consequences that come up often in review:

* `leyline-core` depends on no other crate of the project. If a type needs to descend into it, it is because it is shared; if it is not, it does not belong there.
* `leyline-sdk` contains **nothing but** re-exports. That is deliberate: the SDK is the semver contract, which leaves `leyline-engine` free to evolve with every version. Its only failure mode is the *hole* — a type the engine returns but an external caller cannot name — hence the surface test that goes with it.

---

## Inside Studio

Studio consumes `leyline-sdk` and nothing else: `crates/leyline-studio/Cargo.toml` declares a single Leyline dependency, and all of `src/` references only `leyline_sdk`. That is the same position as a third-party product integrating the SDK. A need that Studio answered by a detour into an internal crate is the sign that something is missing from the public API: the fix is to widen the SDK, never to go around it.

Under that constraint, the interface splits into four layers ([ADR 0045](adr/0045-studio-ui-modularisation.md)):

```
ui/studio.slint     assembly: window attributes, keyboard shortcuts, mount order
ui/types.slint      the structs of the Rust ↔ UI contract, and the translation templates (Tr)
ui/state/           one Slint `global` per domain — the only surface that crosses into Rust
ui/widgets/         reusable controls, with no knowledge of state
ui/panels/          the views (develop, map, browser), the dialog overlay, the menu bar
ui/dialogs/         one file per modal dialog
```

On the Rust side, `src/wiring/` is the exact mirror of `ui/state/`: one module per global, which reaches its own through `Global::<T>::get(&window)` and leaves the others alone. Around it, `app.rs` (application state), `library.rs` (which library is open), `events.rs` (the engine event pump) and `models.rs` (conversions towards display) — plus the pure-logic modules `develop.rs`, `map_view.rs`, `format.rs` and `classify.rs`, which know no Slint type and are the only ones that can be unit-tested.

### The window fits the screen, and the interface fits the window

Two rules, and they only work as a pair.

`startup_size` (`src/main.rs`) opens the window at two thirds of the screen, raised to a floor of 900×600, then **lowered to what the screen can actually show** — that last clamp applied last, so it always wins. The floor is a wish, not a guarantee: a screen too small to hold it gets a window that fits instead of one that overflows. A separate step pulls the other way when there is room, opening wide enough for the side panels whenever the screen can afford them, so folding stays something a small screen forces rather than something a large one chooses.

`narrow-threshold` (`ui/studio.slint`) is the other half: below 1340 logical pixels the browser's side panels fold away on their own. Without it a 900-pixel window would be a lie — the sidebar and the detail panel are 500px of fixed width between them, and the filter bar cannot shrink past its controls, so a narrow window did not compress the layout, it pushed the right-hand panel outside the window where nothing hinted that it existed.

Three things are worth knowing before touching either:

* **Develop is deliberately excluded from the automatic fold.** Its 300px panel *is* the module — every slider lives in it — so folding it would turn Develop into a picture viewer. There, only the manual gesture folds, where hiding the controls is what the user just asked for.
* **The fold is assigned, never bound.** A binding that reads the window's width is a cycle — the width is derived from what the content asks for, and the content would ask based on the width. Slint reports it as a *warning* and then resolves it with a stale value, so it would have shipped looking correct. It is written from a `changed width` handler instead: an event handler assigns, it does not join the dependency graph. The same geometry hazard [ADR 0045](adr/0045-studio-ui-modularisation.md) §3 records, reached from the other side.
* **A line of text sets a view's minimum width.** A Slint `Text` reports its natural width as its column's minimum even when it would elide; giving it `min-width` is what lets it yield. Develop's shortcut hint alone demanded ~1200px until it got one — the same mechanism `develop.slint` already documents for the *maximum*, which cuts both ways.

The threshold is tuned from the longest translation rather than from English: the same bar is markedly wider in French, and a threshold set on English would leave French overflowing in the band between the two.

The rule that bounds the use of globals is in [`contributing.md`](contributing.md#ui-state--global-or-local).

---

## External building blocks

Every heavy dependency has been the subject of a written decision.

| Building block | Role | Decision |
|---|---|---|
| **Rust** | The project's single language | [ADR 0001](adr/0001-rust.md) |
| **Slint** | Studio's graphical interface | [ADR 0002](adr/0002-slint.md) |
| **SQLite** | The catalog's database | [ADR 0003](adr/0003-sqlite.md) |
| **LibRaw** | RAW decoding (LGPL branch) | [ADR 0004](adr/0004-libraw.md) |
| **Lensfun** | Lens correction profiles | [ADR 0005](adr/0005-lensfun-littlecms.md) |
| **LittleCMS** | ICC transforms | [ADR 0005](adr/0005-lensfun-littlecms.md) |
| **libgphoto2** | USB tethered capture | [ADR 0038](adr/0038-tethered-capture.md) |
| **Rayon** | The engine's data parallelism | [ADR 0012](adr/0012-rayon-data-parallelism.md) |
| **BLAKE3** | File checksums | [ADR 0006](adr/0006-blake3.md) |
| **kamadak-exif** | Reading EXIF from the files LibRaw does not read | [ADR 0056](adr/0056-non-raw-exif-import.md) |
| **ab_glyph** | Rasterising the glyphs of the text watermark | [ADR 0051](adr/0051-watermark-rasterization-and-soft-proof-surface.md) |
| **DejaVu Sans** (an asset, not a crate) | The watermark's embedded font, for an identical render on every machine | [ADR 0051](adr/0051-watermark-rasterization-and-soft-proof-surface.md) |
| **rfd** | Native folder pickers in Studio | — |
| **darktable noise profiles** (data, not a crate) | Measured sensor variance per camera body and per sensitivity, frozen with the stage version that reads it (GPL-3.0-or-later, © the darktable contributors) | [ADR 0072](adr/0072-measured-noise-profile.md) |

The underlying reasons, in short: **Rust** for performance close to C++ with memory safety and portability that costs nothing; **Slint** because it is cross-platform, light and designed for Rust; **SQLite** because a catalog ought to be a plain file, serverless, robust and fast — and readable by any tool twenty years from now.

---

## Storage

A Leyline library is **self-contained and movable**: every path it stores is relative to its root ([ADR 0010](adr/0010-relative-paths.md)).

```
Library/
 ├─ catalog.db        the SQLite catalog
 ├─ Photos/           the files imported in copy mode
 ├─ Profiles/         ICC and DCP profiles supplied by the user
 └─ cache/
     ├─ thumbs/       thumbnails
     └─ preview/      developed previews
```

The catalog **never** holds the photos: only references, metadata, settings, collections and indexes. Previews live in a dedicated cache, rebuildable by definition — deleting it loses nothing. RAW files are re-read only when they must be.

The complete schema is specified in [`catalog.md`](catalog.md).

---

## The develop pipeline

Every correction is applied as a pipeline of independent steps, from the decoded RAW through to the output encoding. The order of operations is not an implementation detail: it is part of the render contract, exactly as the formulas are.

The exact order, the `settings_json` format, the stage versions and the reproducibility promise are specified in [`pipeline.md`](pipeline.md) — to be read before any intervention on the engine.

In the code, each operator lives in its own versioned and frozen module (`leyline-engine/src/stages/`, one folder per operator, one file per version), and the `stages.rs` registry says at which rank each version is inserted ([ADR 0042](adr/0042-versioned-stage-pipeline.md), [ADR 0043](adr/0043-collapse-prerelease-render-history.md)).

---

## Development conventions

* `cargo fmt`, `cargo clippy` without a single warning, green tests: all three before every commit.
* Unit tests, integration tests and benchmarks for every feature.
* Public APIs documented.
* No dead code.

The detail is in [`contributing.md`](contributing.md).
