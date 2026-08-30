# Specification

This document answers: **what does Leyline do, and what will it never do**. It also says what is actually delivered — the phase-by-phase state is in [`roadmap.md`](roadmap.md).

Every feature listed as delivered is delivered on all **three clients**: Studio, the CLI and the SDK.

---

## 1. V1 scope — delivered and closed

**Library and catalog**

* Importing a folder (RAW, DNG, JPEG, PNG, TIFF), by copy or by reference — a referenced file stays where it is, provided it already sits under the library root ([ADR 0010](adr/0010-relative-paths.md))
* SQLite catalog — libraries, collections (manual and dynamic), keywords, ratings, colour labels, pick status
* Removing photos from the catalog, or deleting them from disk to the system trash — two distinct gestures ([ADR 0060](adr/0060-asset-removal.md))
* Cached thumbnails and previews
* EXIF reading, full-text search
* XMP sidecars: written on demand, and read as a starting point at import — the migration path from another piece of software ([ADR 0047](adr/0047-xmp-sidecar-read.md))
* USB tethered capture — every photo imported as it is shot, with live view, the body's exposure settings, a remote release and a develop preset applied on arrival ([ADR 0038](adr/0038-tethered-capture.md), [ADR 0087](adr/0087-tethered-capture-bar.md))
* Automatic import from a watched folder ([ADR 0039](adr/0039-watched-folder-import.md))
* GPS map view: an embedded world basemap ([ADR 0059](adr/0059-bundled-world-basemap.md)), refined by an offline MBTiles pack the user brings if they want detail; no network call whatsoever ([ADR 0040](adr/0040-gps-map-view.md))

**Non-destructive development**

* Exposure, white balance, contrast
* Shadows / highlights, whites / blacks
* Vibrance / saturation
* Rotation, cropping
* Choice of demosaic algorithm: AHD, VNG, DCB, DHT ([ADR 0061](adr/0061-demosaic-algorithm.md))
* Noise reduction (luminance and chroma, edge-preserving — [ADR 0046](adr/0046-edge-preserving-denoise.md) — at the threshold that comes from the **measured** noise profile of the camera body at that sensitivity, [ADR 0072](adr/0072-measured-noise-profile.md)), sharpening
* Lens correction through Lensfun: distortion, vignetting, transverse chromatic aberration ([ADR 0016](adr/0016-process-3-lens-correction.md), [0017](adr/0017-process-4-vignetting.md), [0018](adr/0018-process-5-tca.md))
* Colour management through LittleCMS ([ADR 0015](adr/0015-color-management-srgb.md), [ADR 0027](adr/0027-color-management-beyond-srgb.md))
* Develop presets: create, apply, apply in batch ([`presets.md`](presets.md), [ADR 0014](adr/0014-develop-presets.md))
* Reprocessing a photo to the current stage versions

**Output**

* JPEG, TIFF, PNG, WebP, AVIF export — export presets, batch export
* Print module: physical dimensions, margins, destination profile ([ADR 0036](adr/0036-print-module.md))

**Distribution**

* Per-platform installer (Windows, macOS, Linux), with a choice of install folder where the platform allows it ([ADR 0019](adr/0019-distribution-i18n.md))
* Multilingual interface (French, English), extensible without a code change

---

## 2. Beyond V1 — delivered

These features were scoped as post-V1 candidates in [`v2-scope.md`](v2-scope.md). They are implemented.

