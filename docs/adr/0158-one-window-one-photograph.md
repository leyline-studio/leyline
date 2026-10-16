# ADR 0158 — One window around one photograph: the Studio layout and its palette

**Status:** Accepted — 2026-10

## Context

Studio grew panel by panel along the Lightroom *Classic* model
([ADR 0055](0055-library-navigation.md)): a menu bar carrying a module
switcher (Library · Develop · Map), a folder tree on the left, a
left column in Develop ([0124](0124-develop-left-column.md)), sixteen
accordion groups on the right, a toolbar of chips above the photograph. Each
step was argued; the sum is an interface where the photograph competes with
three bands of controls, and where each module rearranges the window.

The photographer has asked for the other model — the one of the cloud-era
Lightroom, which several open tools now follow: **one window that does not
change shape**, the photograph in the middle, **one settings panel on the
right chosen from a rail of icons**, a filmstrip and a slim bar of pills at the
bottom, a search field centred at the top. This is a change of course on the
appearance of the whole application, therefore an ADR before any line.

A second fact makes the first slice unavoidable: **Studio has no palette**.
Colours are literals in the `.slint` files — `#9a9a9a` appears 117 times,
`#e6e6e6` 76, `#1b1b1b` 49, `#3a3a3a` 47, plus an accent pair `#6fa8dc` /
`#2f4f6f` and a yellow `#ffcc00`. Changing the look today means editing
several hundred lines with no way to tell a "secondary text" grey from a
"disabled border" grey that happens to share its value.

Constraints that do not move:

* **No preference may change a pixel** ([0078](0078-preferences-panel.md)),
  and the viewer surround stays the photographer's choice
  ([0117](0117-viewer-surround.md)): the palette below is the *interface*,
  never the ground the photograph is judged on.
* **The keys printed in the interface stay true**
  ([0136](0136-customisation-refused-and-remembered.md) §1): 88 translated
  strings carry a key; none of those keys is reassigned by this ADR.
* **A narrow window never pushes a panel off-screen**
  ([0125](0125-narrow-window-develop.md)); **dialogs stay bounded by the
  window** ([0138](0138-a-dialog-that-fits.md)).
* **Nothing is copied**: not code, not icons, not images from any other
  project. The layout and a set of greys are ideas; the glyphs are drawn here.

## Decision

### 1. A palette, named by role

A single `export global Theme` in `ui/types.slint` holds every colour of the
interface, **named by what it is for**, never by its value:

| Token | Value | Role |
|---|---|---|
| `ground` | `#1c1c1c` | behind the photograph and the filmstrip (the `dark` value of [0117](0117-viewer-surround.md)) |
| `surface` | `#2d2d2d` | top bar, bottom bar, settings panel, rail |
| `sunken` | `#222222` | search field, histogram, a row set into a panel |
| `separator` | `#242424` | lines between panel sections |
| `selected` | `#2a2a2a` | the selected filmstrip or grid cell's background |
| `selected-frame` | `#e6e6e6` | the frame around the selected cell |
| `pill` | `#0e0e0e` | the pill groups of the bottom bar |
| `text` | `#e6e6e6` | headings, values |
| `text-dim` | `#9a9a9a` | labels, secondary text |
| `text-disabled` | `#5b5b5b` | disabled rows and controls |
| `control` | `#b3b3b3` | button faces, slider thumbs |
| `track` | `#5b5b5b` | slider tracks |
| `focus` | `#6fa8dc` | keyboard focus and the active rail icon, the **only** accent |
| `warning` | `#ffcc00` | clipping overlays, warnings — a meaning, not a decoration |

