# ADR 0055 — Library navigation: the landmarks a Lightroom user looks for on arriving

**Status:** Accepted — 2026-08

## Context

[ADR 0054](0054-first-run-and-basic-mode.md) addressed what develop **shows
first**. The same user survey bore on a second gap, one of orientation and not
of functionality: knowing *where one is* and *where to click*.

A point-by-point comparison with Lightroom Classic's Library view gives the
gap's exact state. What Leyline already has, and which is not at issue: the
complete menu bar, the context menus ([ADR 0021](0021-context-menus.md)), the
identical classification shortcuts (1-5, 6-9, P/X/U), `G`/`D`/`M` to change
view, the filters (stars, labels, flags, search, sort), the collections, the
double-click into develop, the filmstrip in develop.

What is missing, in the order in which it disorients:

1. **No module selector.** Going from the library to develop is done by a menu
   or a key. The top-right corner, where a Lightroom user's eyes and cursor go
   on arriving, is empty.
2. **The left panel carries nothing but collections.** No folder tree — and yet
   that is how most people picture their photos, and the catalog already has
   everything needed to display it (a `folders` table with its `parent_id`,
   `GridQuery.folder`).
3. **No toolbar under the grid.** The thumbnails are fixed at 176 px
   (`cell-size`, `panels/browser.slint`), and seeing a photo large requires
   entering develop, hence opening an edit session.
4. **The cells say almost nothing**: a name, a colour dot, stars. No number, no
   flag, and no "this one has already been worked on".

**What is not at issue.** No feature is missing: everything above is a matter
of laying out what already exists. No setting, no rendering and no stored
format is touched by this decision.

## Decision

### 1. A module selector to the right of the menu bar

**Library | Develop | Map**, right-aligned in the existing menu bar — not a
second bar: that side is empty today, and navigation does not deserve a further
row of pixels. The current module is highlighted; *Develop* is inactive with no
photo selected, exactly as the menu entry of the same name already is.

**Print does not appear there.** A selector names *places* one stays in;
printing is an *action* that ends — it stays a dialog, under `Ctrl+P` and in
the File menu, where a Lightroom user finds it too.

### 2. The left panel carries the folders

From top to bottom: **All photos**, the **folder tree** with each one's photo
count, then the **collections**. Clicking a folder filters the grid
(`GridQuery.folder`, which already exists); clicking *All photos* removes the
filter.

The tree is **read-only**: nothing is renamed, moved or deleted there. Moving a
folder is file management, not cataloguing, and Leyline never modifies what it
did not write (`docs/vision.md`).

For that the catalog gains a single new read: listing the folders with their
parent and their count. No schema change.

At the bottom of the panel, the two buttons **Import…** and **Export…**, where
Lightroom puts them, opening the existing dialogs. The grey shortcut line
(`N: new · B: add photo`) disappears: ADR 0054 §1 said a shortcut does not
replace a front door; that is true too when the library is not empty.

**"Previous Import" is not taken up.** The catalog has no import-batch identity
— only an `imported_at` per photo — and deducing a batch from a timestamp would
be a guess that would be wrong the day two imports follow one another. The
"by import date" sort, which exists, covers the real need. Giving it a true
identity would be a catalog decision, to be taken on its own.

### 3. A toolbar under the grid

It carries two things, and not one more:

* a **thumbnail size slider**, because a contact sheet of 400 photos and a
  re-reading of three crops are not looked at at the same size;
* two **view modes**: *Grid* and *Loupe*. The loupe shows the selected photo
  large **without opening an edit session**: it is the preview, not develop.
  The difference is real and worth holding — no revision created, no history,
  nothing to write, and therefore an immediate display. The arrow keys navigate
  there, and `G` returns to the grid.

*Compare* (C) and *survey* (N) are not here: they require a zoom state shared
between two images, which the loupe does not have, and are therefore the
subject of a decision of their own —
[ADR 0057](0057-compare-and-survey.md).

Sorting and filters stay at the top, where they are: repeating them at the
bottom would be two places for one setting.

