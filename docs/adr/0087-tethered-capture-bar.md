# ADR 0087 — Driving the camera: the tethered-capture bar

**Status:** Accepted — 2026-08

## Context

ADR 0038 gave Leyline tethering, but only half of it: a **receiver**.
`TetherSession` autodetects a USB camera, waits for `CameraEvent::NewFile`,
downloads the file and hands it to the import core. Everything else about the
session is invisible and untouchable — the interface is a modal dialog with a
status line, a counter and two buttons.

Lightroom Classic's tethered-capture bar, floating over the photo, is the
reference for what a studio session actually needs:

* which body is connected, and under which session name;
* a **Live** toggle showing the camera's own live view;
* **shutter speed, aperture, ISO and white balance**, read from the body and
  settable from the computer;
* a **release button** that fires the shutter without touching the camera;
* a **Develop Settings** picker, so every shot lands already developed.

None of that exists here. The gap is not a missing library — libgphoto2
exposes all of it — it is a missing shape: the camera is currently owned by a
polling thread that nothing can talk to, and the session reports nothing but
"connected / n shots".

Three constraints frame the design.

1. **libgphoto2 is not reentrant on one camera.** The `gphoto2` crate already
   funnels every call onto a single background thread of its own and blocks
   the caller on a `Task`. So a second thread calling `set_config` while our
   poll thread sits in `wait_event(500 ms)` does not corrupt anything — it
   simply queues behind that wait. At 500 ms per cycle, a live view driven
   that way would run at two frames a second.
2. **ADR 0011: events are notifications, never data — the client re-queries.**
   The client cannot re-query the *catalog* for camera state, because camera
   state was never in it. And a live-view frame is a JPEG arriving ten times a
   second, which must not be cloned through a broadcast channel to every
   subscriber.
3. **A tethered shot is an import like any other** (ADR 0038). Nothing added
   here may create a second data path into the catalog.

## Decision

### 1. One thread owns the camera, and it takes orders

The session's existing polling thread becomes a **command loop**. Each cycle:
drain a `mpsc` command queue and execute each command, grab a live-view frame
if live view is on, then wait for one camera event. The wait is short
(50 ms) while live view is on and long (500 ms) when it is off — the timeout
is the loop's only idle cost, and only live view needs the loop to spin fast.

The camera therefore stays owned by exactly one thread, as before; what
changes is that the thread now has an inbox. `TetherSession` gains

```rust
pub fn capture(&self);
pub fn set_setting(&self, setting: TetherSetting, value: &str);
pub fn set_live_view(&self, on: bool);
pub fn refresh_settings(&self);
```

All four **enqueue and return**. No client thread — least of all a UI thread —
ever blocks on USB.

### 2. Commands notify, the session holds the state

Following ADR 0011's spirit while answering constraint 2, the session keeps
its two pieces of live state in `Mutex` slots and emits bare notifications:

| Slot | Notification | Read back through |
| --- | --- | --- |
| `CameraSettings` (model + the four settings and their choices) | `TetherEvent::SettingsChanged` | `Library::tether_settings()` |
| Latest live-view frame (JPEG bytes) | `TetherEvent::LiveFrame` | `Library::tether_live_frame()` |

Both readers are cheap and lock-free of the camera: they clone an `Arc` out of
a mutex, never talk to USB. A client that misses ten `LiveFrame`
notifications reads the newest frame once and is correct — which is the right
behaviour for a video feed, and the one a data-carrying event would get wrong.

Settings are re-read after every accepted `set_setting`, after every capture,
and on a timer (every 2 s) — so turning a dial **on the body** shows up in the
bar, which is the half of "read from the body" that a one-shot read misses.

A command that fails surfaces as `TetherEvent::CommandFailed { message }` →
`Event::TetherCommandFailed { message }`. A failed `set_setting` is not a
disconnect: a body refusing 1/8000 in its current mode must leave the session
running.

### 3. Settings are named by intent, not by libgphoto2 key

libgphoto2 exposes a camera's configuration as a tree of a hundred-odd widgets
whose names differ per brand and per body. The bar needs four of them, so the
four are named by what they *are* and each probes an ordered list of candidate
config names:

```rust
pub enum TetherSetting { Shutter, Aperture, Iso, WhiteBalance }
```

`TetherSetting`, `CameraSetting` and `CameraSettings` live in **`leyline-core`**
and not in `leyline-tether`, for the reason ADR 0038 already set: the `tether`
feature removes the *backend* and never the API. A client built without
libgphoto2 still calls `tether_settings()` and still needs the type it returns.
The libgphoto2 *spelling* of each setting stays in `leyline-tether`, where the
library it belongs to is.

* `Shutter` → `shutterspeed`, `shutterspeed2`, `eos-shutterspeed`
* `Aperture` → `aperture`, `f-number`, `eos-aperture`
* `Iso` → `iso`, `isospeed`, `eos-iso`
* `WhiteBalance` → `whitebalance`, `whitebalancemode`

