# ADR 0054 — Getting started: an empty library that explains, and a Basic mode by default in develop

**Status:** Accepted — 2026-07

## Context

A survey of what users actually hold against free RAW developers, on a
July 2026 discussion thread comparing eight of them (darktable, RawTherapee,
ART, vkdt, Filmulator, LightZone, RapidRAW, Safelight):

* "a steep learning curve";
* "I simply cannot figure it out";
* "much more complicated than Lightroom";
* a darktable getting-started guide posted twice in the same thread.

**No message asks for a missing feature.** The market's gap is not functional,
it is one of access. And Leyline has just gone the other way: the ADR 0049–0053
series took develop's toolbar from three tools to six and the settings panel to
fifteen groups.

Two concrete moments where Leyline is mute today:

1. **An empty library** shows an empty grid. Nothing says that importing is
   needed, nor how; the only indication is an `N: new · B: add photo` in grey
   at ten pixels, at the bottom of the side panel.
2. **The develop view** opens fifteen collapsed groups whose names — *HSL
   Mixer*, *Color Grading*, *Creative LUT*, *Soft Proof* — mean nothing to a
   beginner. The two groups open by default (white balance, tone) are the right
   ones, but they are drowned in the list of the thirteen others.

**What is not at issue.** No feature is removed, no shortcut changes, and no
stored setting is touched. This decision bears only on what is **shown first**.

## Decision

### 1. An empty library says what to do, where one is looking

When the grid holds no photo, it shows in its place the application's name, a
sentence, and the two gestures that bring photos in: importing a folder, or
watching a folder. Both buttons open the dialogs that already exist — nothing
new behind them.

The shortcut stays displayed where it is. A shortcut is discovered after the
fact; it does not replace a front door.

**And an empty grid is two different situations.** A library that holds
nothing wants photographs imported; a library whose filters, search, folder or
collection happen to match nothing wants those criteria widened. Offering
*Import…* to someone whose only mistake was a three-star filter sends them the
wrong way, and telling someone with 38 000 photographs that they have none is
simply false. So the block reads the query, not the row count: when the query
narrows the library at all, it says **no photograph matches**, explains that
the library is not empty, and offers the one gesture that gets out —
*Show all photos*, which drops every criterion at once and leaves the sort
order alone.

Which of the two it is is not guessed from the filter chips the interface
mirrors: `GridQuery::narrows()` answers it in the catalog, beside the struct
whose every field is a criterion, so a criterion added later cannot quietly
stop counting (ADR 0045 §1 — the UI never decides what matches). One button
rather than one per criterion: several may be on at once, and an empty grid
cannot point at the one to click.

### 2. Develop opens in **Basic** mode, and **Full** mode is one click away

Two levels of disclosure, chosen by a switch at the head of the panel:

| | Basic (default) | Full |
| :--- | :--- | :--- |
| Groups | White balance, Tone, Presence, Lens correction, Detail, Geometry, History | all fifteen |
| Tools on the image | Select, Crop, Target | all of them |

> **Amended by [ADR 0130](0130-direct-manipulation.md) §3**, which adds
> *Target* to Basic. It has to be there: the sliders it drives are Basic's
> own, and a tool that moved *Exposure* from a mode hiding *Exposure* would
> be incoherent — and of the three it is the least technical, asking only
> that the user point at what is wrong.

The split is not arbitrary: **Basic holds what has an obvious equivalent in any
photo tool** — a temperature, an exposure, a crop, a lens-correction switch.
Full holds what presupposes knowing what one is looking for: local masks, LUT,
soft proofing, DCP profile, curve, HSL, colour grading, spot removal, highlight
reconstruction.

**The order inside Basic follows the gesture, not the data model's order.**
White balance, then exposure, contrast, highlights, shadows, whites, blacks,
then texture, clarity, dehaze, vibrance, saturation: that is the order in which
Lightroom's *Basic* panel is taught, because it follows the way the eye reads
an image — light first, matter next, colour last. Two adjustments follow:
*Texture* comes before *Clarity*, and the **highlight shoulder**
(`highlight_rolloff`) leaves the sequence for Full mode, being an *output*
decision and not a presence one.

**And the sequence is *one* group.** White balance, tone and presence are no
longer three collapsible groups but a single *Basic* group, divided by two
non-clickable subheadings (*Tone*, *Presence*). A subheading that collapses is
an invitation to collapse it, and the sequence — light, matter, colour — is
precisely what one wants to read from end to end. Three consequences of form,
taken from the reference screenshots in `assets/`:

* **a setting fits on one line**: the name right-aligned in a fixed column, the
  track, the value. Two lines per setting halved how much of a modification one
  sees at a time; on one line, the whole of tone fits in a screen;
* **the track shows what the setting does** when it can: temperature runs from
  blue to amber, tint from green to magenta, vibrance and saturation from grey
  to colour. The tonal settings keep a grey track;
