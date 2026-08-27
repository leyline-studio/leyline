# ADR 0040 — The GPS map view: offline MBTiles

**Status:** Accepted — 2026-07

## Context

[[studio-workflow-gaps-progress]] left the GPS map view as the only
Lightroom/Darktable gap deliberately unsettled, for want of a solution
compatible with the Local-First principle (`docs/vision.md`): no cloud, and no
dependence on a third-party service that has to stay reachable.

A map needs two things: geographic data (roads, coastlines, places) and a way
to display them. The common solutions almost all rest on a tile server queried
over HTTP at run time (public OpenStreetMap, MapTiler, Mapbox…) —
incompatible with Local-First as they stand: Leyline must never depend on a
network service in order to work.

## Decision

**Pre-downloaded OpenStreetMap tiles, in MBTiles format, supplied by the user
— and never a network call from Leyline itself.**

* **Data**: OpenStreetMap (ODbL licence, free, with the `© OpenStreetMap
  contributors` attribution mandatory and displayed on the map). The user
  downloads or generates a tile pack for the region that interests them (say,
  [Geofabrik](https://download.geofabrik.de) extracts plus a renderer like
  `tilemaker`, or an already-rendered MBTiles pack) — **outside Leyline's
  scope**, treated exactly like DCP camera profiles (ADR 0035): a file the
  user brings, never a network dependency hidden in the product.
* **Format**: MBTiles — a single SQLite file containing the tiles
  (`docs/catalog.md`-style: `tiles(zoom_level, tile_column, tile_row,
  tile_data)` plus `metadata(name, value)`). `rusqlite` is already a project
  dependency (`leyline-catalog`); reading an MBTiles is a handful of SQL
  queries, with no extra library.
* **A new `leyline-map` crate**: an MBTiles reader alone, with the same single
  responsibility as `leyline-catalog`/`leyline-preview` — it opens the file,
  serves a `(z, x, y)` tile as raw bytes (PNG/JPG depending on the pack), and
  reads the metadata (bounds, min/max zoom, attribution). It knows nothing of
  the catalog nor of map rendering — just tile access, consumed by
  `leyline-engine`.
* **Where the pack lives**: a file convention, not a new catalog column.
  `Library::import_map_pack(source)` copies the chosen `.mbtiles` to
  `<root>/Map/pack.mbtiles`, alongside `Photos/`/`Cache/`/`Exports/`/`Backups/`
  (`docs/catalog.md` §3). One active pack at a time in V1 — no `library` table
  to migrate, the file's presence being authoritative.
* **GPS points**: `docs/catalog.md` §13 (`metadata.gps_latitude`/
  `gps_longitude`/`gps_altitude`) already existed in the schema but was never
  filled — no EXIF GPS extraction existed. LibRaw exposes the coordinates
  already parsed (`other.parsed_gps`, degrees/minutes/seconds plus an N/S/E/W
  reference): new accessors in `leyline-raw/src/shim.c`, with the DMS →
  decimal degrees conversion on the Rust side (never in C, the same choice as
  the rest of the shim: C only reads fields, and all the logic stays in Rust).
  Like the rest of EXIF metadata today, GPS extraction covers only RAWs
  identified by LibRaw — JPEG and TIFF have never had camera metadata either,
  the same pre-existing limit, not a regression introduced here.
* **Rendering**: Slint has no tile canvas; rendering is done on the Rust side,
  in the same shape as the existing develop canvas (the same settings, the
  same image composed in memory) — the visible window is composed in RGBA from
  the tiles covering the viewport and then published as a `slint::Image`, and
  pan/zoom recomposes the image on every gesture. No vector rendering: MBTiles
  tiles are already pre-rendered raster images.

## Consequences

* `docs/specification.md` §Included gains "GPS map view (offline MBTiles tiles
  supplied by the user)".
* A new `leyline-map` crate, consumed by `leyline-engine`, at the same
  dependency rank as `leyline-catalog`/`leyline-preview`
  (`docs/architecture.md`).
* `docs/catalog.md` §13: the GPS columns, present in the schema from the start
  but never used, are now actually populated at import for RAW files.
* Studio gains a Map view; **the CLI and the SDK deliberately expose no map
  rendering** — it is a visual surface, like the develop canvas, not a
  scriptable operation. `Library::import_map_pack`/`map_pins`/`map_tile` stay
  reachable from the SDK for a future client, but no CLI rendering command is
  added.
* With no pack imported, the Map view shows an empty state inviting the user
  to import one — never a network fallback, and never a placeholder map that
  would give the illusion of a connection. *Amended by
  [ADR 0059](0059-bundled-world-basemap.md)*: the fallback is now an embedded
  world basemap, and therefore offline like the rest. The empty state survives
  only in a build without the `bundled-basemap` feature.

## Alternatives rejected

* **OSM tiles over HTTP at run time** (public servers, or paid ones like
  MapTiler/Mapbox): rejected outright — it violates Local-First, and using the
  public OSM tile servers is in any case subject to a strict usage policy
  incompatible with a distributed product.
* **MapLibre** (the free fork of Mapbox GL): no mature Rust binding compatible
  with Slint; `maplibre-rs` exists but stays experimental (WebGPU), a large
  integration risk for a rendering gain (styled vector tiles) V1 has no need
  to pay for — raster MBTiles tiles suffice to display pins on a map.
* **Bundling a default regional tile pack**: rejected — a region at useful
  resolution weighs hundreds of megabytes to several gigabytes, incompatible
  with a light installer; letting the user choose their own region is also
  more respectful (no download imposed on first launch). **That wording,
  originally, said "a default pack" without qualifying its resolution, and
  therefore went too far**: a *world* basemap at low zoom costs only 9 MB, and
  [ADR 0059](0059-bundled-world-basemap.md) has since embedded it. What stays
  rejected here, and stays so, is shipping regional detail.
* **Storing the pack's path in the catalog** (a new `library` column or
  table): rejected for V1 — a file convention (`Map/pack.mbtiles`) suffices as
  long as one active pack at a time is supported, and it avoids any schema
  migration for this ticket. To be revisited if multi-pack (several active
  regions) becomes a real need.