A setting the body does not expose is `None` — the bar hides that control.
It is never an error: a phone or a webcam that libgphoto2 drives has no
aperture, and refusing to tether one over that would be absurd.

Only `Widget::Radio`/`Widget::Text` are read (a list of choices, or a free value).
A read-only widget is reported `readonly` and shown as a value, not a picker.

### 4. A capture session is a name, and the name is a folder

`tether_connect` takes options:

```rust
pub struct TetherOptions {
    pub session: String,          // folder name; empty = "Tethered"
    pub preset: Option<PresetId>, // develop preset applied on arrival
}
```

The session name is a **folder under `Photos/`**, obtained without a new
copy path: shots stage into `Cache/Tether/<session>/` and are imported with
`import_files(Cache/Tether, …)`, so the import core's existing
"mirror the file's position relative to the source" rule (`import.rs`,
`copy_into_photos`) files them at `Photos/<session>/<name>`.

The name is validated as a single path component (no separator, no `..`, not
empty after trimming) — the same discipline
`validate_library_relative_path` enforces for stored paths, applied at the
one point where a user string becomes a directory.

A shot whose camera filename already exists at the destination gets a
`-1`, `-2`… suffix **before** the import call. Left alone, the import core
would return a `Skip` for a name collision and the shot would vanish
silently — which is exactly what a card reformatted mid-session produces.

### 5. The preset is applied inside the import handler

`TetherOptions::preset` is applied by `handle_tether_event`, on the version
the import just created, before `Event::AssetsAdded` is emitted. Applying it
from the client instead would work, but every shot would flash its neutral
render first and settle a moment later — the one thing a tethered session,
which exists to judge the shot as it is taken, must not do.

Nothing else changes: the preset goes through `apply_preset`, records itself
in the revision like any other application (ADR 0058 §5), and touches no
process version.

### 6. Live view never enters the catalog

Live-view frames are `Camera::capture_preview()` JPEGs held in memory and
overwritten by the next one. They are never written to `Cache/`, never
imported, never hashed, never previewed. A live view is a viewfinder, not a
photograph — the catalog learns of a frame only if the shutter actually fires.

### 7. Studio shows a bar, not a dialog

File ▸ Tethered Capture… (`T`) keeps a dialog, now reduced to what is decided
*before* a session: the session name, the develop preset, Connect.

Once connected, a **`TetherBar`** overlays every module (browser, develop,
loupe, compare, survey, map), because a studio session is not a mode one
enters — the photographer switches to Develop to judge the last frame while
the next one is being set up, and the release button must still be there.
The bar carries: body model + session name, Live, the four settings as
popup pickers, the develop-preset name, the shot counter, the release button,
and Disconnect.

## Consequences

* `leyline-tether` grows from a downloader to a session driver, and keeps its
  one rule: **it never touches the catalog**. Sessions, presets, folders and
  collisions are all decided in `leyline-engine`.
* `docs/engine-api.md` §3.2 gains `TetherSettingsChanged`, `TetherLiveFrame`
  and `TetherCommandFailed`; §6bis is rewritten around the new surface.
* `docs/specification.md` §Included: "Tethered capture (USB, libgphoto2)"
  becomes "Tethered capture (USB, libgphoto2): live view, camera settings,
  remote release, develop preset on capture".
* `leyline-cli`'s `leyline tether` gains `--session`, `--preset` and
  `--capture-every <s>` (an intervalometer falls out of `capture()` for free,
  and is the one thing a command line can do here that a bar cannot).
* The `tether` feature still removes the *backend* and never the API
  (`Cargo.toml`, ADR 0038): every new call compiles in a build without
  libgphoto2 and reports the same "not available in this build" error.
* No process version is concerned. Tethering still touches only import.
* Still untestable without hardware. The tests that exist stay honest about
  it: the pure logic that is new — session-name validation, collision
  suffixing, config-key probing over a synthetic widget list — is tested; the
  USB path is not, and no mock pretends otherwise.

## Alternatives rejected

* **A blocking `Library::tether_settings()` that round-trips to the camera.**
  Simplest to write, and it would freeze Studio's UI thread for up to the
  length of one `wait_event` on every repaint that reads a value. The slot +
  notification pair costs one mutex and never blocks.
* **Exposing libgphoto2's whole config tree in the interface.** Honest, and
  free of the per-brand key probing of §3 — but a two-hundred-row tree of
  `d1a4`-style widget names is a diagnostic tool, not a capture bar. The four
  settings that belong on the bar are the four a photographer changes between
  frames; anything else is set on the body once.
* **Driving live view by writing frames to `Cache/` and reusing the preview
  path.** It would put a video feed through a disk cache built for
  photographs, and make the catalog's cache grow at ten files a second for
  the length of a session.
* **A `Job` for the session** — already rejected by ADR 0038 for the same
  reason: a session has no total and no predetermined end.
