# ADR 0081 — A grid page no longer sorts the whole library

**Status:** Accepted — 2026-08

## Context

`catalog.md` §35 promises, under "Navigation", **instant scrolling through
several hundred thousand assets**, and adds that "beyond that join, the common
queries avoid any further join". Measured on 2026-08-26 on a synthetic library
of **50,000 assets** (the order of magnitude of the real test corpus: 15,000
CR2s and 29,000 JPEGs), built in `--release`: **neither assertion is true**.

That is not a comfort detail. `load_window`
(`crates/leyline-studio/src/wiring/grid.rs`) calls `Catalog::grid` **on the
interface thread**, once per scroll step, with a window the size of the
viewport plus the overscan margin — a hundred rows or so.

### What the measurement imposes on the design

Three facts, two of which no reading of the code would have given:

* **SQLite materializes the complete output row *before* sorting.** The two
  correlated subqueries of the select list — the "already developed" badge
  ([ADR 0055](0055-library-navigation.md) §5) and the `RAW+J` badge
  ([ADR 0079](0079-raw-jpeg-pairing.md) §6) — therefore run **50,000 times in
  order to display 100 thumbnails**. A select list lightened of those two
  columns falls from 65 ms to 32 ms: they weigh half the time, and they weigh
  it on rows nobody will see.
* **The size of the window requested changes almost nothing.** 100 rows cost
  10.4 ms, 1,000 rows 17.0 ms. The cost is not reading the rendered rows, it is
  sorting the entire filtered set — `EXPLAIN QUERY PLAN` answers
  `USE TEMP B-TREE FOR ORDER BY` in every case.
* **The leading expression term of the `ORDER BY` forbids any index from
  serving the sort.** `ORDER BY a.capture_date IS NULL, a.capture_date DESC`
  can be satisfied by no index on `capture_date`. And descending it is
  **redundant**: SQLite already sorts NULLs last when the order is `DESC`. It
  is necessary only ascending, where `NULLS LAST` says it without forbidding
  the index.

## Decision

### 1. The page is chosen on lean rows, and then decorated

`Catalog::grid` now emits two levels instead of one. The inner level carries
the filters, the order and the window, and selects only **two integers** — the
version's identifier and the asset's. The outer level joins `develop_versions`
and `assets` on the hundred surviving rows and computes `GridItem`'s eleven
columns there, badge subqueries included:

```sql
WITH page AS (
    SELECT c.version_id AS vid, a.id AS aid
    FROM ...  WHERE ...  ORDER BY ...  LIMIT ? OFFSET ?
)
SELECT <the eleven columns>
FROM page
JOIN develop_versions v ON v.id = page.vid
JOIN assets a ON a.id = page.aid
ORDER BY ...
```

The two badges are then evaluated a hundred times and not fifty thousand. The
sort, for its part, bears on nothing but pairs of integers.

That is also what makes §35 literally true for the first time: the further
joins still exist, but they apply only to the page.

### 2. The order loses its expression term

`ORDER BY a.capture_date IS NULL, a.capture_date DESC, a.id` becomes:

* `ORDER BY a.capture_date DESC, a.id` descending — NULLs already fall last,
  and the removed term decided nothing;
* `ORDER BY a.capture_date ASC NULLS LAST, a.id` ascending — the same order as
  before, said in a way an index can serve.

**The observable order is identical**, and it is verified as such: a test
compares the first 300 rows of the old form and the new one across all four
sorts. `NULLS LAST` has existed since SQLite 3.30 (2019), far below the
embedded version.

### 3. A covering index that starts with the clause every grid carries

```sql
CREATE INDEX idx_assets_grid ON assets(companion_of, capture_date, id);
```

`companion_of` first because **every** grid query carries
`a.companion_of IS NULL` ([ADR 0079](0079-raw-jpeg-pairing.md) §5);
`capture_date` next because it is the default sort and by far the most used;
`id` last, which is the tiebreak.

`idx_assets_companion` **stays**, though this new index has it as a strict
prefix. That was the opposite decision until it was measured: `count()` walks
that index end to end in order to size the grid's scrollbar, and walking it in
its wide version costs **28.7 ms against 3.3 ms**. The narrowness of the
entries is exactly what makes that walk cheap; the same covering index also
serves the companion existence test
([ADR 0079](0079-raw-jpeg-pairing.md) §6). The price is one more b-tree per
registered asset, on a write path that is not where imports spend their time.

It is the only place in this ADR where measurement overturned the design, and
it did so after the code was written: without it, counting would have been
eight times slower in order to save an index.

### 4. `count()` does not change shape

Counting does not sort and returns no column: the deferred join has nothing to
offer it. It goes on emitting a single query, over the same shared
`FROM`/`WHERE` trunk — that trunk becomes a function, and that is the only
reason `build` is split.

### 5. The select lists are named, and the grid's order is tested rather than paid for

