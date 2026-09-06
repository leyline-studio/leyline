# ADR 0045 — Modularizing Studio's interface: globals by domain, one file per panel

**Status:** Accepted — 2026-07

## Context

`crates/leyline-studio` is the only place in the repository where
`docs/contributing.md`'s "one responsibility per unit" rule has never been
held. Two files carry the whole interface:

* `ui/studio.slint` — 4,425 lines, of which ~4,000 are for the single
  `StudioWindow` component, which declares **111 properties and 73 callbacks
  flat** before nesting, inside a single `FocusScope`, the Develop view, the Map
  view, the grid, the details panel, ten modal dialogs and the menu bar.
* `src/main.rs` — 3,820 lines: twelve `wire_*` functions (`wire_dialogs` alone
  being 472 lines), the event pumping, the grid's windowing, the model building
  and the formatting helpers.

That is not an aesthetic problem. Three costs are measurable:

1. **No boundary says who has the right to write what.** The 111 properties are
   visible from any point in the tree; nothing prevents the export dialog from
   reading the retouching brush's state. Every slice shipped since Phase 5 has
   therefore widened the same flat surface.
2. **The cost of a modification grows with the file, not with the
   modification.** Adding a Develop setting means crossing a 4,425-line file to
   find its three anchor points, and the slightest brace error is diagnosed
   over the whole thing.
3. **External contribution is blocked by the size.** That is the point that
   forces the decision *now*: opening V1 to the public invites contributors who
   do not have the project's history, and a first interface patch should not
   require reading 8,000 lines.

The freezing of ADRs starts at publication ([[adr-editable-before-release]]):
this is therefore the last window in which to reorganize the UI surface freely
without having to supersede anything.

## Decision

**The interface's state is carried by Slint `global`s split by domain; every
panel and every dialog becomes a self-contained file that reads those globals
directly; the Rust wiring modules are the exact mirror of that split.**

### 1. One Slint `global` per domain, no properties on `StudioWindow`

Slint offers two ways of getting state down to an extracted component: plumbing
it as `in`/`out`/`callback` on each component, or exposing it in a `global` the
component reads with no intermediary. **The project chooses globals**, for a
reason that lies in the program's nature: Studio is not a library of reusable
components, it is a single application whose panels are singletons. A
`DevelopPanel` will never exist twice, with two distinct states, in two
windows. Paying 600 to 800 lines of binding plumbing for a reusability that
will not come would be a cost with no counterpart — and that plumbing would
live precisely in `StudioWindow`, that is, the monolith one is trying to
dismantle would stay large.

The domains, and their content:

| Global | Carries |
|---|---|
| `LibraryState` | the library's name/path, the application version, the status line, the recent libraries, opening/relaunching, quitting |
| `GridState` | the loaded cell window, the total, the selected index, the cell click, the viewport, classification (`classify`) |
| `FilterState` | everything that restricts or orders the grid: rating/label/pick/keyword filters, sort, search — those values are only a mirror of the same `GridQuery` on the Rust side and always move together |
| `DetailState` | the selected photo's metadata, its keywords and their addition/removal |
| `DevelopState` | the develop mode, the images (render and "before"), `DevSettings`, the histogram, the tone curve, the spots, the presets, the revision history, all the `develop-*` |
| `MapState` | the map mode, the rendered image, the pins, the tile pack, pan/zoom |
| `DialogState` | the open dialog and its result, and the input state of the ten dialogs (import, export, print, tethering, watching, collection, preset) |
| `CollectionState` | the collections, the active collection, membership |

`StudioWindow` keeps **no domain property**. It is left with its window
attributes (title, icon, minimum sizes), the keyboard shortcuts' `FocusScope`,
and the assembly of the panels.

### 2. Purely local state stays in the panel, never in a global

The counterpart of the choice above is that a global is a shared variable:
whatever is put there becomes visible from everywhere. The rule that bounds
that effect is simple and non-negotiable:

> **A global carries only the state that crosses the Rust ↔ UI boundary. State
> that concerns only one panel stays a private property of that panel.**

