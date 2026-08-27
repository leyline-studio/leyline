# ADR 0064 — Filtering the grid by shot metadata

**Status:** Accepted — 2026-08

## Context

The grid filters today by folder, collection, rating, label, pick state,
keywords, text and date. **Nothing on the shooting conditions**: not the body,
not the lens, not the sensitivity, not the aperture, not the focal length.

It is a gap all the plainer because **the data is already there, and already
indexed**. The `metadata` table carries `camera_id`, `lens_id`, `iso`, and
three generated columns `aperture_f`, `focal_length_mm`, `shutter_speed_s`; six
indexes cover them (`docs/catalog.md` §32). The schema was designed for that
use and nothing ever exposed it.

The observation that brought it up comes from RawTherapee, whose filter panel
lists bodies and lenses **built from the actual content** of the open folder.
It is ground on which a catalog ought to beat a file browser: RawTherapee
rebuilds those lists by walking a folder on every opening, where an indexed
query gives them over the whole library, at any size.

One point of caution: **a body filter already exists**, in smart collections
(`SmartRules::camera`), with its own matching semantics — the model alone, or
`make model`. Adding a second body filter elsewhere, with other rules, would
make two answers to the same question diverge.

## Decision

**Six shot filters are added to `GridQuery`, without touching the schema.**

### 1. What is filterable, and how

| Filter | Shape | Column |
|---|---|---|
| Body | an exact value, chosen from a list | `cameras` through `camera_id` |
| Lens | likewise | `lenses` through `lens_id` |
| Sensitivity | a `[min, max]` interval | `iso` |
| Aperture | an interval | `aperture_f` |
| Focal length | an interval | `focal_length_mm` |
| Shutter speed | an interval | `shutter_speed_s` |

The first two are **discrete**: one chooses a body from a list, one does not
type its name. The other four are **continuous** and are given as an interval,
both bounds optional — "ISO ≥ 3200" is a more frequent request than "ISO
between 3200 and 6400".

Each filter is independent, and they combine with **and**. A photo with no
metadata for a filtered criterion does not appear: it does not satisfy the
criterion, and making it appear "by default" would make every filter lie.

### 2. The body filter reuses the existing semantics

The matching is `SmartRules::camera`'s — the model alone or `make model` — and
the clause-building code is **shared**, not copied. Two implementations of the
same question would end up answering differently, and that is the kind of
divergence one discovers only on a strange case, long afterwards.

### 3. The value lists come from the whole library

`Catalog::shot_facets()` returns the bodies and lenses present, along with the
observed bounds for the four continuous quantities — a `SELECT DISTINCT` and a
few `MIN`/`MAX` over indexed columns.

**Over the whole library, not over the currently filtered selection.**
Progressive refinement — where choosing "Canon 60D" would remove from the list
the lenses never mounted on it — is cleverer and costs more: every facet must
be recomputed on every change, excluding the filter whose list is being
computed. The gain is real but slight at this scale, and the behaviour is
harder to predict for whoever uses it. To be taken up if usage calls for it;
that would be an evolution of this decision, not a contradiction of it.

### 4. No migration, no index

Nothing to add to the schema. That is what makes this slice small, and it is
also what explains why it waited so long: nothing was missing, so nothing
called for it.

### 5. The three clients

The CLI gains options on `ls` (`--camera`, `--lens`, `--iso`, `--aperture`,
`--focal`, `--shutter`, with intervals written `min-max`, `min-` or `-max`),
the SDK exposes `GridQuery`'s fields, and Studio puts a collapsible panel under
the filter bar for them — not a side tab: those filters combine with the rating
and the label already present, and separating them into two places would make
people hunt.

Three details that came out of the implementation:

* An interval's written form is **read by the engine** (`ShotRange::parse`),
  not by each client: the CLI and Studio take the same string, so `1/200-`
  cannot mean two things. The bounds accept fractions, because a shutter speed
  is written `1/200` everywhere else. A lone value stands for both bounds.
* An inverted interval (`3200-400`) is **refused**, never answered with an
  empty grid: zero photos reads as "the library contains none", which would be
  false.
* The CLI also gains `leyline facets <library>`, which prints what
  `shot_facets` returns. Without it, §3's lists would exist only in Studio and
  a CLI user would have to guess a body's exact spelling in order to use it —
  the opposite of what §3 promises.

In Studio, bodies and lenses are **chips**, like the labels and flags of the
same bar, rather than a dropdown: a library's bodies can be counted on one
hand, and a chip shows at once what exists and what is active. The four
continuous quantities are input fields: both bounds being optional, a
two-handled slider would have to invent a way of saying "no bound at all". The
chip that unfolds the panel carries the number of active criteria, so that a
filtered grid never looks whole when the panel is collapsed.

## Consequences

* `GridQuery` gains six fields. Its construction stays a chain of optional
  `AND`s, and a query with no filter produces exactly today's SQL.
* Smart collections do not change, but now share their body clause with the
  grid.
* The facet lists are recomputed when the library changes (`AssetsAdded`,
  `AssetsRemoved`), not on every keystroke.
* `docs/catalog.md` gains the description of `shot_facets`.

## Alternatives rejected

* **Extending full-text search** instead of adding filters: typing "60D" would
  find photos, but "ISO between 800 and 3200" is not a question a full-text
  index knows how to ask, and mixing the two would make what the search bar
  does unpredictable.
* **A single expression filter** (`iso>800 AND camera="60D"`): powerful, and it
  has to be learned. Lists built from the actual content have nothing to learn
  — one sees what one has.
* **Progressive facets** from this version on: see §3.
* **Filtering client-side, over the loaded rows**: the grid is virtual, only a
  window is in memory, and a filter that saw only that window would be wrong
  from the first photo off screen.