### 4. `E` opens the loupe, export moves to `Ctrl+E`

That is the only existing shortcut this decision moves, and it does so
deliberately. `G`, `E` and `D` are the three keys a Lightroom user presses
without thinking; `G` and `D` already do here what they expect, and `E` is the
last one missing. Export, for its part, is a deliberate action reached through
the File menu, through the left panel's button (§2) and through `Ctrl+E` —
three doors rather than one letter.

The shortcuts dialog, the File menu and `docs/` are updated at the same time: a
shortcut that changes without the help saying so is a bug.

### 5. The filmstrip is shared, and the cells say the essentials

The filmstrip becomes a single component, shown in **loupe** and in
**develop**. Not in the grid: it would repeat the grid itself there.

The cells gain three landmarks, all already known to the catalog or one read
away:

* the **index number** in the grid, like Lightroom — it is what lets one say
  "number 47" to someone;
* the **flag** (pick / reject), which is already in `GridItem` and simply was
  not displayed;
* an **"already developed"** badge: the version carries more than its initial
  revision. That is the only new field, read-only, computed by the same query
  as the grid.

Stars and the colour dot stay where they are.

### 6. `Tab` collapses the panels, `Shift+Tab` everything else

A photo is judged on the photo. `Tab` hides the side panels — the library on
the left, the metadata or the settings on the right — and `Shift+Tab` also
hides the toolbars and the filmstrip, leaving only the image and the menu bar.

**The menu bar stays**, always: it carries the module selector (§1), that is,
the only way out that does not require knowing a shortcut. An interface that
collapses until it no longer says how to get out is a trap, not a full-screen
mode.

It is pure interface state, it lives for the window's lifetime and Rust never
reads it (§7 below). Both shortcuts are Lightroom's, identically.

### 7. Three properties to hold

* **none of this is stored**: the current module, the view mode, the thumbnail
  size and the selected folder are interface state, they live for the window's
  lifetime and Rust never reads them
  ([ADR 0045](0045-studio-ui-modularisation.md) §2);
* **no setting, no rendering and no stored format changes**: the catalog's only
  surface evolution is read-only;
* **nothing is removed**: every gesture that existed before exists after, in
  the same place or with one more door.

## Out of scope

* **The Navigator** (the preview at the top of the left panel). It really
  serves only to move around an image zoomed beyond 100 %; the grid and the
  loupe already show the photo.
* **Renaming, moving or deleting folders** from the tree (§2).
* **An import-batch identity** (§2).
* **Cloning the interface to the pixel.** The aim is that a Lightroom user
  should know where to click, not that they should believe they launched
  Lightroom.

## Consequences

* **A migrant's first screen has its three landmarks**: where to change module,
  where their folders are, how to enlarge the thumbnails.
* **The catalog gains two reads** — the folder list with its counts, and a
  "developed" boolean per grid row — no writes, and no schema change.
  `docs/catalog.md` is updated.
* **One shortcut changes** (`E`), with its documentation in the same gesture
  (§4).
* **The grid gains four interface states** (view mode, thumbnail size, current
  folder, panel collapse) which stay on the UI side, and ADR 0045 §2's
  mechanical test goes on passing.
* **The left panel's grey shortcut line disappears**, replaced by two buttons.

## Alternatives rejected

* **Changing nothing and writing a guide.** The same answer as ADR 0054: it is
  what we hold against the others.
* **An optional "Lightroom shortcuts" mode.** Two sets to maintain, and a
  choice to make before having anything to choose from.
* **Putting *Print* in the module selector** so as to match Lightroom. There,
  Print *is* a module with its panels; here it is a dialog
  ([ADR 0036](0036-print-module.md)). A selector that opens a modal window lies
  about what it is.
* **Keeping `E` for export and putting the loupe elsewhere.** Any other key is
  a key to be learned — which is precisely what this decision seeks to avoid.
* **Showing the filmstrip in the grid too**, like Lightroom. It would show the
  same thumbnails as the grid, twice, in two sizes.
