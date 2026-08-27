# ADR 0002 — Slint for the graphical interface

**Status:** Accepted — 2026-07

## Context

Leyline Studio has to be cross-platform, light, fluid over grids of hundreds of thousands of items, and to sit naturally on top of a Rust engine.

## Decision

Leyline Studio's interface is built with Slint.

## Consequences

* Native Rust integration, no JavaScript bridge and no embedded runtime.
* GPU rendering, small memory footprint.
* Licence: the GPL-3.0 branch for the community edition; Slint's royalty-free licence for a future proprietary desktop edition (see ADR 0009).
* A younger ecosystem than Qt's: some widgets will have to be built.

## Alternatives rejected

* **Qt**: mature, but C++ and an expensive commercial licence; second-rate Rust bindings.
* **Tauri / Electron**: a web runtime, with a memory footprint and a latency incompatible with the "Fast" goal.
* **egui**: excellent for tooling, but immediate mode is unsuited to a rich, persistent UI.
* **GTK**: poor integration on macOS and Windows.