Every query in `leyline-catalog` read its rows **by position**. Eleven of the
grid's columns are integers, and §1 above rewrote its select list: a column
reordered in the SQL and not in the closure would have gone on compiling, and
would have swapped a rating for a colour label in silence. The catalog now
names every column it selects (`AS version_id`, `AS photo_count`, ...) and, for
every query but one, reads it by that name.

The exception is `Catalog::grid`, and it is a measured one. It runs **once per
scroll step, on the interface thread**, and `row.get("name")` scans the
statement's column names on **every column of every row** — 2,200 scans for a
200-row page. Measured on the same 50,000-asset database, in `--release`:

| | head page | rolling offset |
|---|---|---|
| by position | 585 µs | 3,487 µs |
| by name, per row | 790 µs (**+35 %**) | 3,634 µs (+4.2 %) |
| names resolved once per statement | 651 µs (**+11 %**) | 3,462 µs (−0.7 %) |

The cost is **flat** — around 200 µs a page, whatever the page — so it
disappears into the sort at a deep offset and is most visible exactly where §1
worked hardest to be fast. Averaged over a scroll it reads as +4 %, which
understates it by a factor of eight at the head.

None of that buys anything, though: naming columns is a **correctness**
measure, not a performance one, and there is no reason to pay for it at
runtime. `grid` keeps its positional reads and its 585 µs; the guarantee moves
into a test. `GRID_COLUMNS` declares the select list's order beside `GridItem`,
`Catalog::grid_columns` reports the order SQLite really prepares — a diagnostic
alongside `grid_plan`, and for the same reason: the SQL is built privately and
varies with the sort, so a test that rebuilt it would check its own copy — and
`grid_columns_match_the_declared_order` compares the two across all nine sorts.
Reordering the select list now fails a test instead of swapping two integers.
Teeth verified by sabotage: swapping `rating` and `color_label` in the SQL
fails it.

Everywhere else — folders, presets, exports, prints, metadata — the per-row
named form stays: those queries run once per user action, not once per frame,
and readability is worth more there than a microsecond.

## Consequences

Measurements over **one same database** of 50,000 assets, with capture dates
shuffled, a 100-row window, `--release`: the old form run by hand and the new
one through `Catalog::grid`, one after the other.

| | before | after | ratio |
|---|---|---|---|
| head, default sort (date descending) | 10.32 ms | **0.46 ms** | ×22 |
| head, date ascending | 10.38 ms | **0.42 ms** | ×25 |
| a 1,000-row window | 15.99 ms | 3.30 ms | ×4.8 |
| sort by import date | 58.87 ms | 16.47 ms | ×3.6 |
| offset 25,000 | 69.97 ms | 20.83 ms | ×3.4 |
| offset 49,000 | 77.01 ms | 42.01 ms | ×1.8 |
| `count()` | 3.47 ms | 3.47 ms | unchanged |

The gain is largest where it matters most — opening a folder and starting to
scroll — and it grows with the library's size, since an `O(N log N)` per page
becomes an index walk bounded by the window.

Two effects to know about:

* **Paging by `OFFSET` stays linear in the offset.** Going down to the bottom
  of a 50,000-photo library still costs 41 ms. The deferred form still wins
  there (77.5 ms today), but the index can do almost nothing about it: at
  offset 49,000, walking the index costs even ~10 ms more than sorting lean
  rows. That is the accepted price of a factor of 36 at the head.
* **Sorts other than by capture date gain only the deferred join**, for want of
  an index to serve them: the import date goes from 58.9 ms to 16.5 ms, while
  `filename` and the rating gain less than 5 %. None regresses.
* **A filter that returns nothing still costs a complete walk**: to prove that
  no photo is rated 3 stars, all of them have to have been looked at — 29.9 ms,
  index or no index. As soon as the filter returns a page, we fall back to
  0.31 ms. That is a property of the question, not of the query.

## Alternatives rejected

* **Keyset pagination (`WHERE (capture_date, id) < (?, ?)`)** — that is the
  real answer to the offset's cost, and it is incompatible with the interface:
  `desired_window` asks for a window **by absolute index**, because the grid's
  scrollbar must be able to jump anywhere. Changing that is an interface
  decision, not a catalog one.
* **Denormalizing the two badges into `assets` columns** — it would remove the
  subqueries, but would introduce two columns to keep up to date on every
  revision and every pairing, hence two ways of lying. The deferred join makes
  them rare enough that the question no longer arises.
* **Memoizing the page in Studio** — it would move the cost without removing
  it, and the first scroll would pay it anyway. A cache is an answer to an
  irreducible cost; this one was not.
* **Removing the badges** — they are decided by
  [ADR 0055](0055-library-navigation.md) §5 and
  [ADR 0079](0079-raw-jpeg-pairing.md) §6, and they were expensive only by an
  accident of shape.

## What this ADR does not do

No pixel moves: no stage, no stage version, no revision. `pipeline.md` §5.1 is
not concerned. The schema gains neither a column nor a table — one index
replaces one index. And `Catalog::grid`'s contract is unchanged: the same
parameters, the same rows, in the same order.