The interface is **neutral grey**: one accent, kept for focus, so that no
colour of the interface sits next to the photograph pretending to be in it.
After the slice that introduces it, **a colour literal in a `.slint` file
outside `Theme` is a defect**, checked by `make check` (a grep for
`#[0-9a-fA-F]{6}` outside `types.slint`, with an allow-list for the
photograph's own overlays: histogram channels, colour-wheel hues, mask tints).

There is **one theme**. A light theme or a theme preference is not admitted:
it would be a setting with no bearing on the installation
([0078](0078-preferences-panel.md) admission rule), and twice the screenshots
to verify for every slice.

### 2. The frame of the window does not change

From top to bottom, the window is always:

1. **Menu bar** — the text menus stay (`File`, `Edit`, `Photo`, `View`, …):
   they carry most of the 88 printed keys, and a menu is where an unknown
   action is looked for. The **module switcher leaves it**.
2. **Top bar** (`surface`, 40px): at the left, the toggle of the left panel,
   then `↶` `↷`; **centred**, the search field (`sunken`), which is the
   existing text filter given the place it deserves; at the right, the filter
   menu, Map, Import, Export.
3. **Body**: left panel (optional) · centre · rail-driven right panel ·
   **rail**.
4. **Filmstrip** (`ground`), under the centre only, shown in the loupe and in
   editing, hidden in the grid.
5. **Bottom bar** (`surface`, 40px): at the left, the three views — square
   grid, justified grid, loupe — and the sort; **centred, a pill** (`pill`)
   with stars, reject/pick flags and *Copy settings*; at the right, zoom
   (`Fit` / percentage) and the toggles of the filmstrip and the right panel.

### 3. The left panel is the library

The folder tree, the collections, the smart collections and the keyword list
stay in **one left panel**, toggled from the top bar, the same in every view.
It is the first to fold on a narrow window, as the left column already is
([0125](0125-narrow-window-develop.md) §1).

The Develop left column of [0124](0124-develop-left-column.md) disappears as a
column; its four sections move to the rail (§4), except the **Navigator**,
which becomes a small overlay in the bottom-left corner of the photograph,
shown only when the zoom exceeds *Fit* — the only moment it answers a
question.

### 4. The rail chooses the one panel on the right

A column of glyph buttons (40px, `surface`) on the far right. Each opens
**its** panel in the right column (300px, unchanged); clicking the open one
closes the panel. Top group, top to bottom:

| Icon | Panel | Was |
|---|---|---|
| Edit | histogram, Auto / B&W / Reset, the develop groups as foldable sections | the right column of Develop |
| Crop | crop and straighten | a toolbar mode |
| Retouch | clone / heal, red eye | toolbar modes |
| Masks | local adjustments | a toolbar mode |
| Presets | the preset library | left column ([0124](0124-develop-left-column.md)) |
| Versions | versions of the photograph | left column |
| History | revision history | left column |

Bottom group: **Keywords** and **Info** (EXIF, file, authored description) —
library panels reachable without leaving the photograph.

Glyphs carry no words, so each has its hint with its key
([0127](0127-hints-on-wordless-controls.md)). **Existing keys keep their
meaning**: `D` (*Develop*) opens the Edit panel in the loupe, `E` the loupe,
`G` (*Library*, *Grid*) the grid, `M` the map, `C` and `N` compare and survey,
`Ctrl+E` export — wherever a key used to switch a module it now switches the
view and opens the panel. The other rail entries get **no new key** in this
ADR: `R` is already *Reprocess*, and a key is added only with its printed
string and its hint, in its own change.

The groups of the Edit panel keep their remembered fold state
([0128](0128-remembered-interface-state.md)); the open rail entry is
remembered the same way, as interface state, not a preference.

### 5. Modules become views

"Library" and "Develop" stop being modules: the grid is a view, the loupe is a
view, and **editing is the loupe with a rail panel open**. Opening the Edit
panel from the grid switches to the loupe on the selected photograph. **Map**
stays a whole-window view, entered from the top bar and left by any of the
three view buttons. Compare and Survey stay views of the bottom bar's view
menu. Print, contact sheet and tethering stay where the menus put them.

This replaces §1 of [ADR 0055](0055-library-navigation.md) (the module
switcher) and the left column of [ADR 0124](0124-develop-left-column.md) §1;
the rest of those ADRs stands. [ADR 0125](0125-narrow-window-develop.md) is
re-read with the new columns: left panel folds first, then the filmstrip;
**the right panel and the rail never fold** while a rail entry is open.

### 6. Delivery in four slices, each verified on screen

1. **Palette** (§1): `Theme`, every literal replaced, the `make check` guard.
   Same layout, new greys.
2. **Frame** (§2, §5): top bar, bottom bar, filmstrip, views instead of
   modules.
3. **Rail** (§3, §4): the right panel chosen by the rail, the left column
   dissolved, the Navigator overlay.
4. **Narrow window and dialogs**: the [0125](0125-narrow-window-develop.md)
   widths measured again, the 23 dialogs swept again in French
   ([0138](0138-a-dialog-that-fits.md)), `.pot` re-extracted.

Each slice ends with screenshots at 1920x1080 and at the narrowest supported
width, in English and in French, before and after.

## Consequences

* The photograph gets the width the Develop left column and the toolbar took;
  the toolbar of chips disappears, its tools are rail entries.
* Muscle memory from the Classic model survives through the keys, not
  through the places: a photographer who used the module switcher has to find
  the rail once.
* The user guide ([0151](0151-a-guide-that-ships-with-the-binary.md)) and its
  screenshots are rewritten with slice 3; until then it describes a layout
  that changes under the reader.
* A colour literal becomes reviewable: a diff that adds `#xxxxxx` outside
  `Theme` is visibly wrong.
* No engine, catalog, SDK, CLI or MCP surface moves: this is entirely
  `leyline-studio/ui` and its wiring.

## Alternatives rejected

* **Recolouring only**: cheaper, and leaves the three bands of controls that
  are the actual complaint.
* **A theme preference (dark / light / classic)**: refused by
  [0078](0078-preferences-panel.md)'s admission rule, and it doubles every
  slice's verification.
* **Dockable, floating, reorderable panels**: refused by
  [0136](0136-customisation-refused-and-remembered.md) for reasons this ADR
  does not change; one fixed frame is the point.
* **Keeping both layouts behind a switch**: two interfaces to keep true, for
  the length of a transition nobody would end.
* **Taking another project's icons or theme code**: incompatible licensing
  and nothing to gain; the greys are measured, the glyphs are drawn here.