> **Amended twice.** [ADR 0128](0128-remembered-interface-state.md) makes the
> `expand-*` booleans and the Basic/Full mode cross the boundary — as **one
> opaque number per view**, because persistence *is* such a crossing and the
> rule above then permits it, while the meaning of every bit stays in the
> panel. And [ADR 0127](0127-hints-on-wordless-controls.md) and
> [ADR 0130](0130-direct-manipulation.md) add two globals — `HintState` and
> `DragState` — that carry interface state Rust never reads: both have their
> two ends in *different panels*, which is the case the rule's "concerns only
> one panel" does not cover.

Concretely, what stays local — and therefore disappears from the shared surface
— is the Develop panel's eleven `expand-*` accordion booleans, `active-tool`,
`dev-zoomed` (and the `changed develop-image` that resets it), `spot-pending*`,
`hsl-band-names`, as well as the menu bar's `open-menu` and its five
`*-submenu-open`. That is a third of `StudioWindow`'s current properties
ceasing to be global instead of becoming so.

When `StudioWindow` nevertheless needs to act on that state — the `FocusScope`
closes the menus on Escape, and the arrow keys must scroll the grid — it goes
through the panel's public surface (`menubar.open-menu`, a
`public function select-cell(int)` exposed by the grid panel) rather than
through a global. Reading a child's property is standard Slint; what changes is
that the panel chooses what it exposes.

### 3. The tree

```
ui/
  studio.slint            assembly + FocusScope, ~250 lines
  types.slint             structs (Cell, DevSettings, …) + the Tr global
  state/                  one file per §1 global
  widgets/                DetailRow, GroupHeader, FilterChip, EditSlider,
                          TopMenuLabel, MenuDropdown, MenuRow, MenuSep, …
  panels/                 develop, map, browser (grid + details), menubar
  dialogs/                import, export, print, tether, watch, collection,
                          preset, shortcuts, about
```

`types.slint` stays separate from `state/` because the structs are imported by
the generated Rust as much as by the UI, whereas a global is a runtime access
point: they are not the same objects, and mixing them would recreate a catch-all
file.

### 4. The Rust modules mirror the panels

`main.rs` is reduced to `main()`/`run()` and the bootstrap. The rest is spread
across
`src/wiring/{grid,filters,develop,dialogs,collections,keywords,presets,map,library}`
— one module per global, wiring that global and it alone through
`Global::<T>::get(&window)` (the mechanism already used for `Tr`) — plus
`src/{app,library,events,models}.rs` for the application state, the library
paths, the event pumping and the conversion to Slint models.

Two of those modules subdivide, because a single function in each exceeded 400
lines:

* `wiring/dialogs/` — one submodule per dialog, the exact mirror of
  `ui/dialogs/`, each with the validation helpers for its own fields.
* `wiring/develop/` — split not by dialog (the develop panel is a single
  surface) but by what an edit *is*: changing photo (`session`), changing a
  value (`adjustments`, `curve`, `spot`), moving within what has already been
  changed (`history`), and carrying settings from one photo to another
  (`clipboard`).

