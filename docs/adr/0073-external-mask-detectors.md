# ADR 0073 — Detected masks: an open socket, separate detectors

**Status:** Accepted — 2026-08

## Context

C2 of [`measured-findings.md`](../measured-findings.md) — automatic masks — is
the only item on the AI axis compatible with
[`pipeline.md`](../pipeline.md) §5.1 without compromise, and
[ADR 0069](0069-closed-extension-boundary.md) settled *where* a closed feature
attaches: it produces settings, never pixels.

**The open half is already delivered**, and that must be said before deciding
anything:

* [ADR 0070](0070-stored-mask-coverage.md) gave `Mask` a `Coverage` variant — a
  mask can be a stored image — and gave the library `store_mask_coverage`,
  whose comment already announces "the surface a closed extension uses through
  the SDK";
* the same ADR, §7, delivered the import of a mask **from an image file**;
* [ADR 0071](0071-mask-overlay.md) delivered the overlay, without which a
  computed mask would be invisible.

Nothing is therefore missing in the engine. What is missing is **the socket**:
by what gesture a Studio user obtains a sky mask without going through an
export, a third-party tool and a manual import.

### The gesture retained, and the one that is not

Two forms exist in the software on the market:

* **automatic detection** — a button, "the sky", "the subject";
* **click selection** — one designates a point, and a SAM-style model segments
  what was designated.

The second is more powerful and covers both cases of the first by inversion. It
does however require an interactive interface to build — positive and negative
points, immediate feedback, re-encoding on every click — and an encoder running
before the first click. **Automatic detection is retained** (decided
2026-08-04): a button, a result, and no new interface to invent. Click
selection stays open, and the socket decided below does not close the door on
it — it is merely its simplest call.

## Decision

### 1. Nothing in the engine, and a crate apart for the socket

Not a line is added to `leyline-engine`: everything a detector needs is already
there (§Context). The socket lives in a **new open crate**, `leyline-detect`,
whose sole purpose it is:

```
Studio ─→ SDK ─→ leyline-detect ──(process)──→ a detector, whatever it is
```

That crate detects nothing. It holds **the contract, the discovery and the
invocation** — a few hundred lines, no heavy dependency, and `leyline-core` as
the only Leyline crate beneath it.

It is **re-exported by `leyline-sdk`**, as `leyline-map` and `leyline-export`
are: [`architecture.md`](../architecture.md) §Inside Studio requires that
Studio declare *one single* Leyline dependency, and that rule does not bend for
an accessory. The CLI obtains it by the same path, without copying a line.

### 2. A detector is an executable that turns an image into a mask

That is the structural decision, and it is deliberately **smaller** than what
ADR 0069 §2 envisaged.

```
detector --image <input.png> --detector <id> --out <output.png>
```

* **input**: the developed preview, rendered by Studio, as an 8-bit RGB PNG;
* **output**: a **16-bit grey** PNG, `0` = the setting does not apply,
  `65535` = it does — exactly the format ADR 0070 §4 froze;
* **the rest**: exit code `0`, and `stderr` to say why when it is not `0`.

Four consequences, and they are what justify the shape:

* **The detector touches neither the catalog nor the library.** It does not
  open it, takes no SQLite lock, and knows no identifier. It is *the open
  client* that then calls `store_mask_coverage` and writes the revision through
  an ordinary edit session. ADR 0069 §1's rule — an extension produces
  settings, never pixels — is held here **more strictly** than the ADR
  required: the detector does not even produce a setting, it produces an image
  the open code turns into a setting.
* **The detector need not link `leyline-sdk`.** ADR 0069 §4's GPLv3 §7
  additional permission stays useful for other forms of extension, but **this
  one does not put it in play**: two processes exchanging two PNG files do not
  form a combined work. The licence boundary is crossed by an `execve`, which
  is the hardest point one can reach.
* **Anyone can write one**, in twenty lines of Python, with whatever model they
  like. The socket therefore has a value of its own for the free project,
  independently of any paid product — which is what makes it legitimate in the
  open repository rather than cut for a vendor.
* **A detector that crashes does not kill Studio.** A dynamically loaded plugin
  would.

### 3. Discovery: one manifest per detector, in the user's configuration

A detector is installed by dropping a JSON manifest into
`<user config>/Leyline/detectors/<id>.json` — the same folder
`recent_libraries.json` already occupies (`directories::ProjectDirs`), and for
the same reason: **a library is portable and self-contained**
(`catalog.md` §37), it must not gain an ancillary file talking about software
installed on *this* machine.

```json
{
  "id": "leyline-assist",
  "label": "Leyline Assist",
  "command": "/opt/leyline-assist/leyline-assist",
  "args": ["detect"],
  "detections": [
    { "id": "sky",     "label": "Sky" },
    { "id": "subject", "label": "Subject" }
  ]
}
```

The list is named `detections`, not `detectors`: a manifest describes one
detector, and what it enumerates is what that detector can find. This ADR
wrote `detectors` here until 2026-08-31, when the first real detector was
built against the document rather than against the code and its manifest
was silently ignored — see [ADR 0105](0105-detector-conformance-and-cli.md)
§4, which also decided what to do about the silence.