| Feature | Decision |
|---|---|
| Tone curve (monotone cubic spline, applied on luminance) | [ADR 0030](adr/0030-tone-curve.md) |
| Spot removal (deterministic cloning, no *heal* mode) | [ADR 0032](adr/0032-spot-removal-clone.md) |
| Masked local adjustments — brush, radial, graduated | [ADR 0029](adr/0029-process-6-local-adjustments.md) |
| Range masks — a luminance band and a hue band, refining a geometric mask | [ADR 0048](adr/0048-range-masks.md) |
| HSL mixer and colour grading | [ADR 0031](adr/0031-hsl-color-grading.md) |
| Clarity, texture, dehaze | [ADR 0033](adr/0033-clarity-texture-dehaze.md) |
| DCP camera profiles — matrices, interpolated illuminants and tables ([ADR 0062](adr/0062-dcp-illuminant-interpolation.md), [ADR 0063](adr/0063-dcp-tables.md)) — **experimental** | [ADR 0035](adr/0035-camera-profile-dcp.md), [ADR 0037](adr/0037-dcp-parsing-dependency.md) |
| Perspective correction (two sliders, a homography) | [ADR 0052](adr/0052-perspective-correction.md) |
| Creative `.cube` LUT imported into the library, with an amount | [ADR 0053](adr/0053-creative-lut.md) |
| Clipped highlight reconstruction (`clip`/`blend`/`rebuild`, before demosaicing) | [ADR 0050](adr/0050-highlight-reconstruction.md) |
| Text watermark on export and soft proofing (view only) | [ADR 0034](adr/0034-softproofing-watermark-print.md), [ADR 0051](adr/0051-watermark-rasterization-and-soft-proof-surface.md) |
| RAW+JPEG pairing: the JPEG written by the camera becomes the companion of the RAW from the same shot, one photo in the grid instead of two | [ADR 0079](adr/0079-raw-jpeg-pairing.md) |

Masked local adjustments and their range masks have been exposed in all three clients since [ADR 0049](adr/0049-local-adjustments-clients.md): drawing tools and an editor in Studio, the stored `LocalAdjustment` JSON payload in the CLI. What ADR 0049 left out of scope: the overlay of the coverage the engine computes (the "red mask") is **delivered** ([ADR 0071](adr/0071-mask-overlay.md)); what remains is the range eyedropper and drag handles on a geometry already drawn.

The word *experimental* is literal: the colorimetric accuracy of the DCP matrix path has not been validated against real Adobe `.dcp` files and their reference renders, and the `ProfileHueSatMapData` / `ProfileLookTableData` / `ProfileToneCurve` tables are not applied. Studio and the CLI say so to the user.

---

## 3. Decided, not implemented

| Subject | Decision |
|---|---|
| Image watermark (a logo) | Cut by [ADR 0034](adr/0034-softproofing-watermark-print.md): the asset-reference problem is not settled |
| A catalog spanning several volumes | Decided by [ADR 0085](adr/0085-named-roots.md): named roots, identified by a marker file inside the folder and located by an advisory hint outside the catalog. Until it is implemented, a library covers one tree, and photographs are catalogued where they already live by putting the library root above them |

---

## 4. Deliberate exclusions

These absences are **decisions**, not delays. They are not to be proposed as missing features.

They say what the delivered application does not contain. They are not
statements about what the project may ever build: what the user is guaranteed,
and what any future optional service would have to satisfy, is fixed by
[ADR 0080](adr/0080-the-promise-and-its-boundary.md).

| Excluded | Reason |
|---|---|
| Cloud | Nothing is hosted: the library is a folder on your disk, and it is the original. A shared catalog, if one ever existed, would be a copy and an option — never the source of truth ([ADR 0080](adr/0080-the-promise-and-its-boundary.md) §4) |
| User accounts | The application authenticates against nothing, because no delivered feature needs a server. No account can ever become the condition of what is already installed |
| Subscription | The application is not sold by subscription, does not expire and holds no licence check. What could be sold one day is hosting or an extension ([ADR 0069](adr/0069-closed-extension-boundary.md)), never the right to run what you have |
| Artificial intelligence | Out of scope: **no model is shipped, no inference takes place**. What exists since [ADR 0073](adr/0073-external-mask-detectors.md) is a *socket* — a third-party mask detector, installed separately, may propose a coverage that the free pipeline then renders like any other mask. An optional local AI remains conceivable in the very long term |
| HDR | A whole feature, to be settled by an ADR of its own when the day comes |
| Panorama | Likewise |
| Face recognition | Likewise, with a privacy dimension that demands its own decision |
| Automatic synchronisation | Contradicts Local First |

The reasoning about what deserves to be built after V1 — and about what was deliberately cut — is in [`v2-scope.md`](v2-scope.md) and [`v2-implementation-plan.md`](v2-implementation-plan.md).
