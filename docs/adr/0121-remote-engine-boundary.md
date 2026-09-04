# ADR 0121 — Driving the engine from another device

**Status:** Accepted — 2026-09

## Context

A photographer with a library on a desktop machine wants to develop from a
tablet on the sofa. There are two ways to grant that, and they are not
variants of one idea.

**Two catalogs and a synchronisation.** This is the one
[`specification.md`](../specification.md) §4 excludes, and the exclusion is
sound on the merits, not only by principle: it costs a conflict model
(`version_head` is one head per version, so two devices editing one
photograph make two heads and one of them loses), an identity model across
machines, a way to carry 25–60 MB files to a tablet — and, worst,
**pixels that differ per device**. [`pipeline.md`](../pipeline.md) §5.1 names
the platform, the toolchain and the decoder ([ADR 0086](0086-decoder-in-the-promise.md));
a Windows x86 build and an ARM tablet build are outside it by construction.
[ADR 0080](0080-the-promise-and-its-boundary.md) §2 already says identical
pixels across machines are not promised. A synchronisation would therefore
ship a photograph that looks slightly different depending on which screen
opened it, and would have to explain that forever.

**One catalog and two screens.** The tablet does not develop; it *drives* the
machine that does. There is no copy, so there is no conflict, no merge and no
identity to reconcile. §5.1 is never crossed, because the pixels are always
computed by one machine, one decoder, one toolchain — the same ones as
yesterday.

The second is the right shape, and the architecture already leans that way:
the engine has been unaware that a graphical interface exists since the
beginning, and Studio, the CLI and the SDK are already *only* clients.

### The feasibility is a number, so it was measured

| | measured |
|---|---|
| JPEG encode, 45 Mpx, file written | **876 ms** (`export_encode/45mpx_jpg`) |
| the same per 1024 px frame (0.70 Mpx) | **~14 ms**, an upper bound |
| live render with the proxy cache ([ADR 0076](0076-proxy-cache.md)) | **15 ms** (10 Mpx), **11 ms** (30 Mpx) |
| one 1024 px frame, JPEG | ~150 KB → **~30 Mbit/s** at 25 frames per second |

Finger to pixel: ~15 ms of render, ~14 ms of encoding, 2–3 ms of local
network, ~5 ms of decoding on the tablet — **35 to 45 ms**, inside
[ADR 0074](0074-live-preview-while-dragging.md) §3's 40 ms bound, and better
than the 59 ms Studio shipped *locally* on the day live preview was accepted.

Performance is therefore not what decides this. What decides it is below.

## Decision

### 1. One catalog, two screens — never a copy

The desktop holds the library, the catalog and the photographs. The tablet
holds a view. Nothing is replicated, nothing is merged, nothing is hosted.

This is **not** the synchronisation §4 excludes, and the two must not be
confused in a year: a synchronisation holds a second copy of the catalog, and
this holds none.

### 2. The remote surface is the SDK's, and not one verb more

`leyline-sdk` is a **pure façade** — re-exports only, with `tests/surface.rs`
standing guard over the holes. That list is the remote surface, and the guard
extends to cover it. A verb that is not in the façade is not remotable; a
verb added to the façade is answerable for whether it is.

The consequence is that this ADR invents no API. It serialises one that
exists, and every future decision about what the engine offers keeps being
taken once.

### 3. A new crate, and the engine learns nothing

The server is a client of the SDK exactly as the CLI is — its own crate,
depending on `leyline-sdk`, adding whatever transport and async runtime it
needs. **`leyline-engine` gains no network dependency**, no runtime and no
awareness that a remote client exists, precisely as it gained none of the
GUI's. The dependency direction of [`architecture.md`](../architecture.md)
is not bent for this.

### 4. A verb naming a path on the caller's disk is not remotable

Import a folder, export into a directory, choose an ICC profile or a `.cube`:
every one of these names a path, and a tablet has no such disk. They stay
local until a **server-side** browsing surface is decided on its own merits —
which is a real decision, because an engine that lists and reads arbitrary
paths on request is a file server, and that is the sharpest hazard in this
whole ADR.

The first boundary is therefore the **develop and browsing** subset: open a
library, query the grid, read metadata, rate and flag, get previews, run an
edit session. That is the sofa, which is the request.

### 5. Events cross unchanged, and that is the load-bearing reuse