A manifest that is unreadable, incomplete, or whose command does not exist is
**ignored** — not an error at startup: a detector is an accessory, and Studio
must launch without one. No manifest, no menu: the feature does not appear
rather than appearing greyed out.

### 4. What the gesture produces in the revision

A **new local adjustment**, with the detected mask and **neutral** values:
detection chooses *where*, the user chooses *what*. Creating a mask with an
exposure already set would guess the intent.

The mask is stored at the resolution the detector returned — ADR 0070 §5
already settled that a coverage need not follow the sensor's, and a model
working at 512×512 has nothing to gain from seeing its output upsampled before
being written.

### 5. The models: the **weights'** licence is eliminating, and it eliminates

The plan's condition 5 (§4) demands a GPL-3.0-compatible licence. Verified on
2026-08-04, and the result alone justifies having looked before coding:

| Model | Licence | Verdict |
|---|---|---|
| SegFormer ADE20K (NVIDIA) | *NVIDIA Source Code License-NC* | **Rejected** — non-commercial |
| RMBG-1.4 (BRIA) | its own licence, paid commercial | **Rejected** |
| U²-Net | Apache-2.0 | Retained (subject) |
| BiRefNet | MIT | Retained (subject) |
| MMSegmentation / PaddleSeg zoo (ADE20K, class *sky*) | Apache-2.0 | Retained (sky) |

The first two are the most visible and the easiest to find: exactly the trap the
condition exists to catch. Apache-2.0 and MIT enter a GPL-3.0-only work without
difficulty, in that direction.

The definitive choice of a model per detector, its conversion to ONNX and its
measurement are **not** settled here: they belong to the detector, and hence to
its own repository. What is settled here is the criterion and the fact that it
was verified before any line was written.

### 6. The first detector, and where it lives

`leyline-assist`: an executable, a private repository, two detections (sky,
subject), ONNX inference in **pure Rust** — the detector must build for Windows
without replaying the pain of
[ADR 0038](0038-tethered-capture.md), where `libgphoto2` ended up a disabled
feature for want of a mingw package. It is a private decision, recorded here
because it explains why the socket imposes no runtime: it knows of none.

The **weights are never in the open repository**, nor in the free AppImage —
the plan's condition 6. They come with the detector.

### 7. What is not settled here

* **The paid key system** — referred to ADR 0069 §5, which already put it out
  of scope and named its two constraints (offline verification,
  `specification.md` §4 to be corrected rather than worked around).
* **Click selection** (SAM). The socket above would make it possible without
  serving it: it passes files, not clicks. That will be another ADR, and
  probably another form of socket.
* **C1, AI denoising.** Still with no way out, and for a reason better stated
  since this ADR: a denoiser produces **pixels**. It can therefore neither be
  materialized once like a mask, nor cross ADR 0069's boundary, nor enter the
  render path without taking §5.1 with it.
  [ADR 0072](0072-measured-noise-profile.md) — the measured noise profile — was
  the partial answer planned in its place, and it is delivered.

## Consequences

* **One more crate, open and small**: `leyline-detect`,
  [`architecture.md`](../architecture.md) lists it. The engine, for its part,
  does not move a line — hence no stage version, hence §5.1 not concerned,
  hence no reference render to bless.
* **A masked photo stays an ordinary photo.** A detected mask is a stored
  coverage like any other: a build with no detector at all opens it, renders it
  and exports it identically. That is ADR 0069 §1, verified here by
  construction rather than promised.
* **The catalog's format does not move**: no schema, no `settings_json`, no
  stage version. `local_adjustments::v3` already renders coverages.
* **Studio gains one entry per discovered detection**, and nothing at all when
  no manifest exists — which is the case for every installation today.

## Alternatives rejected

* **A dynamically loaded plugin** in Studio. Forbidden by ADR 0069 §2 on the
  engine side, and pointless on the interface side: linking would make the free
  binary and the closed binary a combined work, where two processes do not even
  have the question to ask.
* **The detector opening the library itself** (what ADR 0069 §2 described: "it
  asks for a preview, computes, and writes through an edit session"). Two
  processes on the same `catalog.db` would contend for a SQLite lock the
  project has already spent two ADRs narrowing
  ([ADR 0023](0023-catalog-lock-narrowing-preview.md),
  [ADR 0024](0024-catalog-lock-narrowing-export.md)), and the detector would
  have to know the data model in order to write a revision. Passing two PNGs
  removes both problems at once.
* **Two builds of Studio**, free and paid. That is the clone ADR 0069 refuses,
  moved up a notch: the same perpetual cherry-picking, and an interface that
  diverges.
* **Embedding a model in the open repository.** Several hundred megabytes for
  an optional feature (condition 6), and a core feature that would depend on
  the weights (condition 2).
* **Waiting for click selection** so as to ship only once. The button covers
  the most frequent case — a landscape's sky — and the socket it requires is
  the one a future interactive tool will reuse for its own purposes.
