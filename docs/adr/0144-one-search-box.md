# ADR 0144 — One search box, and what it is allowed not to know

**Status:** Accepted — 2026-09

## Context

Two things sit side by side in the filter bar: a **search box** and a **shot
filter** whose chips list every body and every lens the library holds.

Typing `Canon` in the box returned nothing.

`search_index` carried four columns — filename, keywords, artist, copyright —
and the shot filter reads `metadata` directly. So the library knew it had
three Canon bodies, and the box that looks like the place to ask did not.
Neither did it know the **title** and the **caption** the detail panel invites
the photographer to write ([ADR 0099](0099-authored-descriptions.md)), nor the
**day** the photograph was taken.

A search box that answers for some of what a photographer remembers and stays
silent about the rest is worse than one whose limits are visible: nothing on
screen says which half it holds.

## Decision

### 1. The index learns what the photographer wrote

`title` and `caption` join the row, beside the artist and the copyright that
were already there and for the same reason: they are what somebody typed about
a photograph, and typing it again is how they will look for it.

They follow the description, not the file: clearing a description clears the
index with it, because `search_index` is a cache of the tables and never a
memory of what they used to hold. Tested.

### 2. And what the file says: body, lens, and the day

`camera` and `lens` are written the way [ADR 0064](0064-metadata-filters.md) §2
names them — `Canon EOS 60D`, manufacturer and model — so **one vocabulary
answers both** the chips and the box. `captured` is the **local** day, offset
included, formatted `YYYY-MM-DD`: the tokenizer splits it into a year, a month
and a day, so `2023` finds a year and `2023-05-16` finds an afternoon. Local,
because what a photographer types is the date they remember taking the
photograph, not the same instant read in UTC — the distinction
`docs/catalog.md` §13 makes and that the grid already honours.

Maintenance follows the existing shape: the row is written at registration
(with the capture date, which is known then), the gear columns are refreshed
when `set_metadata` lands, and the authored columns when a description is
written.

### 3. What the box is allowed not to find, and why that is not a bug

A lens is stored as `EF-S18-55mm f/3.5-5.6 IS II`, and `unicode61` tokenises it
as `ef`, `s18`, `55mm`, `f`, `3`, `5`, `6`, `is`, `ii`. So `55mm` finds it and
**`18` does not** — `s18` is one token, and no prefix query reaches inside it.

That is not worth fixing, and the reason is the shot filter: a lens is
something one **picks from a list of what the library holds**, which is what
those chips are for; the box is for the word one remembers. Recorded here so
the next person who types `18-55`, gets nothing and reaches for a trigram
tokenizer reads this first — the cost would be an index several times larger,
and the answer already exists one chip away.

### 4. The migration is the rebuild

FTS5 has no `ALTER TABLE ADD COLUMN`. Migration v13 therefore **drops
`search_index` and refills it** from the source tables — which is not a
workaround but exactly what §30 says the table is: *rebuildable at any moment
from the source tables; in case of doubt, it regenerates like a cache*. An
existing library gains the five columns and their content on first open, with
no pass to ask for.

## Consequences

* Typing a body, a year, a title or a caption now finds photographs. The
  review's example — `Canon` finding nothing while the chips list three Canon
  bodies — measured on a real library after the migration: `HTC` finds 8,
  `2014` finds 6.
* The placeholder stays « Rechercher… ». There was a case for spelling out what
  it covers; the better fix was to make the answer match what the word promises.
* The index row grows by five columns of short text. `search_index` is derived,
  so the cost is disk in the catalog file and nothing in the photographs.
* A library opened by this build is at v13 and an older build refuses it by
  name — which is the startup refusal [ADR 0122](0122-startup-failure-window.md)
  built, seen here for the first time in ordinary work.

## Alternatives rejected

* **Indexing the whole `metadata` row** (ISO, aperture, focal length, shutter).
  Numbers with units are exactly what a *range* filter answers well and a text
  match answers badly: `iso 800` would match `800` in a filename, and `f/2.8`
  tokenises into two numbers. The shot filter already takes ranges.
* **A trigram tokenizer** so that `18` reaches inside `s18` — §3.
* **Searching by joining the source tables at query time** instead of keeping
  an index. §30's rule is that a search must be executable by SQLite without an
  external index; a `LIKE '%canon%'` over a join uses none of them.
* **A second box for metadata**, next to the free-text one. Two boxes is the
  problem this ADR is fixing, drawn twice.
