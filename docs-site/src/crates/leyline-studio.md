# leyline-studio

> Leyline Studio desktop application.

## What it does

The library browser and non-destructive editor: opens a library through
the SDK (never the catalog directly), renders the photo grid with cached
thumbnails, lets the user filter/sort/classify (rate, label, flag) photos,
shows metadata and keywords, and develops photos in a dedicated view
(`D` to enter, `G` to leave).

## Why it's built this way

Built on [Slint](https://slint.dev) (`docs/adr/0002-slint.md`) rather than
a web-view/Electron-style stack, in keeping with Local First: a native
GUI toolkit with no bundled browser runtime, no network stack implied by
the framework choice.

Studio depends on `leyline-sdk`, never on `leyline-engine` or
`leyline-catalog` directly (see [Architecture](../architecture.md)) — the
window/taskbar icon, menu bar and dialogs are all Studio-specific
presentation, but every action they trigger routes through the same API
surface the CLI uses. The menu bar is hand-rolled rather than using
Slint's native `MenuBar`, after the native one was found to break repaint
scheduling on Windows (`docs/adr/0020-menu-bar.md`,
`0021-context-menus.md`).

## See also

* `docs/adr/0002-slint.md`
* `docs/adr/0019-distribution-i18n.md` — packaging, i18n
* `docs/adr/0020-menu-bar.md`, `0021-context-menus.md`
* `docs/adr/0022-default-library-fallback.md`
* `docs/roadmap.md` phase 5
