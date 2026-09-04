# ADR 0019 — Distribution: a per-platform installer, and FR/EN internationalization

**Status:** Accepted — 2026-07

## Context

Leyline Studio can only be launched today through `cargo run -p leyline-studio <folder>`: no installation mechanism, no translation, and an interface in hard-coded English. A V1 aimed at end users — not developers alone — must be installable without going through Cargo, and the team wants at least French and English from launch, with the ability to add other languages without touching the code.

## Decision

### A per-platform installer

Generated with `cargo-packager`, driven from a new `packaging/` directory at the root (per-OS configuration files plus scripts, **not a new Rust crate** — the same code/tooling separation as the rest of the workspace):

* **Windows**: an NSIS installer (`.exe`), the classic wizard with a "choose the installation folder" page — which answers the stated need directly.
* **macOS**: a `.app` bundle delivered in a `.dmg`, dragged and dropped into `/Applications` — the native macOS convention; no folder choice here, as that is not how the platform works and a "custom path" installer would look suspicious rather than practical.
* **Linux**: a portable AppImage as the first deliverable — the user places it wherever they like, the closest equivalent to an "installation folder" without depending on a package manager. A `.deb` can follow if the demand exists, but it does not offer the same folder flexibility (FHS paths are imposed).

### Internationalization (FR/EN, extensible)

Slint has native translation support: UI strings go through `@tr(...)` in the `.slint` files (and `slint::tr!()` on the Rust side), are extracted into `.po` files by `slint-tr-extractor`, and are loaded at runtime. The decision:

* Every visible string of Leyline Studio goes through `@tr(...)` instead of being hard-coded (they currently are, cf. `docs/roadmap.md` phase 5, already delivered — the extraction work remains, see Consequences).
* Two languages at launch: the existing English becomes the reference locale (the source of extractions), and French is the first translation added.
* System-language detection at startup, falling back on English if no translation exists for that language; an explicit setting may override it later (out of immediate scope — delivered by [ADR 0078](0078-preferences-panel.md) §3, which makes it switch live).
* Adding a language means adding a translated `.po` file, without touching Rust code or `.slint` files. **Corrected by [ADR 0078](0078-preferences-panel.md) §3**: now that a menu names the languages, a line in the table of native names is needed too — the list Slint exposes (`["", "fr"]`) carries no displayable name. A test compares `translations/` against that table, so that a `.po` added on its own fails instead of producing an empty menu entry.
* The CLI (`leyline-cli`) stays English-only for V1: a scriptable tool does not have a graphical UI's need for translation, and translating its messages would break parsing for any script that inspected them (`docs/engine-api.md` §1 — the CLI is a thin client, and its messages are not part of the API contract).

## Consequences

* A new `packaging/` folder (icons, per-OS build scripts, no application code) — the same logic as `docs/adr/0004-libraw.md`: build tooling stays separate from domain code. A correction made after implementation: the `[package.metadata.packager]` table itself lives in `crates/leyline-studio/Cargo.toml`, not in a standalone file under `packaging/` — `cargo-packager` only matches it up automatically with the crate's metadata (binaries, version, out-dir) when it reads that table from a workspace `Cargo.toml`, not through a file passed with `-c`. Only the paths to assets (icons, `.ico`) and the per-OS build scripts stay under `packaging/`.
* Two ways a translation goes missing without anything failing, both found on a cold re-read and both now fixed. **A plural `msgid` answered by a singular `msgstr`**: six entries — the grid's photo count, and the confirmations before removing photographs from the catalog or deleting them from disk — carried `msgstr "…"` where the template declares `msgid_plural`. A plural lookup reads `msgstr[N]` and finds nothing there, so those lines fell back to English for every count above one, in an interface that was otherwise entirely French. A count of empty `msgstr`s does not see it: the entry looks translated. What sees it is comparing the *shape* of each `.po` entry against the `.pot`'s. **A sentence assembled in Rust**: `format!("Exported to {}.", …)` is a string no `.pot` ever learns about, so the dialog answered an import in French and an export in English. The remedy is the same in both cases — Rust decides *which* of the cases holds, `Tr` in `ui/types.slint` words it.
* An inventory of Leyline Studio's UI strings to route through `@tr(...)` is a prerequisite before translation is actually effective — that is a piece of work of its own (Phase 8, see `docs/roadmap.md`), not done in one go with this ADR.
* Producing the three installers requires building on (or cross-compiling for) Windows, macOS and Linux; the `fmt`/`clippy`/`test` checks stay local as they are today (`docs/no-github-ci-yet` remains the decision in force — this ADR does not touch it).

## Alternatives rejected

* **cargo-dist**: aimed more at CLI binaries delivered through GitHub Releases (shell/PowerShell scripts, Homebrew formulae) than at genuine graphical installers with a wizard and a folder of one's choosing — less suited to a consumer desktop application like Leyline Studio.
* **gettext called directly, outside Slint**: it would reinvent what `slint-tr-extractor`/`@tr(...)` already does natively for Slint UI code, for no benefit.
* **Translating the CLI too, from V1**: deferred — see the last point of the decision.
