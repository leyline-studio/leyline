# ADR 0022 — A default library on first launch

**Status:** Accepted — 2026-07

## Context

`leyline-studio` required a mandatory argument: the path of an already existing library (`leyline-studio <library-dir>`). Without it, `run()` returned `Err("usage: leyline-studio <library-dir>")`, printed by `eprintln!` and followed by `ExitCode::FAILURE`.

That error never has an audience: launched from a graphical shortcut — the Start menu entry the Windows NSIS installer creates, the Linux AppImage, a double-click on the macOS `.app` — none of those paths attaches a console. From the user's point of view the failure therefore looks like an instant, silent crash: the window never opens and nothing explains why. Observed in real conditions this evening: the user installed the freshly built Windows installer and launched it from Explorer — exactly that path.

The bug is not specific to Windows: the three platforms share the same `run()`, hence the same behaviour.

## Decision

When `leyline-studio` is launched **with no argument**, it no longer returns an error: it opens — or creates, on first use — a library at a default location, resolved through the `directories` crate (already a conventional choice, well maintained, a trivial dependency — the same family as `sys-locale`, already in the workspace):

* `<the user's Documents>/Leyline Library` on all three platforms (`UserDirs::document_dir()`);
* falling back on `<home>/Leyline Library` (`UserDirs::home_dir()`) if the system — a minimal container, say — exposes no Documents folder.

The folder is created if it does not exist (`std::fs::create_dir_all`), and then:

* if a `catalog.db` is already there (subsequent launches), `Library::open` — unchanged behaviour;
* otherwise (first launch), `Library::create` with the name `"Leyline Library"`.

**An explicit argument keeps the historical behaviour exactly**: `Library::open` alone, with no fallback and no automatic creation. A wrong or non-existent path therefore goes on failing outright — no change for scripts, for the CLI test harness, or for a user who already passes a path to their own library.

The resolved location is shown in **Help ▸ About Leyline** (the `about` dialog added by ADR 0020), below the existing description line — the only addition made to that dialog. No new setting, no dedicated window: a user wondering where their photos went can open that dialog and see the exact path, without a further preference having to be designed for it.

## Consequences

* A first launch with no argument is never silent again — either the window opens on a freshly created empty library, or (on subsequent launches) on the one already created in the same place.
* `default_library_root` is a pure function taking the candidate folders as parameters (with no direct call to `directories::UserDirs` inside): testable without touching the real home folder of the machine running the tests. `default_library_dir` is the thin call that connects it to the real user directories.
* A new dependency: `directories = "6"` (workspace), used by `leyline-studio` alone.

## Alternatives rejected

* **A folder-selection dialog on first launch** (a native `FolderDialog` asking where to create the library): closer to what a photo application's installer usually offers, but a wider scope than this evening's fix justifies — new UI, and a new first-launch state to design and translate (ADR 0019). A silent but discoverable default (through About) closes the immediate bug without committing to that design; a real folder choice on first launch remains a future improvement, worth its own ADR if it is decided.
* **An application data folder (`ProjectDirs::data_dir`, e.g. `%APPDATA%`/`~/.local/share`) rather than Documents**: technically closer to "app data" conventions, but a photo library is content the user owns and would want to find, back up or move themselves — Documents matches that use better than an application's hidden data folder.
