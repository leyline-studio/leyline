# System requirements

**Document:** `docs/system-requirements.md`
**Version:** 1.0
**Status:** Reference

---

# 1. Purpose

This document answers: **what machine does Leyline run on, and from what floor?**

Every value below is **measured**, never estimated from a plausible order of magnitude. The measurement conditions are in §2.1 and the procedure to reproduce them in §7: a figure in this document that no longer reproduces is a figure to correct, not to keep.

Out of scope here: the **build** prerequisites (pinned Rust toolchain, LibRaw, Lensfun, LittleCMS, nasm), which are in [`contributing.md`](contributing.md), and the contents of the installers, which are in [ADR 0019](adr/0019-distribution-i18n.md).

---

# 2. The limiting factor is memory

It is not the processor. **One** photo's pipeline cannot fill a modern machine — a batch of 12 files at 30 Mpx measured 280 % of 1,600 % on sixteen threads ([ADR 0068](adr/0068-concurrent-export-batch.md)) — but every photo in flight ties up several hundred megabytes of floating-point buffers. Adding cores speeds things up little; running out of memory makes the machine page, and paging costs far more than concurrency ever returns.

Hence the sizing rule:

```
RAM ≈ 300 MB (Studio and its catalog)
    + degree × (Mpx / 30) × 700 MB
```

