# ADR 0059 — An embedded world basemap

**Status:** Accepted — 2026-08

## Context

[ADR 0040](0040-gps-map-view.md) rejected the idea of shipping a default tile
pack, on the grounds that "a single region at useful resolution weighs hundreds
of megabytes to several gigabytes, incompatible with a light installer". The
reasoning was right, but it bore on a **regional pack at useful resolution**.
It never weighed the case of a **world basemap at low zoom**, which is an
object of another nature: one does not locate a photo to the street on it, one
sees the continents, the coastlines and the relief, which is enough to give GPS
pins a context.

The consequence of that absence: the Map view opens on an empty screen until
the user has found, downloaded and imported a `.mbtiles`. A shipped feature
nobody sees working on first launch.

The weight was measured before deciding, not estimated — Web Mercator tiles
rendered from the Natural Earth I raster (21600 × 10800), by GDAL:

| Depth | Tiles | JPEG q80 | 32-bit PNG | 8-bit PNG |
|---|---|---|---|---|
| z0–5 | 1,365 | **9.0 MB** | 26.5 MB | 73.0 MB |
| z0–6 | 5,461 | 29.8 MB | 90.7 MB | 253.9 MB |

For comparison, the AppImage weighs 16 MB and the Windows installer 22 MB;
darktable, the direct comparable, weighs 108.

## Decision

**A z0–5 world basemap is embedded in Leyline Studio, as JPEG, and serves
whenever no user pack is active.**

* **Data**: Natural Earth I with shaded relief and water
  (`NE1_HR_LC_SR_W`), **public domain** — no licence to propagate, no
  attribution legally required, and above all no redistribution forbidden.
  Tiles rendered by the public OpenStreetMap server cannot be embedded: their
  usage policy forbids it. That is what rules OSM out as the source of a
  *shipped* pack, not as the source of a pack the user brings.
* **Depth and format**: z0–5, JPEG tiles at quality 80, 1,365 tiles, 9.0 MB.
  z6 would quadruple the weight for a level of detail the Map view does not
  need, and the 10m source tops out around z6 anyway.
* **Location**: `assets/basemap/world-z0-5.mbtiles`, versioned in the
  repository. A 9 MB binary in Git is an accepted cost: the pack is immutable,
  it will not be re-issued with every version, and the alternative
  (regenerating it at build time) would impose GDAL and 309 MB of source on
  anyone compiling the project.
* **Embedding**: `include_bytes!`, and opening **without extraction** through
  `sqlite3_deserialize` read-only (`Connection::deserialize_bytes`, rusqlite
  0.37). No copy into the user's library, no temporary file to clean up, and no
  write path on a library opened read-only. `leyline-map` gains a
  `TilePack::from_static`, beside `TilePack::open`: the same reader, the same
  queries, only the way the database is attached changes.
* **Scope**: behind a `bundled-basemap` Cargo feature of `leyline-engine`,
  off by default and re-enabled by Leyline Studio — exactly the arrangement of
  the `tether` feature. The CLI and the SDK render no map
  ([ADR 0040](0040-gps-map-view.md)): they have no reason to carry 9 MB of
  tiles.
* **Priority**: a pack imported by the user always wins. The embedded basemap
  is a *fallback*, never a blend: Leyline does not compose two packs in one
  view, it serves one.
* **Attribution**: the pack declares its own in its `metadata` table
  (`Natural Earth (public domain)`), and the interface already displays the
  active pack's. The hard-coded "© OpenStreetMap contributors" fallback stays
  for packs that declare nothing — most packs a user brings are derived from
  OSM, where attribution is mandatory — but it no longer applies to the
  embedded basemap, which would then be credited wrongly. No new API to
  distinguish the two packs: what the interface needs to say, the pack's `name`
  metadata already says.

## Consequences

* Real weight, measured afterwards on the built packages: the AppImage goes
  from 16 to **24 MB**, the Windows installer from 22 to **30 MB**. We stay
  under a third of darktable (108 MB).
* `packaging/windows/build-nsis.sh` compiles with `--no-default-features` to
  remove tethering: the feature must therefore be **explicitly renamed** there,
  otherwise the Windows installer ships without the basemap. It was measuring
  the package that revealed it, not a test.
* The Map view no longer has an empty state on first launch: it shows the
  world, and the GPS pins on it. The "Import a pack…" button does not disappear
  for all that — it becomes what it should always have been, the way to
  **refine**, not the prerequisite for seeing anything at all.
* `docs/specification.md` §1: the "GPS map view" line stops saying "tiles
  supplied by the user" without qualification.
* [ADR 0040](0040-gps-map-view.md) sees its "Bundling a default tile pack"
  alternative corrected in place: it stays rejected for a regional pack, it no
  longer is for a world basemap. That rewriting is permitted as long as the
  project is unpublished (`docs/adr/README.md`).
* Regenerating the pack one day requires GDAL and the Natural Earth raster; the
  exact recipe is in `assets/basemap/README.md`, so that nobody has to
  rediscover it.

## Alternatives rejected

* **Downloading the pack from the application** (a "choose your region" menu,
  with the client fetching the tiles): that is the initial request, and it is
  rejected for this slice. It contradicts head-on "never a network call from
  Leyline itself" ([ADR 0040](0040-gps-map-view.md)), which would need its own
  decision; it presupposes hosting and maintaining packs, hence bandwidth and
  an availability the project does not yet commit to; and it does not replace
  the embedded basemap, which is precisely what makes the map useful **without**
  a network. Nothing prevents taking it up later, on top.
* **A precision choice in the installer**: impossible to hold on all three
  platforms. The AppImage has no installation step, it is a file one launches;
  the `.dmg` is a drag and drop. Only NSIS could offer a components page, at
  the price of a hand-written `.nsi` template. A setting that exists on only
  one system in three is not a setting, it is an asymmetry to explain.
* **Embedding z0–6** (29.8 MB): three times the weight for one more zoom level,
  when the Map view serves to locate photos, not to navigate. The user pack
  stays the answer as soon as detail is wanted.
* **8-bit PNG**: the obvious "light" format proves eight times heavier than
  JPEG on this content (73 MB against 9), quantization sitting poorly with a
  relief gradient. Measured, not assumed.
* **Extracting the embedded pack to a file on first launch**: it would add a
  cache to manage, to invalidate between versions, and a write path where none
  is needed. `sqlite3_deserialize` makes extraction unnecessary.
