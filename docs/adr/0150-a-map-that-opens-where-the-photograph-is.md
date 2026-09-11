# ADR 0150 — A map that opens where the photograph is

**Status:** Accepted — 2026-09

## Context

Opening the Map view on the reference library shows the Sahara, with a dense
cluster of pins in the Gulf of Guinea and the photographs nowhere in sight. Two
independent defects, and they compound.

**A camera with no GPS receiver claims to be at 0°N 0°E.** A body that cannot
know where it is still writes a GPS block, filled with zeros, and `exif.rs`
reads `0/1, 0/1, 0/1` with a valid `N`/`E` reference as a perfectly good fix.
Counted in the reference library (`G:\Mes images`, 38 389 photographs):

| | |
|---|---|
| geotagged | 9 626 |
| at exactly 0, 0 | **2 997** (31 %) |
| of which, from a Canon EOS 5D Mark IV | 2 995 |
| of which, `.CR2` | 2 995 |
| rows with only *one* coordinate at zero | **0** |

The 5D Mark IV has no GPS receiver. Not one row zeroes a single coordinate,
which is what says these are absences rather than positions: a real fix lands on
the meridian or the equator by accident, never on both at once. And 2 995 of the
2 997 are `.CR2`, which is to say they came through **LibRaw**, not the EXIF
reader — so the rule has to sit where both paths meet, or it is two rules.

**And the view opens on the mean of every pin.** A mean is not a place. The
measured mean of those 9 626 pins is 32.62 N, 1.98 E — Algeria — and even with
the phantom pins gone it would be a field somewhere between the clusters, at a
zoom close enough to show nothing. Averaging Paris and Brussels gives a point in
neither.

## Decision

### 1. Zero and zero is not a position

A fix whose latitude **and** longitude are both exactly zero is read as no fix
at all. The test is `leyline_catalog::gps_fix`, in the crate that owns
`Metadata` because that is where the two readers meet — LibRaw fills the RAW
path, `exif` the rest, and a rule living in two places is two rules. Nothing
enters the catalog.

Only both. A photograph taken on the equator, or on the Greenwich meridian,
keeps its pin — and the table above is why that distinction is the right one to
draw rather than a cautious-sounding compromise.

`Catalog::map_pins` filters the same pair as well, so a library imported before
this stops drawing 2 997 phantom pins without waiting for anything to be read
again. The stored rows are left alone: `metadata` is what the file said, it is
replaced wholesale on every re-read ([ADR 0099](0099-authored-descriptions.md)
§1 draws that line), and a re-import now writes nothing there.

The loss is a photograph genuinely taken within a few metres of 0, 0, in the
Gulf of Guinea. Set against 2 997 photographs of Belgium and the Île-de-France
claiming to be there, it is not a close call.

### 2. The map opens where the photograph is

`View::initial` takes the photograph one was looking at. When it carries a pin,
the map opens centred on **it**, at zoom 14 — the street, not the region.

When it does not, the view frames **every** pin: the centre of their bounding
box, at the largest zoom whose viewport contains the box. That is the answer to
"where are my photographs", where a mean answered "nowhere in particular".

The zoom is found by trying each level from the closest down — twenty
iterations of a projection that the renderer runs per tile anyway — rather than
by inverting the projection, because the viewport is in pixels, the box is in
degrees, and Mercator makes the relation between them latitude-dependent. A
loop over twenty integers is the cheap, obviously-correct version of that.

An empty library keeps the whole-world view it already had.

## Consequences

* The Map view opens on the photograph one selected, and the Gulf of Guinea
  cluster is gone.
* `View::initial` gains the focused pin and the canvas size; both come from the
  session that was already building it. Importing a pack rebuilds the view with
  **no** focus: that is not arriving from a photograph, and what it should show
  is what the new pack covers.
* No migration. The wrong rows stay in `metadata` — where they are a faithful
  record of what the file says — and no reader of them shows a pin any more.

## Alternatives rejected

* **Dropping any zero coordinate**, not only the pair. It would silence a real
  fix on the equator, which the measurement says never co-occurs with the
  phantoms.
* **A migration nulling the 2 997 rows.** `metadata` mirrors the file; a re-read
  would put them back on any build older than this one, and the filter costs one
  `AND` in a query that already runs once per map opening.
* **Keeping the mean** and merely excluding Null Island. Still a point between
  the clusters, still at a zoom that shows neither.
* **Opening on the whole world.** Safe, and it makes the first gesture in the
  module a zoom towards a cluster one has to find by eye.