where `degree` is the number of photos an export batch processes at a time ([§3.1](#31-the-export-degree-follows-the-machine)).

## 2.1 The measurements

Conditions: `--release` build of 2026-08-25, Intel i9-9900K (8 cores / 16 threads), 32 GiB available, Linux. Corpus: three Canon 5D IV CR2 files (30 Mpx, ~65 MB each) and 199 JPEGs of varied origin.

| Action | Peak memory | Time |
|---|---|---|
| Studio open on a library, idle | **178 MB** | — |
| Importing 3 RAW files (EXIF + thumbnails) | 313 MB | 3.2 s |
| Importing 199 JPEGs | 133 MB | 8.7 s |
| `small` preview — 1024 px, the develop view | 220 MB | 1.3 s |
| `large` preview | 665 MB | 5.5 s |
| `full` preview — full resolution | 990 MB | 11.5 s |
| Exporting 3 JPEGs, `--concurrency 1` | 760 MB | 7.1 s |
| Exporting 3 JPEGs, `--concurrency 2` | 1.26 GB | 5.0 s |
| Exporting 3 JPEGs, `--concurrency 4` | **1.80 GB** | 3.1 s |

Two readings of that table:

* **An export's peak is proportional to the degree**, linearly: ~700 MB per photo in flight at 30 Mpx, past a gigabyte at 45 Mpx. It is the only line that can bring a machine to its knees.
* **A full-resolution preview costs almost as much as an export**, and it is a gesture the user makes without thinking. A machine sized to the bone must be able to absorb that gigabyte.

Interactive rendering, by contrast, weighs nothing: a slider dragged on an already-open photo redraws in 11 to 15 ms from the cached proxy ([ADR 0074](adr/0074-live-preview-while-dragging.md), [ADR 0076](adr/0076-proxy-cache.md)), and the caches that make it possible are bounded — 64 MB for proxies, ~144 MB for decodes.

---

# 3. The configurations

| | **Minimum** | **Recommended** |
|---|---|---|
| Processor | baseline x86-64, 2 cores | 4 cores / 8 threads |
| Memory | **4 GB** | **16 GB** |
| Display | 1024 × 700 | 1920 × 1080 |
| Graphics | OpenGL ES 2.0, or Mesa llvmpipe | a hardware GPU |
| Disk | 100 MB plus the cache (§6) | an SSD |

**No recent instruction set is required.** The project sets neither `target-cpu` nor `target-feature`: the binaries are compiled for baseline x86-64 (SSE2). A processor without AVX2 runs Leyline. On macOS, the target is arm64.

**The GPU computes no pixels.** It only draws the interface. The question of a GPU pipeline has been asked twice and rejected twice: by [ADR 0012](adr/0012-rayon-data-parallelism.md) (inter-GPU determinism not guaranteed), then again on 2026-08-03 ([`measured-findings.md`](measured-findings.md) §B3), once interaction had been made fluid on the CPU. Not because the promise forbids it — [ADR 0080](adr/0080-the-promise-and-its-boundary.md) §3 settled that a GPU operator is simply a new stage version — but because nothing was left to gain. A more powerful graphics card therefore accelerates **nothing** about development today.

## 3.1 The export degree follows the machine

The default degree is `min(4, available_parallelism())` ([ADR 0068](adr/0068-concurrent-export-batch.md) §1): a two-core machine processes two photos at a time, not four. That is what makes the 4 GB of the "minimum" column tenable — two 30 Mpx photos in flight ask for ~1.3 GB, four ask for 1.8 to 2.4.

On a machine at the floor **and** a corpus beyond 30 Mpx, explicitly dropping to `--concurrency 1` remains the right reflex: a sequential batch is slow, a batch that pages is far slower.

Conversely, a large machine gains by raising it: 6 photos in flight take 3.4× instead of 2.8×, for ~3.3 GB of peak.

---

# 4. Operating systems

| Platform | Floor | Delivered as |
|---|---|---|
| Linux | glibc **2.38** — Ubuntu 24.04, Debian 13, Fedora 39 or newer | AppImage |
| Windows | Windows 10 | NSIS installer |
| macOS | arm64 | `.dmg` |

The Linux floor is not an architectural decision: it is the glibc of the machine that built the AppImage. Building it on an older distribution lowers it accordingly, without changing a line of code.

It is also not set by Leyline's own code. Measured on the 0.1.0 AppImage: the Rust binary asks for nothing above glibc 2.35 (its two 2.39 symbols, `pidfd_spawnp` and `pidfd_getpid`, are weak references the standard library falls back from). The 2.38 comes from the C libraries the AppImage carries — `libraw_r` built on the packaging machine, and `libgomp` and `libltdl` copied from it — which the C23 headers of glibc 2.38 bind to `__isoc23_strtol` and its siblings. This page used to say 2.35, the binary's own figure, for as long as nobody had looked inside the package; §7 gives the command that looks.

---

# 5. Display and graphics card

Studio declares `min-width: 900px` and `min-height: 600px`, and its window opens at two thirds of the detected screen — **raised** to those bounds when two thirds would be smaller, then **lowered** to what the screen can actually show, which is the clamp that decides. A window is therefore never larger than the display it opens on.

Below 1340 logical pixels of width the side panels fold away on their own, and the interface stays complete without them; above it they are shown, and the window opens wide enough for them whenever the screen can afford it. Measured, on 2026-08-29:

| Screen | Window | Side panels |
|---|---|---|
| 1024 × 768 | 900 × 600 | folded |
| 1280 × 720 | 900 × 600 | folded |
| 1366 × 768 | 911 × 600 | folded |
| 1600 × 900 | 1340 × 600 | shown |
| 1920 × 1080 | 1340 × 720 | shown |
| 2560 × 1440 | 1707 × 960 | shown |

The previous text on this line claimed a floor of 1024 × 700 and that "a 1024 × 768 display works". Neither was true: at 1024 the right-hand panel was pushed clean off the window — in English before French, whose longer labels only widened the band where it happened — and the floor could make the window *taller* than a 720-pixel screen has left once the desktop's own furniture is out.

The interface is rendered through Slint and its femtovg renderer, which requires **OpenGL ES 2.0**. Without a hardware GPU, the correct path is Mesa's software OpenGL driver:

```bash
LIBGL_ALWAYS_SOFTWARE=1 leyline-studio
```

**What is not the correct path: `SLINT_BACKEND=winit-software`.** Slint's software renderer starts without error and draws **no `Path` element at all**: mask overlays and histogram outlines disappear, silently, with nothing to signal that the display is incomplete. A silent defect is worse than a refusal to start; that backend must not be presented as a fallback.

---

# 6. Disk

## 6.1 The installation

The AppImage weighs 26 MiB and the Windows installer 30 MB, world basemap included ([ADR 0059](adr/0059-bundled-world-basemap.md) — 9 MB of z0–5 tiles, embedded so that the map view never issues a network request).

## 6.2 The library

The photos dominate everything else. What Leyline adds on top, measured:

| Item | Cost | For 15,000 photos |
|---|---|---|
| SQLite catalog | 1.7 kB/photo (plus 264 kB of schema) | ~26 MB |
| Thumbnails | 86 KiB/photo | ~1.3 GB |
| Previews of an **edited** photo | ~2 MB | proportional to edited photos only |

A 1024 px preview weighs 0.68 MB, a 2048 px one 2.47 MB, a 4096 px one 9.03 MB. The preview cache is bounded by a window of three revisions plus the heads ([ADR 0075](adr/0075-preview-cache-retention.md)), so it grows with the number of photos **actually edited**, not with the size of the library.

A `full` preview is the exception: it is kept as is and weighs several tens of megabytes per photo. Generating them over thousands of photos is the one gesture that makes the cache explode.

## 6.3 Referencing instead of copying

`leyline import --reference` registers files without copying them into `Photos/`, which avoids doubling the disk footprint. The constraint: **the files must already sit under the library root**, since the catalog stores nothing but paths relative to that root ([ADR 0010](adr/0010-relative-paths.md)). A file located elsewhere is skipped, with the reason `file is outside the library root`.

---

# 7. Reproducing the measurements

Every figure in §2.1 reproduces with the repository's binaries, with no special harness:

```bash
# Peak memory and time of any command
/usr/bin/time -f "%M KiB  %e s  %P CPU" leyline-cli export <lib> <dest> 1 2 3 --concurrency 4

# Studio idle, with no physical screen
Xvfb :77 -screen 0 1920x1080x24 &
DISPLAY=:77 leyline-studio <lib> &
grep VmHWM /proc/$!/status

# Growth of the catalog and of the thumbnails
ls -l <lib>/catalog.db && du -sb <lib>/Cache/thumbnails

# Linux floor (§4): the highest glibc any file in the AppImage requires
leyline-studio_*.AppImage --appimage-extract >/dev/null
find squashfs-root -type f \( -name '*.so*' -o -name leyline-studio \) \
  -exec objdump -T {} \; | grep -v ' w ' | grep -oE 'GLIBC_[0-9.]+' | sort -uV | tail -1
```

The measurement taken under `Xvfb` goes through llvmpipe: it is therefore also the figure for a machine with no hardware GPU.

---

# 8. Related documents

* [`architecture.md`](architecture.md) — the crates and external building blocks these requirements come from.
* [`contributing.md`](contributing.md) — the build prerequisites, distinct from the runtime ones.
* [ADR 0068](adr/0068-concurrent-export-batch.md) — the original measurement of what a photo in flight costs in memory.
* [ADR 0075](adr/0075-preview-cache-retention.md), [ADR 0076](adr/0076-proxy-cache.md) — what bounds the caches.
* [ADR 0019](adr/0019-distribution-i18n.md) — the per-platform installers.
