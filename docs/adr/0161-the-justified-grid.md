# ADR 0161 — The justified grid

**Status:** Accepted — 2026-10

## Context

[ADR 0158](0158-one-window-one-photograph.md) §2 puts three views at the
left of the bottom bar — *square grid, justified grid, loupe*. Slices 1 to
4 delivered everything around it except the justified grid: the bar has a
*Grid* and a *Loupe* button, and nothing in between. 0158 names the view and
says nothing about how it is laid out, and 0158 is frozen
([README](README.md)): it is completed here, not edited.

Two facts make it more than a button.

* **[ADR 0146](0146-the-details-one-notices-first.md) §2 refused a masonry
  grid**, on three grounds: the cell is a hit target, a badge frame and a
  drop target, and arrow-key navigation counts rows and columns. A justified
  grid is not masonry — rows of one height, photographs of their own width,
  each row filled edge to edge — but the third ground applies to both, and
  0158 overrides the refusal only by naming the view. This ADR has to say
  what answers each ground.
* **The grid is virtual.** `GridState.cells` holds a window of rows out of
  `total-cells`, and the position of cell *n* is arithmetic
  (`n / columns`, `n mod columns`), which is what lets Slint ask for
  `viewport-moved(first, capacity)` without knowing anything else. In a
  justified grid the row of photograph *n* depends on the shape of every
  photograph before it. No arithmetic on the index finds it.

## Decision

### 1. The justified grid is a second layout of the same grid

Same query, same selection, same cells, same badges, same context menu,
same drag and drop target: only the position and size of each cell change.
The cell keeps the foot of 0146 §2 (name and stars, 20px) under an image
area that is now the photograph's shape instead of a square. So the cell
stays a hit target, a badge frame and a drop target of the same kind — the
first two grounds of 0146 hold without change.

The choice is a preference, `grid_layout` in `preferences.json`
(`"square"` | `"justified"`, default `"square"`), set by the bottom bar's
middle button and remembered like `thumbnail_size`. No new key: 0158 adds
none, and `G` keeps returning to whichever grid layout was last chosen.

### 2. A cell's shape is the file's oriented frame, never the crop

The shape is the asset's `width` / `height`, swapped when the EXIF
orientation is 5–8. Missing dimensions count as 3:2. A ratio is clamped to
[1:3, 3:1], so a panorama does not take a row of its own at a height of
forty pixels.

The crop is **not** read. Reading it would mean opening the settings of the
current revision of every matching photograph — thirty-eight thousand JSON
documents on the reference library — to lay out a screen. A cropped
photograph is drawn `contain` inside its frame, as every thumbnail already
is: a few pixels of band, never a reframing (0146 §2's rule).

### 3. The layout is computed in Rust, over the whole query

When the query, the width of the grid or the row height changes, Studio
asks the catalog for the shapes of **all** matching rows — one query
returning `(width, height, orientation)` in grid order, no thumbnail, no
join beyond the one `grid` already makes — and a pure function
`justify(shapes, width, row_height, gap) -> Layout` breaks them into rows:

* a row is filled while its natural width (each ratio × `row_height`) is
  under the grid's width, then scaled down to fill it exactly;
* the **last row is not stretched**: it keeps `row_height` and stays
  left-aligned, so a folder of three photographs does not show three giants;
* a single photograph wider than the grid takes the row alone, scaled to fit.

`Layout` holds, per row, its first index and its y, and per photograph its
x and width. On 38 000 photographs that is a few hundred kilobytes and a
linear pass; the cost to watch is the query, which slice 1 measures before
anything depends on it.

### 4. The virtual window travels in pixels

Slint reports `justified-viewport-moved(y, height)` instead of
`viewport-moved(first, capacity)`; Rust finds the rows that intersect it by
binary search over the row y's and loads that index range through the
existing `load_window`. Each `Cell` gains `x`, `y`, `w`, `h`, filled by Rust
in the justified layout and unused in the square one; `viewport-height` is
the layout's total height. The square grid does not change by a line.

### 5. Arrow keys follow the rows

`←` / `→` move by one index, as now. `↑` / `↓` move to the photograph of the
row above or below whose horizontal centre is closest to the focused one's —
the rule every justified viewer uses, and the answer to 0146's third
ground: navigation counts rows, not columns, and Rust, which holds the
layout, answers `grid-step-vertical(index, ±1) -> index`.

### 6. The slider sets the row height

In the justified layout the bottom bar's size slider and `+` / `-` set the
row height, between the same bounds as the square cell (110–320px), from the
same `thumbnail-size` preference: one number means "how big", whatever the
layout.

### 7. Delivery

1. Catalog: the shapes query, measured on the reference library; the pure
   `justify` and `step_vertical` with their unit tests.
2. Studio: the preference, the middle button, the cells positioned from
   `Layout`, the pixel viewport, the arrow keys, `make i18n`.
3. The guide (FR + EN) gains the view, and an on-screen check at 1920 and
   900px with a mixed folder (landscape, portrait, a panorama).

## Consequences

* The square grid and everything that reads `columns` stay as they are;
  the justified layout is additive.
* Every change of query, width or size costs one more catalog query, over
  the whole result rather than a page. If slice 1 measures it as visible on
  the reference library, the layout is computed in the background and the
  grid shows the square layout until it lands — decided then, on the number.
* A photograph whose orientation tag is wrong shows its wrong shape here
  where the square grid hid it.
* Cropped photographs show a thin band. Accepted, for the reason in §2.

## Alternatives rejected

* **Reading the crop for each cell's shape**: exact, and a settings read
  per photograph to lay out a screen.
* **Laying out only the loaded window**: rows would change when the window
  moves, and the scrollbar would lie about the length of the grid.
* **Computing the layout in Slint**: Slint has the shapes of the loaded
  window only, and no way to binary-search the rest.
* **Stretching the last row**: three photographs in a folder become three
  grid-wide giants.
* **A masonry grid (columns of one width)**: still refused, for 0146 §2's
  reasons; it was never what 0158 asked for.