The existing pure-logic modules (`develop.rs`, `map_view.rs`, `format.rs`,
`classify.rs`) do not move: they already have one responsibility, and their
prime quality — knowing no Slint type, hence being unit-testable — is exactly
what `wiring/` cannot have. The proximity of name between `src/develop.rs`
(setting rules, no Slint) and `src/wiring/develop.rs` (the global's wiring) is
deliberate: it makes the logic/wiring separation visible.

### 5. A behaviour-preserving refactor, in shippable slices

No behaviour change, no rendering change and no new feature in this work — the
only deliverable is the structure. The order is constrained by the
dependencies: types and widgets first (no caller to change), then the globals
(that is the slice that touches the Rust in bulk), then the panels (which
become mere block moves), then the split of `main.rs`.

Every slice compiles, passes `cargo fmt`/`clippy -D warnings`/`cargo test
--workspace`, and is committed separately.

### 6. The SDK boundary does not move: Studio stays one third-party client among others

Studio has no privileged access to the engine. It consumes `leyline-sdk`
exactly as a third-party product would, on the same footing as the CLI:
`crates/leyline-studio/Cargo.toml` declares a single Leyline dependency, and
all the code in `src/` references nothing but `leyline_sdk`. That is what makes
the façade honest — if Studio needed a back door, it would be the sign that
something is missing from the public API, and the fix would be to widen the SDK
([[sdk-is-a-pure-facade]]), not to work around it.

This work is therefore **strictly upstream of that boundary**: it reorganizes
the interface and its wiring, and touches neither `leyline-sdk` nor what Studio
asks of it. In particular, no `wiring/` module may add a dependency on
`leyline-engine`, `leyline-catalog` or any other internal crate — the
temptation exists at the moment of splitting ("that module would need only one
type from `leyline-core`"), and it would turn an interface refactor into an
architectural regression. The single-Leyline-dependency `Cargo.toml` is the
guard: breaking it takes a visible line in review.

If the split reveals a real gap in the API — a need Studio met by a detour —
that is an SDK decision to handle separately, with its own commit, and not in a
modularization slice.

### 7. The `.pot` is regenerated at the end, not along the way

`slint-tr-extractor` records in the catalog the file and line of every
`@tr(...)`. Moving 4,000 lines therefore invalidates every reference, in
silence ([[i18n-extraction-workflow]]). The extraction is re-run once the new
tree is stable.

The point this work revealed and that must be remembered: a string's `msgctxt`
is **the name of the component that contains it**. Taking the dialogs out of
`StudioWindow` therefore changes the key of each of their strings, and a naive
regeneration of the `.po` would lose every French translation — with no error,
no warning, and an interface that compiles. The carry-over is done by `msgid`,
which is made safe by the fact that no `msgid` in the catalog had two distinct
translations (verified: 226 distinct `msgid`s, 226 translations). The final
check is to launch Studio in `LANG=fr_FR.UTF-8` and look at the screen.

## Consequences

* `StudioWindow` goes from ~4,000 to ~250 lines; no interface file exceeds a
  few hundred lines. A contributor wanting to fix the print dialog opens
  `ui/dialogs/print.slint`.
* The shared surface becomes explicit and bounded: what is in `state/` crosses
  the Rust boundary, what is not there does not. That is a property verifiable
  by reading, which the 111 flat properties did not allow.
* The cost is real and accepted: the panels are not reusable outside Studio,
  since they read singletons. That is acceptable because they are not meant to
  be (§1); if a component one day had to be instantiated twice, it would switch
  to `in`/`out` properties — the switch is local to the component concerned,
  not structural.
* The "globals" slice's diff is wide and mechanical (every `window.set_x(…)`
  becomes `window.global::<T>().set_x(…)`). It is entirely covered by
  compilation: an access left on the old surface does not compile.
* The SDK boundary comes out of this work as it went in: a single Leyline
  dependency in `crates/leyline-studio/Cargo.toml`, and `leyline_sdk` as the
  only import path in `src/`. The property is verified in two greps, before and
  after.
* `docs/architecture.md` gains a description of the `ui/` tree, and
  `docs/contributing.md` §2's rule, which is the one a contributor can break
  without noticing.

## Alternatives rejected

* **Plumbing `in`/`out` properties on each panel.** The orthodox option, and
  the right one for a component library. Here it costs 600 to 800 lines of
  bindings, leaves `StudioWindow` bulky, and buys a reusability the application
  has no planned use for. Retained as an occasional escape hatch
  (§Consequences), not as a rule.
* **A single `AppState` global.** One file instead of eight, but that is the
  monolith moved rather than dismantled: the surface stays flat and the
  question "who has the right to write what" stays unanswered.
* **Splitting the files without introducing globals**, relying on `root`'s
  visibility from nested components. Slint does not allow it beyond one file:
  an imported component has no access to its instantiator's scope. The option
  does not technically exist.
* **Splitting only the Slint and leaving `main.rs` at 3,820 lines.** Half the
  coupling would remain, and the later split of the Rust would reopen exactly
  the same areas — two passes where one suffices.
* **Deferring until after the V1 publication.** That is precisely the reverse
  of the need that motivates the work: the structure must be in place *before*
  outside contributors write code on it, without which they will write into the
  monolith and the debt will grow during the rework.
* **Taking the opportunity to retouch the ergonomics or the visual style.**
  Mixing a massive code move with behaviour changes would make any regression
  indistinguishable from a deliberate choice. §5 forbids it explicitly.
