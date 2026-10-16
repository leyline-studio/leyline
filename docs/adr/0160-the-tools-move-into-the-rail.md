# ADR 0160 — The tools move into the rail, and where the unnamed ones go

**Status:** Accepted — 2026-10

## Context

[ADR 0158](0158-one-window-one-photograph.md) §4 gives the rail three tool
entries — **Crop** (crop and straighten), **Retouch** (clone / heal, red
eye), **Masks** (local adjustments) — and its Consequences say *the toolbar
of chips disappears, its tools are rail entries*. The toolbar of Develop
(`ui/panels/develop.slint`) holds twelve chips, and 0158 places only six of
them:

| Chip | Placed by 0158 |
|---|---|
| Crop | Crop |
| Spot Removal, Red Eye | Retouch |
| Radial, Gradient, Brush | Masks |
| Select | — |
| White-balance picker ([0091](0091-white-balance-picker-and-presets.md)) | — |
| Target ([0130](0130-direct-manipulation.md) §3) | — |
| Reshape | — |
| Keystone ([0119](0119-guided-keystone.md)) | — |
| `?` (the shortcut card) | — |

Removing the toolbar without a home for the other six removes features.
0158 is accepted and frozen ([README](README.md)): it is completed here, not
edited.

## Decision

### 1. A tool entry is a panel and a tool together

Opening **Crop** picks the crop tool; opening **Retouch** picks Spot
Removal, the first of its tools; opening **Masks** picks none, so the
handles of the existing masks — drawn only under Select — are what the
photograph shows first. Closing a tool entry, or opening any other entry,
returns to **Select**. Select stops being a chip: it is the state of the
photograph when no tool panel asks for something else.

`O` (composition guides, [0135](0135-crop-ratios-and-guides.md) §3) opens the Crop
entry where it used to pick the crop tool, for the same reason: a guide is
drawn only while cropping.

### 2. What each tool panel holds

* **Crop** — the ratio row and the guide (0135 §1), **Rotation** (the
  straighten angle), the four crop edges and *Reset crop*. These leave the
  Geometry group of the Edit panel, which keeps **Perspective** and the
  Keystone guides.
* **Retouch** — three buttons (Spot Removal, Red Eye, **Reshape**) choosing
  the tool, then the Spot Removal, Red Eye and Reshape groups. Reshape is a
  repair of the photograph's shapes, not a tone setting: it joins the
  repairs.
* **Masks** — three buttons (Radial, Gradient, Brush) drawing a new mask,
  then the Local group as it is today.

Retouch and Masks are Full-mode entries ([0054](0054-first-run-and-basic-mode.md)
§2): in Basic mode the rail does not show them, as the toolbar hid their
chips.

### 3. The pickers go beside what they set

The **white-balance picker** sits on the White Balance row of Basic, the
**Target** tool in the Basic group's header — it moves Basic's sliders
(0130 §3) — and the **Keystone** tool heads the Geometry group whose
sliders it writes. Each stays picked until another tool is, as today; they
are the only tools reached from the Edit panel.

### 4. The shortcut card loses its chip

*Help ▸ Keyboard Shortcuts…* already opens it. No key is added
([0158](0158-one-window-one-photograph.md) §4).

### 5. Delivery

Four slices, each verified on screen: **3c-1** Crop, **3c-2** Retouch,
**3c-3** Masks, **3c-4** the pickers move and the toolbar goes. Until 3c-4
the toolbar keeps the chips not yet moved.

## Consequences

* Every chip has a place, and the toolbar row returns its height to the
  photograph.
* A tool that used to be one click from any panel is now one click on the
  rail, then possibly one in its panel: Retouch costs a click for Red Eye and
  Reshape. Accepted — the panel then shows what the click wrote.
* The Edit panel becomes shorter by three groups and half of Geometry.

## Alternatives rejected

* **A rail entry per tool** (twelve glyphs): a rail is a list of panels,
  and six of these tools have no panel of their own.
* **Keeping a slim toolbar for the pickers**: two places to look for a tool
  is the problem 0158 set out to remove.
* **Reshape in the Edit panel**: it would be the one drag tool living among
  sliders with no gesture.
