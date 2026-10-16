# ADR 0159 — Menus in the top bar, titled columns, two themes, and three ways in

**Status:** Accepted — 2026-10

## Context

[ADR 0158](0158-one-window-one-photograph.md) decided the frame of the
window from one screenshot. The reference application, run on the
photographer's own machine the same day, showed four things the screenshot
did not, each of which contradicts or completes 0158:

1. **There is no separate menu bar.** The text menus sit *inside* the top
   bar, between the back/forward arrows and the search field. 0158 §2 stacks
   a menu bar *and* a 40px top bar: two bands where one does the work.
2. **Columns carry a title and Presets is a column of its own.** Each column
   has a small title strip (*Library* on the left, *Tools* on the right), and
   opening the presets puts a **second column beside the tools** rather than
   replacing them — one browses looks while the Edit panel stays in view.
   0158 §4 made Presets one rail entry among seven, which would hide the Edit
   panel the moment a preset is being chosen.
3. **Import is three entries, not one.** *Import Photos…*, *Import from
   Folder…*, and *Import from Device ▸*, a submenu listing the cards and
   cameras plugged in — or saying that none is. The photographer singled this
   out.
4. **There is a light theme**, switched by a sun/moon button at the right of
   the top bar. 0158 §1 refused any second theme; the photographer asked for
   two themes and a three-way switch: dark, light, and *system*.

Leyline today has **one** entry, `Import…    I`
(`ui/panels/menubar.slint`), opening a folder chooser and then the import
dialog of [0065](0065-selective-import.md) with its options (copy into the
library, recursive, RAW+JPEG pairing, exact duplicates, look first). The
engine already takes an explicit list of files (`Library::import_files`,
0065) as well as a folder (`scan_import`). **Nothing detects a card**: the
only device path is tethering over libgphoto2
([0038](0038-tethered-capture.md)), which drives a camera, does not read a
card, and is absent from the Windows build.

ADR 0158 is accepted and frozen ([README](README.md)): it is completed here,
not edited.

## Decision

### 1. One band: the menus live in the top bar

0158 §2 items 1 and 2 merge. The top bar (`surface`, 40px) reads, left to
right: left-panel toggle · `↶` `↷` · the text menus (`File`, `Library`,
`Photo`, `Develop`, `View`) · **the search field, centred in what is left** ·
filter · Map · Export · the theme switch (§6). The menus keep every entry, every printed key
([0136](0136-customisation-refused-and-remembered.md)) and their keyboard
access; only their container changes. *Import* leaves the right of the bar:
it is in `File`, three times (§4).

On a narrow window the search field shrinks first (down to an icon that
opens it), the menus never: a menu is where an unknown action is looked for.

### 2. Every column has a title strip

Each column — the left library panel, the presets column, the tools column —
starts with a 22px strip (`surface`, label in `text-dim`) naming it:
*Library*, *Presets*, *Tools*. The strip is a label, not a tab: there is one
thing per column, so nothing to switch to, and [0136](0136-customisation-refused-and-remembered.md)
still refuses docking and dragging columns about.

### 3. Presets is a column, not a rail entry

The rail of 0158 §4 keeps its entries, but **Presets opens its own column to
the left of the tools column** instead of replacing the tool panel. Its
rail icon toggles that column independently; any other icon chooses what the
tools column shows (Edit, Crop, Retouch, Masks, Versions, History, Keywords,
Info), one at a time as 0158 decided. Applying a preset while the Edit panel
is open shows its sliders move — which is what a preset *is*
([0058](0058-preset-provenance-and-shelf.md)).

On a narrow window ([0125](0125-narrow-window-develop.md) re-read by 0158
§5) the order of folding becomes: left panel, then **presets column**, then
filmstrip. The tools column and the rail still never fold.

### 4. Three ways in

The `File` menu opens with:

| Entry | Key | What it does |
|---|---|---|
| **Import Photos…** | `I` (moved) | a file chooser, multiple selection; the chosen files go to the import dialog through `import_files` — no folder scan |
| **Import from Folder…** | `Shift+I` (today `I` and `Shift+I` both import; `studio.slint` matches `i` or `I`) | today's `Import…`, unchanged: folder chooser, `scan_import`, the dialog |
| **Import from Device ▸** | — | a submenu, filled when it opens (§5) |

`I` moves to *Import Photos…* because choosing files is the general case
(a folder is many files; one photograph is not a folder). The printed string
changes with it, so the key stays true. The import dialog is the same in all
three: what differs is how the candidate list is built.

### 5. A device is a mounted volume with a `DCIM` folder

A card or a camera in mass-storage mode is recognised by one fact common to
every camera since the DCF standard: **a `DCIM` directory at the root of a
removable volume**. The submenu lists, at the moment it opens:

* **Linux**: the mount points under `/media/$USER` and `/run/media/$USER`
  (read from `/proc/self/mounts`), each kept if it has `DCIM/`;