`Event` is already documented as *"notifications, never complete data: a
client re-queries what it needs, so the stream and the catalog can never
disagree"*. That sentence was written for an in-process client and it is
exactly what a network protocol needs: **the wire carries "something
changed", never state**. Nothing is replicated, so nothing can diverge, and
the failure mode a naive remote API is made of — a client believing a stale
copy — cannot occur.

Jobs cross unchanged for the same reason: `JobId`, `JobProgress`,
`JobFinished` are already an asynchronous protocol.

### 6. Frames are encoded images, and may be missed

The preview reaches the tablet as an encoded image, not a buffer, with the
discipline `TetherLiveFrame` already established: *safe to miss — a client
that skipped ten reads the newest once and is right.* A dropped frame during
a drag is a frame nobody needed; a queue of stale frames is lag.

### 7. Off by default, paired by a gesture, local network only

* **Off until switched on.** [ADR 0080](0080-the-promise-and-its-boundary.md)'s
  fifth guarantee — *nothing leaves your machine without an explicit action* —
  is satisfied by the pairing, and only if the pairing is the *first* thing
  that happens, not a default that has to be turned off.
* **No relay, no account, no certificate authority, no server of ours.** The
  test is blunt and is the same one [ADR 0059](0059-bundled-world-basemap.md)
  and the map module already pass: it must work with the internet unplugged.
* **Mutual authentication and confidentiality**, from a code shown on the
  desktop and typed on the tablet. A local network is not a trusted network.
  The cryptographic primitive is an implementation choice; that it exists is
  not.
* The five conditions of [ADR 0080](0080-the-promise-and-its-boundary.md) §4
  are met without effort, and it is worth writing down which: nothing is
  hosted (a), the catalog on your disk *is* the original and there is no other
  (b), everything remains in the documented SQLite it was always in (c),
  nothing is sold and the desktop application is complete with the tablet
  switched off (d), and (e) is the paragraph above.

### 8. The session lock is settled first

A remote holder's disappearance cannot be observed, and today a session holds
the entire catalog's lock for as long as it lives. [ADR 0120](0120-edit-session-claim.md)
settles that — a claim on a version, and a deadline for holders whose end
cannot be seen — and this ADR depends on it. It is a prerequisite, not a
consequence: the lock is a defect locally too, and it is worth fixing whether
or not a tablet ever exists.

## Consequences

* **Three separate projects, not one**: a transport for the façade, the claim
  and lease of ADR 0120, and a touch client. Sequencing them apart is what
  keeps any of them finishable.
* **The cheapest proof needs no interface at all.** Make the existing CLI talk
  to a remote library: if `leyline develop <remote library> …` works from
  another machine, everything above it works. A real milestone at the cost of
  zero pixels.
* **A mobile client needs none of the native dependencies.** LibRaw, Lensfun,
  LittleCMS and libheif stay on the desktop. That is the entire difference
  between this and a port, and it is most of why this is the affordable road.
* **The touch interface is a new, smaller interface**, not the develop panel
  recompiled: that panel is a dense pointer-driven surface, and a finger is
  not a pointer.
* `specification.md` §4 gains a line distinguishing this from the excluded
  synchronisation, so the two are not confused by a reader — or by us.
* **Nothing in the render changes.** No stage version, no schema, no
  migration, no golden entry.

## Rejected

* **Synchronising two catalogs** (Context). Conflicts, cross-machine identity,
  60 MB files on a tablet, and pixels that differ per device — four costs, in
  exchange for offline editing on the tablet, which is not what was asked.
* **Porting the engine to the tablet.** The native dependencies must be
  cross-built for ARM, iOS adds a store, and the result would render the same
  revision into different pixels (§5.2) on the two screens of one library.
* **A cloud relay**, so the tablet works from anywhere. It fails ADR 0080 §4
  (b) and (e) at once, and it makes a photographer's library depend on our
  bill being paid.
* **A REST API shaped like the catalog schema.** It would put state on the
  wire, which is exactly what the event design exists to avoid, and it would
  freeze a database schema as a public protocol.
* **On by default, bound to every interface.** A convenience that opens a file
  server on a café's Wi-Fi is not a convenience.
* **Streaming raw pixel buffers.** 2.1 MB per frame against 150 KB, for an
  image about to be shown on a screen — and it would put the working buffer,
  which is unbounded linear Rec. 2020 ([ADR 0044](0044-linear-wide-gamut-working-space.md)),
  on a wire where nothing can interpret it.
