# ADR 0056 — EXIF metadata of non-RAW files at import

**Status:** Accepted — 2026-08

## Context

At import, Leyline fills the `metadata` table only when LibRaw has been able to
read the file (`leyline-engine/src/import.rs`, `exif_metadata(&raw)`). A JPEG,
a TIFF or a PNG imported — which is what everyone does when arriving with an
existing library, and what Leyline's own export produces besides — therefore
have **no metadata at all** in the catalog:

* the details panel shows `—` for the body, the lens and the exposure;
* develop's shot strip ([ADR 0054](0054-first-run-and-basic-mode.md) §2) does
  not appear;
* **the capture date is empty**, so the default sort (by capture date) places
  those photos together, at the end of the list, whatever the real date;
* full-text search finds them neither by body nor by lens.

That is not a tenable position: the file contains the information, and Leyline
does not read it. A RAW developer may refuse to *develop* a JPEG; it cannot
claim not to know when it was taken.

## Decision

### 1. An EXIF reader for the files LibRaw does not read

Import reads the EXIF of non-RAW files and fills the same `metadata` table,
with the same fields, as the RAW path. **The source depends on the file, not
the destination**: nothing changes in the catalog, in the queries, nor in what
the interface displays.

The precedence rule is unambiguous: **if LibRaw read the file, LibRaw is
authoritative**, and the EXIF reader does not run. It is the identification of
the decoder that renders the photo, and it must stay the one displayed. The
EXIF reader therefore intervenes only where there is nothing today.

### 2. `kamadak-exif`, in the engine, beside the XMP reader

The dependency is `kamadak-exif`: pure Rust, without `unsafe`, read-only, under
a licence compatible with the project's GPL-3.0, and covering the containers
that concern us (JPEG, TIFF, PNG, WebP, HEIF).

It is consumed from a `leyline-engine/src/exif.rs` module, twin to `xmp.rs`:
the same shape, the same status. Neither a new crate — there is no new
responsibility, only a second source for data already modelled — nor an
addition to `leyline-raw`, whose role is LibRaw and nothing else.

### 3. What is read

The fields `Metadata` already carries and no more: body (make, model), lens,
sensitivity, shutter speed, aperture, focal length, exposure compensation,
flash, white balance mode, colour space, orientation, GPS position, artist,
copyright — plus the **capture date**, which is not in `Metadata` but in the
asset itself, and which is the most visible datum of this document's first five
lines.

A field absent from the file stays absent from the catalog. **No value is
invented, and no default value is written.**

### 4. The time, and the only thing we know about it

`DateTimeOriginal` is a local time with no zone. When `OffsetTimeOriginal`
(EXIF 2.31) is present, it is applied: the stored instant is the true UTC
instant, and the offset is kept in `capture_offset_minutes`, a column that
exists for exactly that. When it is absent, the time is taken as it is and the
offset stays unknown — the same convention as the RAW path, which already makes
that choice. A local time recorded as such and flagged as unknown is honest; a
local time shifted by a guessed zone is not.

### 5. Best effort, never blocking

EXIF that is absent, truncated or aberrant **does not fail the import**: the
photo enters the catalog without metadata, exactly as today. It is the rule the
thumbnail and the XMP sidecar already follow at the same place
([ADR 0047](0047-xmp-sidecar-read.md)) — a file's import does not turn on a
block of metadata.

### 6. Read-only, definitively

Leyline never rewrites a source file's EXIF. That is the non-destructive
principle (`docs/vision.md`), and it is not negotiable here: the project's only
metadata writing stays the export, which produces a new file.

## Out of scope

* **Applying EXIF orientation to the rendering** of a displayed JPEG.
  Orientation is now *read* and stored; whether it is *applied* by the display
  pipeline is another subject, with its own stage-version question.
* **Proprietary metadata** (MakerNotes): shooting modes, AF points, the
  manufacturer's lens corrections. Every brand has its own format, and nothing
  here depends on any of it.
* **A resynchronization of already-imported photos.** JPEGs imported before
  this decision stay without metadata until a re-import; re-reading their EXIF
  after the fact presupposes deciding what wins in case of conflict with what
  the user has entered meanwhile, which is a synchronization decision, like the
  one ADR 0047 explicitly refused to take.
* **Writing EXIF**, into the source file as into a sidecar (§6).

## Consequences

* **An imported JPEG at last has a date, a body and an exposure**: it sorts
  with the others, is searched like the others, and shows develop's shot strip.
* **One more dependency** in `leyline-engine`, read-only and without `unsafe`
  (`docs/architecture.md` §External bricks is updated).
* **`docs/catalog.md` gains a sentence**: `capture_offset_minutes` now has a
  producer.
* **No change of schema, of rendering, of pipeline nor of stage version.** A
  file imported before and after this decision *develops* identically.

## Alternatives rejected

* **Having LibRaw read the JPEGs.** LibRaw opens some non-RAW files, but its
  identification there is partial and its cost is that of a complete RAW
  decoder in order to read four numbers.
* **Writing our own EXIF reader.** The format is a swamp of per-manufacturer
  special cases; it is exactly the kind of brick ADR 0037 agreed to write by
  hand *because it was tiny and framed* (a few DCP tags), which is not the case
  here.
* **Reading EXIF at display time rather than at import.** The catalog exists so
  as not to reopen 60,000 files on every sort.
* **Replacing LibRaw with the EXIF reader everywhere**, so as to have a single
  path. The identification of the decoder that renders the image is the one
  that must be displayed beside it (§1).