* **Windows**: drives whose `GetDriveTypeW` is `DRIVE_REMOVABLE`, each kept if
  it has `DCIM\`;
* **macOS**: the entries of `/Volumes` other than the boot volume, each kept
  if it has `DCIM/`.

Each row is the volume's label and the photo count of its `DCIM` tree, e.g.
`EOS_DIGITAL — 412 photos`. With none: a disabled row, *No camera or card
found*. Detection is a function in `leyline-engine`
(`removable_photo_volumes() -> Vec<PhotoVolume>`), exposed through the SDK
so the CLI gets `leyline import --device` from the same truth; it reads
directories, never writes, never polls — it runs when the menu opens.

Choosing a device opens the import dialog on its `DCIM` tree with three
defaults that differ from a folder import, because a card is **about to be
erased**:

* **Copy into the library is on**, and cannot be turned off: referencing in
  place a file whose volume is leaving in five minutes makes a catalog of
  missing photos ([0010](0010-relative-paths.md), [0085](0085-named-roots.md)).
* **Recursive is on**: cameras split `DCIM` into `100CANON`, `101CANON`, …
* **Already-imported photos are flagged and unticked**, using the duplicate
  flag of [0065](0065-selective-import.md) (name + size) — the usual case of a
  card imported yesterday and shot on again today.

Nothing is ever deleted from the card, and there is no *eject after import*
or *erase after import*: an application that erases the only other copy of a
photograph has to be sure the copy is good, and verifying a backup is a job
Leyline does not take on.

### 6. Two themes, and a switch with three positions

This **reverses the refusal of 0158 §1**: there are two palettes, `dark`
(0158's values) and `light`, and `Theme` exposes the same tokens for both, so
no `.slint` file knows which one is on. Light values, measured on the
reference the same way as the dark ones:

| Token | Light |
|---|---|
| `surface` | `#e6e6e6` |
| `ground` | `#cfcfcf` |
| `sunken` | `#f6f6f6` |
| `selected` | `#cfcfcf` |
| `pill` | `#cfcfcf` |
| `text` | `#1c1c1c` |
| `text-dim` | `#5b5b5b` |

The others (`separator`, `control`, `track`, `text-disabled`, `focus`,
`warning`) are set in slice 1 by contrast against `surface`, not guessed
here: `focus` and `warning` must stay readable on both.

**The switch** is one button at the right of the top bar, cycling
**dark → light → system**, its glyph showing the current position (moon, sun,
half-disc) and its hint saying it in words
([0127](0127-hints-on-wordless-controls.md)). *System* follows the operating
system's appearance through Slint's `Palette.color-scheme`, and follows it
live when it changes. **The default is *system*.**

It is **interface state, not a preference**
([0128](0128-remembered-interface-state.md)): it has a natural place where it
acts, which the [0078](0078-preferences-panel.md) admission rule excludes from
the preferences panel; it is remembered across launches like the open rail
entry.

**The theme never changes the photograph's ground.** What surrounds the image
is the viewer surround of [0117](0117-viewer-surround.md) — dark, grey or
white, a choice about *judging* the photograph — and it stays independent:
a light interface around a dark ground is a legitimate pair, and an interface
switch that silently moved the ground would change every judgement made on
the image. `ground` is the colour of the filmstrip and of empty areas, never
of the viewer's surround. No pixel of an export moves either way
([0078](0078-preferences-panel.md)).

### 7. Recent libraries say which one is open

`File ▸ Open Recent` already lists the recent libraries by path. Two rows
are missing, both seen on the reference: the **library currently open is
ticked** (a list of places where one of them is *here* reads as a list of
places), and the list ends with **Clear Recent Libraries** after a
separator. Each row reads `name — path`, the name being the library
folder's. Clearing forgets paths only; it touches no library.

## Consequences

* One band less: 40px more height for the photograph than 0158 promised.
* `I` changes meaning, from *folder* to *files*. It is the one printed key
  this pair of ADRs moves, and it moves with its string.
* A camera in **PTP/MTP mode** (most recent bodies and every phone) mounts no
  volume and so is **not listed**; the photographer switches the camera to
  mass storage or uses a card reader. Reading over PTP needs libgphoto2,
  which the Windows build does not have ([0038](0038-tethered-capture.md)),
  and a submenu that lists a camera on two systems out of three would lie on
  the third.
* Every slice is now verified in **both themes**: screenshots at 1920x1080
  and at the narrowest width, in English and French, dark and light. That is
  the cost 0158 §1 refused, accepted on request.
* The slices of 0158 §6 become: 1 palette **with both themes and the
  switch**; 2 frame **with the menus in the
  top bar and the column titles**; 3 rail **and the presets column**; 4
  narrow window and dialogs. The three import entries are a **separate
  slice**, independent of the layout, deliverable first.

## Alternatives rejected

* **A light theme whose ground follows the interface** (the reference's
  behaviour): see §6 — the ground is a judging condition, owned by
  [0117](0117-viewer-surround.md).
* **A two-position switch without *system***: the photographer asked for it,
  and it is the only position that needs no decision on first launch.
* **Keeping the menu bar above the top bar** (0158 as written): two bands for
  the work of one, and the reference shows it is not needed.
* **Tabs in the column titles**: a tab invites several things per column and
  the drag to rearrange them — the docking [0136](0136-customisation-refused-and-remembered.md) refused.
* **A device watcher** (udev, `WM_DEVICECHANGE`, DiskArbitration) that
  announces a card as it is inserted: three platform APIs and a thread for
  what one directory read does when the menu opens; and an import that starts
  itself is the automatic step [0084](0084-assisted-culling.md) and
  [0105](0105-detector-conformance-and-cli.md) §2 refuse.
* **Erase or eject after import**: see §5.
* **Listing PTP cameras through libgphoto2**: true on two systems out of three.
