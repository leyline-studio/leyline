# ADR 0157 — What the exported file says about the photograph

**Status:** Accepted — 2026-10

## Context

An exported JPEG leaves Leyline knowing nothing about itself. `leyline-export`
embeds the sRGB profile (`lib.rs`, the `encode` match) and nothing else: no
capture date, no camera, no exposure, no author, no copyright, no keyword.
Everything the photographer typed — the descriptions of
[ADR 0099](0099-authored-descriptions.md), the creator of
[ADR 0147](0147-what-the-file-says-about-its-author.md), the keyword tree of
[ADR 0134](0134-keywords-in-the-interface.md) — reaches the **sidecar** (`leyline-engine/src/xmp.rs`)
and stops there. A file sent to a client, uploaded to a gallery or dropped on a
phone arrives anonymous, sorted by the date it was written rather than the date
it was taken. Every comparable tool writes this metadata by default; its
absence is the first thing a photographer notices on the receiving side.

The WebP branch does not even embed the ICC profile, which every other branch
does — found while reading the code for this ADR.

Three constraints shape the decision:

* **§5.1 promises the exported file, bit for bit** (`pipeline.md` §5.1), and
  independently of the application version. An export recipe written last
  month and replayed on the next build must produce the same bytes. Adding
  metadata to every export changes those bytes.
* **The location is private by nature.** 9 626 photographs of the reference
  library carry a GPS position ([ADR 0150](0150-a-map-that-opens-where-the-photograph-is.md));
  many are taken at home. An export is the one moment a photograph leaves the
  machine.
* **The AVIF encoder cannot carry metadata.** `ravif` returns a finished file
  (`EncodedImage::avif_file`); `avif-serialize` knows `set_exif`, but `ravif`
  offers no way to reach it.

## Decision

### 1. One policy, recorded with the export settings

`ExportSettings` gains `metadata: ExportMetadata`:

| Value | Writes |
|---|---|
| `Full` | everything in §2 |
| `WithoutLocation` | everything in §2 except GPS and the `photoshop:` city, state and country |
| `None` | nothing but the ICC profile — today's behaviour |

It is a recorded export parameter like `avif_speed`, **always serialized**, so
the file a recipe produces does not depend on a default that may move.

Two defaults, deliberately different:

* **A recipe written before this ADR deserializes to `None`.** Its parameters
  are "strictly identical" in the sense of §5.1 only if it goes on producing
  the same bytes; reading a missing field as anything else would make an old
  preset change its output silently under a new build.
* **A new export — the Studio dialog, the CLI, `ExportSettings::default()` —
  starts at `WithoutLocation`.** Publishing where one lives is a choice one
  makes, not one a default makes for them. `Full` is one click or one flag
  away and the dialog says what it adds.

### 2. What is written

From `metadata` (what the file said) and `asset_descriptions` (what the
photographer wrote), authored winning over read as in ADR 0099 §2:

* **Capture**: date and time of capture with its offset when known, make,
  model, lens, ISO, shutter, aperture, focal length, exposure bias, flash.
* **Authorship**: creator, copyright, credit, title, caption.
* **Keywords**: the flat `dc:subject` list and `lr:hierarchicalSubject`, the
  same two the sidecar writes.
* **Location** (`Full` only): GPS latitude, longitude, altitude; city, state,
  country.
* **Orientation = 1**, always: the pixels are already upright, and copying the
  source orientation would rotate them a second time.
* `Software = "Leyline"`, without a version number.

Not written: rating, flag and colour label (judgements of a rendering inside
one library, [ADR 0099](0099-authored-descriptions.md) §1), develop settings,
and the RAW maker notes. Provenance — which revision, which decoder — is the
subject of the planned ADR 0153 and will travel in the same XMP packet under
its own `leyline:` namespace, subject to this same policy (`None` strips it).

### 3. No clock, no randomness

The packet carries **no** `xmp:MetadataDate`, `xmp:ModifyDate`, export date,
`xmpMM:InstanceID` or `DocumentID`. Each would make two identical exports
differ, the lesson of the ICC profile's creation date
([pipeline.md](../pipeline.md) §5.1). The export date already lives in the
catalog (`export_history`). Fields are written in a fixed order and the XMP is
produced by our own writer, so the same inputs give the same bytes.

### 4. The metadata is an input of the export

§5.1 lists what must be equal for two exports to be bit-identical. Under
`Full` or `WithoutLocation`, the **metadata and descriptions as they stand at
export time** join that list: editing a caption changes the next export's
bytes without touching its pixels. `pipeline.md` §5.1 says so in the same
change.

### 5. Per format, and AVIF said out loud

| Format | Carrier |
|---|---|
| JPEG | APP1 `Exif` segment + APP1 XMP segment (`add_app_segment`) |
| PNG | `eXIf` chunk + `iTXt` `XML:com.adobe.xmp` |
| WebP | `set_exif_metadata` + `set_xmp_metadata` — and `set_icc_profile`, closing the gap above |
| TIFF | baseline tags in IFD 0 (Make, Model, Artist, Copyright, DateTime, Orientation) + tag 700 (XMP) |
| AVIF | **none** |

The EXIF block is a small TIFF structure written by `leyline-export` itself —
a dozen tags, no new dependency, deterministic by construction. XMP carries
every field of §2 (`exif:`, `tiff:`, `dc:`, `photoshop:`, `lr:`), so a reader
that only reads XMP misses nothing.

An XMP packet that would not fit one JPEG APP1 segment (65 502 bytes —
thousands of keywords) is a **named export error**, never a truncation.

AVIF writes nothing because the encoder offers no way to, and that is
**shown, not silent**: `ExportFormat::carries_metadata()` answers `false`, the
Studio dialog greys the policy with a line saying why, the CLI prints it once
per batch, and the MCP export tool reports it. The policy is still recorded,
so a later encoder that can carry metadata changes AVIF output only through a
new, explicit decision.

## Consequences

* Exports become self-describing in every format but AVIF; galleries and
  phones sort them by capture date.
* `ExportSettings` grows one field in the engine, the CLI (`--metadata
  full|without-location|none`), the Studio export dialog and the MCP export
  tool, at parity.
* `pipeline.md` §5.1 gains the metadata as an input; `specification.md` §3
  lists this ADR until it is built.
* WebP exports gain their ICC profile: a WebP exported before and after differs
  even under `None`. Accepted — the earlier file was the defect, and no WebP
  recipe could have relied on a missing profile meaning anything.
* Tests: a round trip per format read back with `kamadak-exif` (already in the
  tree) and `roxmltree`; `WithoutLocation` and `None` leave no GPS byte; two
  exports of one revision are bit-identical; an old recipe without the field
  still produces today's bytes.

## Alternatives rejected

* **`Full` by default.** What most tools do, and how home coordinates end up
  on a public gallery. The cost of the safer default is one click.
* **Old recipes read as the new default.** Simpler serde, and a silent change
  of every existing preset's output — the exact thing §5.1 forbids.
* **A metadata crate** (`img-parts`, `little_exif`). The writing is a dozen
  tags and four carriers each encoder already exposes; a dependency would buy
  generality we do not need and a byte order we do not control.
* **Copying the source EXIF block wholesale.** It carries maker notes, the
  source orientation, thumbnails and serial numbers; one rotates the image
  twice and the rest leaks more than the photographer chose.
* **Re-wrapping AVIF ourselves** with `avif-serialize` around the AV1 payload.
  `ravif` does not expose the payload; forking the encoder for metadata is out
  of proportion. Revisit when upstream offers EXIF.
* **Truncating an oversized XMP packet.** Losing keywords without a word is
  worse than an export that says why it stopped.
