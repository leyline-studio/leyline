# ADR 0038 — Tethered capture: importing straight from the camera over USB (libgphoto2)

**Status:** Accepted — 2026-07

## Context

A photographer shooting in a studio (or any context where the camera stays
connected to a computer) wants every photo to appear in the software the
moment the shutter fires, without pulling the memory card — that is
tethering, Lightroom's "Tethered Capture" feature. It was covered neither by
`docs/specification.md` §Included (which provides only for importing an
existing folder) nor by a deliberate exclusion: the subject had simply never
been settled.

Two families of solution exist:

1. **Per-manufacturer proprietary SDKs** (Canon EDSDK, Nikon SDK, Sony
   Imaging Edge SDK…) — Lightroom's approach: one module per brand, each
   under a closed licence, with redistribution subject to a manufacturer
   agreement.
2. **libgphoto2** — a free C library (LGPL) that speaks PTP (and the
   proprietary extensions layered over PTP for most brands), covering several
   hundred Canon/Nikon/Sony/Fujifilm/Olympus bodies. It is already the tool
   free tethering software uses (entangle, digiKam), and Linux ships it as a
   standard system package.

## Decision

Leyline implements tethering through **libgphoto2**, never through a
manufacturer SDK — consistent with the choice already made for LibRaw (ADR
0004), Lensfun and LittleCMS (ADR 0005): free C libraries, with no dependence
on a per-camera-brand licence agreement.

A new **`leyline-tether`** crate wraps the `gphoto2` Rust crate (safe bindings
over libgphoto2) and exposes only:

* `TetherSession::connect(staging_dir, on_event)` — auto-detects the first USB
  camera found, starts a dedicated thread that polls the camera
  (`Camera::wait_event`, polling every 500 ms) and downloads every file it
  reports (`CameraEvent::NewFile`) into `staging_dir`;
* `TetherSession::stop()` (and `Drop`) — stops the polling and releases the
  camera;
* `TetherEvent::{Captured, Disconnected}` — notifications surfaced through
  `on_event`, called on the session's thread.

That crate never touches the catalog: importing each received file is the
engine's business. `Library::tether_connect()` (`leyline-engine`) starts a
session and, for every `TetherEvent::Captured`, calls the existing import core
(`Library::import`, `copy_files: true`) — **a tethered capture is an import
like any other**, not a separate data path: the same BLAKE3 checksum, the same
thumbnail generated, the same `Event::AssetsAdded` fired
(`docs/engine-api.md` §3.2). No new "capture" event exists:
`Event::AssetsAdded` suffices, and the client re-queries as for any import (a
principle already laid down by ADR 0011 — events are notifications, never
data).

Two new events, only for the connection's life cycle, which `AssetsAdded`
cannot carry:

```rust
pub enum Event {
    // ...
    TetherConnected,
    TetherDisconnected { reason: Option<String> },
}
```

`docs/engine-api.md` §3.2 is extended accordingly. One session per `Library`
(one camera at a time): `tether_connect` refuses a second connection while a
session is open — simultaneous multi-camera stays out of scope, to be
revisited if the need arises.

Nothing changes in the render pipeline: tethering touches only import, and no
process version is concerned.

## Consequences

* A new system dependency: `libgphoto2` (plus its development headers at build
  time) — the same family of constraint as LibRaw, Lensfun and LittleCMS,
  already packaged by the installer (`docs/adr/0019`). Windows and macOS will
  have to bundle or link `libgphoto2` as they do those libraries; that
  per-platform packaging work stays open (the crate and the engine are ready,
  only the per-OS binary distribution remains — the same status as the rest of
  Phase 8).
* `docs/specification.md` §Included gains "Tethered capture (USB,
  libgphoto2)".
* `leyline-cli` gains `leyline tether <library>`: it opens a session, prints
  each imported asset, and stops cleanly on disconnection or on Ctrl+C — the
  same API Studio will use for its tethering panel (§13, ADR 0011: the CLI and
  Studio consume the same SDK surface).
* Studio's interface: File ▸ Tethered Capture… (`T`) opens a modal panel that
  calls `tether_connect`/`tether_disconnect`, shows the connection's state,
  the session's shot count and the name of the last file received — just one
  more client of the API above, with no further architectural decision needed
  to add it. **Superseded in shape, not in principle, by
  [ADR 0087](0087-tethered-capture-bar.md)**: the dialog now decides only what
  precedes a session (its folder and its preset), and a floating bar drives
  the camera once one is open. The receiver this ADR describes is still the
  whole of the import path.
* With no USB camera plugged in (CI, an ordinary development machine),
  `TetherSession::connect` fails cleanly with `TetherError::NoCamera` rather
  than blocking or panicking — that is the only behaviour testable without
  hardware, and `leyline-tether`/`leyline-engine`'s tests verify it
  explicitly.

## Alternatives rejected

* **A per-manufacturer proprietary SDK (Canon EDSDK, and so on)**:
  redistribution and per-platform compilation subject to a separate
  manufacturer agreement per brand, incompatible with the "one repository, a
  uniform GPL-3.0 licence" model (ADR 0009) — it would have required one crate
  per brand, each with its own binary licence constraints.
* **A generic watch folder (watching a folder where third-party software or
  the camera itself drops files)**: simpler, zero new dependencies, but it
  does not answer the "like Lightroom" ask — it adds latency (a disk write
  before detection) and depends on a third-party tool to actually drive the
  camera. It remains a possible extension later (for cameras libgphoto2 does
  not support) but is not the main route taken here.
* **Modelling capture as a `Job` (`JobId`/`JobFinished`)**: a tethering
  session has neither a total nor an end determined in advance — it lasts as
  long as the camera stays plugged in. The `Job` contract
  (`docs/engine-api.md` §3.1) assumes an end; forcing this case into it would
  have meant a `JobFinished` that never finishes, or an arbitrary total. A
  connection state (`TetherConnected`/`TetherDisconnected`) plus the usual
  `AssetsAdded` describes what actually happens better.
