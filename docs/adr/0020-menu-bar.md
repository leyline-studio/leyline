# ADR 0020 — The menu bar as a command surface

**Status:** Accepted — 2026-07

## Context

Leyline Studio is today driven entirely by mouse and keyboard shortcuts: `G`/`D` (library/develop), `I`/`E`/`N`/`B`/`Shift+B` (import, export, collection), `R`/`Shift+R` (reprocessing, ADR 0016–0018 §10.4), `S` (save a preset), `Ctrl+Z`/`Ctrl+Y` (undo/redo), digits/`P`/`X`/`U`/colour letters (classification). No window and no menu exposes that list anywhere but a one-line fragment visible only in develop mode (`studio.slint`) — nothing in library mode. A new user has no way of discovering `Shift+B` or `Shift+R` without reading the documentation.

ADR 0019 furthermore adds Preferences (a language choice) and has no natural place to put them: no menu, and no existing settings window.

## Decision

Leyline Studio adopts a **menu bar** (`File` / `Library` / `Photo` / `Develop` / `View` / `Help`) through Slint's native `MenuBar` component — rendered as the real system bar on macOS, and as a bar inside the window on Windows and Linux. Every entry shows its existing shortcut beside the label; the menu bar **adds no new functionality**, it makes visible and clickable what today exists only as a shortcut — the same Rust callbacks already wired (`on_run_import`, `on_develop_undo`, `on_reprocess_library`, and so on), with no new code path.

> **Correction, 2026-08-05.** The means changed, the decision did not. Slint's
> native `MenuBar` brings OS-level menu integration (`muda`) with it, which
> **breaks repaint scheduling** for the grid and the collection list: the
> window stopped redrawing until it was hovered. The bar is therefore
> **hand-written** in Slint (`ui/panels/menubar.slint` +
> `ui/widgets/menu.slint`), with the same structure, the same labels and the
> same shortcuts.
>
> What that costs, and it is accepted: on macOS the bar sits **inside the
> window** as it does on Windows and Linux, rather than in the system bar. The
> rest of this document — the structure of the six menus, the "no new
> capability" rule, the contract of each entry — describes the implementation
> as it stands, but for one entry: **Preferences…** is present but
> **disabled**. That is consistent with
> [ADR 0019](0019-distribution-i18n.md), which puts an explicit language
> setting "out of immediate scope" — the entry holds the place this menu map
> gives it, and will become active with the panel that fills it (see
> [ADR 0077](0077-application-updates.md) §2, which hangs a second
> expectation on it). **[ADR 0078](0078-preferences-panel.md) builds that
> panel and enables the entry**: the map below no longer has an exception.

The structure retained (it reflects what exists; `docs/engine-api.md` is authoritative for each action's real behaviour):

* **File** — Import…, Export…, ———, Preferences… *(new, ADR 0019: language)*, ———, Quit
* **Library** — New Collection…, Add to Collection, Remove from Collection, ———, Reprocess Library…
* **Photo** — Rate ▸, Label ▸, Flag ▸ (Pick/Reject/Clear), ———, Develop, Reprocess
* **Develop** *(active in develop mode)* — Undo, Redo, ———, Save Preset…, Apply Preset ▸, ———, Back to Library
* **View** — Library, Develop
* **Help** — Keyboard Shortcuts…, About Leyline

Context-sensitive entries (say, `Reprocess` under Photo, which today acts only on the photo open in develop) stay greyed out outside that context rather than silently changing behaviour — the same contract as the corresponding keyboard shortcut.

## Consequences

* No change to the event model or to the engine API: the menu bar is a pure UI layer on the `leyline-studio` side, and it adds nothing to `leyline-engine`/`leyline-sdk`.
* Existing keyboard shortcuts stay unchanged and take priority; the menu bar is a second way in, not a replacement.
* It becomes the natural home of Preferences (ADR 0019) and of a future "About" panel — no competing location to invent elsewhere.
* `Help ▸ Keyboard Shortcuts…` documents what the bar itself cannot show permanently (the single-key shortcuts of classification mode, digits for rating for instance) — still to be designed as a panel, outside this ADR's scope.

## Alternatives rejected

* **A single overlay menu (an avatar or kebab icon)**: lighter, but it does not solve discoverability — one still has to know an action exists in order to go looking for it in a generic menu rather than in a thematic hierarchy.
* **No menu at all, only a shortcut sheet (`?`)**: consistent with the current "everything by keyboard" style, but it asks a new user to learn the whole sheet before knowing what the application can do, rather than discovering it by browsing familiar menus.
