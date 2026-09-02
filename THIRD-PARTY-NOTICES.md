# Third-party notices

Leyline is released under the [GNU General Public License v3.0 only](LICENSE).
This file lists the third-party works it links against or ships inside its
binaries, with the licence each one carries. It is informational: nothing here
overrides the terms of those licences, and none of them is modified by being
listed.

Rust dependencies pulled from crates.io are not enumerated one by one — they
change with every lockfile update, and `cargo tree` together with
`Cargo.lock` is the authoritative list. What follows is the set of works that
are **bundled, embedded or linked as a system library**, which is the set a
reader cannot discover from the manifests alone.

## Libraries

| Work | Role | Licence |
|---|---|---|
| [LibRaw](https://www.libraw.org/) | RAW decoding. Linked as a system library, never vendored ([ADR 0004](docs/adr/0004-libraw.md)). | LGPL-2.1, as built and distributed by the platform |
| [LittleCMS](https://littlecms.com/) (`lcms2`) | ICC colour transforms ([ADR 0005](docs/adr/0005-lensfun-littlecms.md)). | MIT |
| [libheif](https://github.com/strukturag/libheif) | HEIF/HEIC reading ([ADR 0114](docs/adr/0114-heif-reading.md)). Linked as a system library, never vendored — and deliberately absent from the packages published here, which therefore carry no HEVC decoder. The codec plugin comes from the same system: `libde265` (LGPL-3+) decodes HEVC, `libaom`/`dav1d` decode AV1. | LGPL-3.0 |
| [Lensfun](https://lensfun.github.io/), through the `lensfun` crate — a pure-Rust port, itself a derivative work of the upstream C++ library | Lens correction: distortion, vignetting, transverse chromatic aberration ([ADR 0016](docs/adr/0016-process-3-lens-correction.md)–[0018](docs/adr/0018-process-5-tca.md)). The crate bundles the calibration database, which Leyline reads and never modifies. | LGPL-3.0-or-later (code), CC-BY-SA-3.0 (calibration database, the collective work of the Lensfun community) |
| [Slint](https://slint.dev/) | The desktop interface toolkit ([ADR 0002](docs/adr/0002-slint.md)). Used under its GPLv3 option, which is what makes Leyline Studio itself GPLv3. | GPL-3.0 (multi-licensed upstream) |
| [SQLite](https://sqlite.org/) | The catalogue, through `rusqlite` ([ADR 0003](docs/adr/0003-sqlite.md)). | Public domain |

## Embedded data

These are compiled into the binaries and travel with them.

| Work | Role | Licence |
|---|---|---|
| **darktable noise profiles** (`data/noiseprofiles.json`), © the darktable contributors | Measured sensor noise, per camera and per ISO — the thresholds of the profiled denoising stages ([ADR 0072](docs/adr/0072-measured-noise-profile.md)). Reduced, never altered: fields dropped, numbers rounded, duplicate ISO entries resolved. The upstream commit it was taken from is pinned in the file's own header. | GPL-3.0-or-later |
| **Natural Earth I** raster, rendered to zoom 0–5 tiles | The world basemap the map view starts from, when a build enables it ([ADR 0059](docs/adr/0059-bundled-world-basemap.md)). An imported map pack always takes precedence. | Public domain |
| **DejaVu Sans** | The font rasterised into text watermarks, embedded so the same watermark renders identically on every machine ([ADR 0051](docs/adr/0051-watermark-rasterization-and-soft-proof-surface.md)). | DejaVu Fonts License (Bitstream Vera derivative, permissive) |

## What is *not* here

No model weights, and no inference of any kind: Leyline ships neither
([`specification.md`](docs/specification.md) §4). A third-party mask detector
can be installed separately and called as an external process
([ADR 0073](docs/adr/0073-external-mask-detectors.md)) — whatever it embeds is
its own affair, not Leyline's.