* **a double-click returns a setting to neutral** — zero almost everywhere,
  6500 K for temperature, clamped to the range for a setting that does not
  reach zero. And the explicit sign (`+12`) appears only on two-way settings: a
  temperature in kelvins is a quantity, not a departure.

Under the histogram, **four shot values**: sensitivity, focal length, aperture,
shutter speed, spread across the width. That is what one checks *while
correcting* an exposure — "was it already at 3200 ISO?" — whereas the rest of
the metadata (file, dimensions, body, lens, keywords) stays where it is, in the
library panel. They describe the file and not the revision: they come from the
catalog, change only when the photo changes, and a file that recorded none does
not show a line of dashes — the line disappears.

The panel no longer lists the presets: it is the *modification in progress*,
not the library of modifications. Saving, applying and deleting a preset live
in the **Develop** menu, where the first two already were.

Three properties to hold:

* **the mode changes no rendering.** A setting made in Full stays active and
  visible in its group, even if the group is hidden in Basic — hiding a panel
  resets nothing. That is what distinguishes progressive disclosure from a
  degraded mode;
* **it is stored nowhere.** Not in a revision, not in a preset, not in a
  configuration file: it is interface state, it lives for the window's
  lifetime, and Rust never reads it
  ([ADR 0045](0045-studio-ui-modularisation.md) §2);
* **switching to Full is reversible and immediate**, with no dialog and no
  restart.

### 3. Basic is the default, including for those who already know the application

> **Amended by [ADR 0128](0128-remembered-interface-state.md).** The reason
> below — *a configuration file the project does not have* — expired when
> [ADR 0078](0078-preferences-panel.md) created one. The mode and the group
> folds are now remembered; what stands is the paragraph's real argument,
> about the **first** launch, which still opens exactly the interface
> described here.


The opposite — remembering the last mode — would require storing a preference,
hence a configuration file the project does not have, and this decision does
not justify creating one. An experienced user clicks once per session; a
beginner has no "once" to give.

### 4. Out of scope

* **A guided tour, tooltips, a first-run assistant.** Software that needs to be
  explained on top of its interface has a problem in its interface.

  > **Narrowed by [ADR 0127](0127-hints-on-wordless-controls.md).** The
  > sentence holds for a *labelled* interface, which is what it was written
  > about. It was never an argument about a control that carries no words at
  > all — a 10x10px square, a `⟲`, an eyedropper — and the interface has
  > acquired eleven of those since, this ADR's own `?` among them. A hint
  > appears on those and on nothing else.

  Applied afterwards to the one place that already did it: Develop carried a
  permanent strip of grey text listing six shortcuts, across the row that has
  to show the photograph's name. It predated both the menu bar (which prints
  each shortcut beside its item) and `Help ▸ Keyboard Shortcuts` (which lists
  all six, grouped by where the key works), so it had become a third copy —
  and it was the module's longest string, markedly so in French, which made it
  set the view's minimum width at ~1200px. It is replaced by a **`?`** at the
  end of the toolbar, one glyph wide, opening the card that already holds
  them. The reminder stays one click away; there is one list to maintain
  rather than two that drift.
* **Reorganizing the fifteen groups** or merging some. Perhaps justified, but
  that is a redesign, and it would be decided with real users rather than with
  assumptions.
* **A shipped set of presets** to start with. That would be an aesthetic choice
  by the publisher, which `docs/vision.md` refuses.
* **Translating the documentation.** Real, and unrelated to the interface.

## Consequences

* **The first screen stops being empty**, and the second stops presenting
  fifteen closed doors to someone looking for two.
* **Nothing is lost for the advanced user**: one click, and the interface is
  exactly the one from before this decision.
* **No setting, no rendering and no stored format changes.** This ADR adds not
  a line to `settings_json` and touches no stage.
* **The develop panel gains one more private state** (`basic-mode`), which
  stays on the UI side in keeping with ADR 0045 §2 — and the mechanical test of
  that rule (no corresponding `get_*`/`set_*` on the Rust side) goes on
  passing.
* **The toolbar becomes mode-dependent**, so the active tool must fall back to
  Select when leaving Full with a tool Basic does not show — without which a
  click on the image would draw an invisible mask.

## Alternatives rejected

* **Doing nothing, and writing a getting-started guide.** That is the answer
  the discussion thread gives for darktable, twice, and it proves the problem
  rather than solving it.
* **Hiding the advanced groups until they are used** (automatic disclosure). An
  interface that changes on its own is harder to learn than a stable one: one
  can no longer find what one saw yesterday.
* **Remembering the last mode in a configuration file.** §3: it would create
  the project's first preferences file for a switch.
* **A third, intermediate mode.** Three levels require understanding the split
  before choosing, which is exactly the problem being addressed.
* **Removing features from Studio.** The thread does not complain about what
  the tools do, but about what they show up front.
