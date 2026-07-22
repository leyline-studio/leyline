# leyline-studio

```rust
{{#include ../../../crates/leyline-studio/src/main.rs:1:8}}
```

Built on [Slint](https://slint.dev) rather than a web-view/Electron-style
stack, in keeping with Local First (`docs/adr/0002-slint.md`). Depends on
`leyline-sdk` only, never on `leyline-engine`/`leyline-catalog` directly
(see [Architecture](../architecture.md)).

## See also

* `docs/adr/0002-slint.md`
* `docs/adr/0019-distribution-i18n.md` — packaging, i18n
* `docs/adr/0020-menu-bar.md`, `0021-context-menus.md`
* `docs/adr/0022-default-library-fallback.md`
* `docs/roadmap.md` phase 5
