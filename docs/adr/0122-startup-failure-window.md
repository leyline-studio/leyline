# ADR 0122 — A start that fails says so: the window for the failure that has no console

**Status:** Accepted — 2026-09

## Context

Leyline Studio's `main` does this, and has always done it:

```rust
Err(message) => {
    eprintln!("error: {message}");
    std::process::ExitCode::FAILURE
}
```

On a terminal that is exactly right. On the way Studio is actually launched it
writes to nothing at all. The shipped Windows build carries
`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` — it is
deliberate, and [`main.rs`](../../crates/leyline-studio/src/main.rs) explains
why: without it the linker gives the process a console window that *hosts* it,
and closing that console kills Studio. The cost of not having one is that
`eprintln!` has nowhere to go. Double-clicking the Start Menu entry then
produces **nothing whatsoever**: no window, no message, no flicker. The AppImage
and the macOS `.app` are launched the same way, from a desktop with no console
attached.

This exact shape has been met before, and fixed for one cause only. A launch
with no argument used to return `Err("usage: leyline-studio <library-dir>")`,
which looked to every user like an instant silent crash; the answer was to stop
failing — fall back to a library under `Documents`. That removed the commonest
cause and left the mechanism intact for the others.

What can still refuse to start, before anything is on screen:

* an explicit path argument that is not a library, or is a typo — deliberately
  still fatal, so a script cannot silently create a library in the wrong place;
* a catalog written by a **newer** engine (`LeylineError::NewerCatalog`), which
  is what an older Leyline meets after the user has tried a newer one;
* a migration that fails, or a `catalog.db` that cannot be read at all;
* no home directory to resolve, on a stripped container or a service account.

None of these is common. All of them are, today, an application that does not
open and does not say why — the single worst outcome in a session where someone
is trying the program for the first time, because there is nothing for them to
report.

The CLI needs none of this. It has a console by definition, and
[ADR 0019](0019-distribution-i18n.md) already keeps it English-only for the
same family of reasons. This decision is about the GUI alone.

## Decision

### 1. A failure **before** the event loop gets a window; a failure **of** the loop does not

`run()` stops returning a bare `String` and returns a two-armed error instead:
a start that was **refused** — nothing has been on screen, the event loop has
never run — and a **loop that ended badly**, which is a different animal.

Only the first gets a window. Two reasons, and the second is the one that
decides it:

* after `window.run()` returns, the event loop is spent. Whether a second one
  may be started in the same process is a backend question, and an error path
  is the worst possible place to depend on the answer;
* by then the user has *seen* the application. An exit after a session is a
  bug to report, not a launch that vanished; the betrayal this ADR is about is
  specific to never having shown anything.

The loop-failure arm keeps `eprintln!` and `ExitCode::FAILURE`, unchanged.

### 2. The window **explains**; it does not repair

It shows a heading, the sentence the engine gave, and — when the failure knows
one — the path that was tried. Its only action is to close.

No "choose another library", no "repair this catalog", no "retry". Each of
those is a feature with failure modes of its own, needing a picker and a second
startup path, in the one code path that must not itself fail. And a window
offering to open a *different* library is no longer an error message: it is a
launcher, and Studio has one front door
([ADR 0055](0055-lightroom-navigation.md) §2 keeps Open Recent inside the
running application, relaunching into a choice the user made there).

The thing that is missing today is being told. That is the whole of what this
adds.

### 3. A Slint window, not a system message box

`rfd` is already a dependency and does have `MessageDialog` — but not on the
backend this project builds against. `rfd` 0.15's default features select the
XDG desktop portal, which has no message-dialog API at all, so `MessageDialog`
is not compiled on Linux; reaching it would mean switching `rfd` to its GTK
backend and taking a GTK system dependency, for an error path.

Slint is linked already, needs nothing new on any of the three platforms, and
brings something a native box would not: the window goes through `@tr(...)`
like the rest of the interface, so a French user is told in French that the
program will not start.

### 4. Translated, best-effort, and never fatal itself

The order is forced by the mechanism and is worth stating: **create the window
first** — the generated constructor is what registers the bundled language
list — then apply the stored or system language, then set the text.

Every step of it is best-effort, because in this path the preferences file may
be exactly what failed. A language that cannot be resolved leaves the English
strings `@tr(...)` is written in, which is the same fallback the main window
already has. If the error window itself cannot be created, `eprintln!` has
already run and the process exits `FAILURE` as it does today: this may improve
the outcome, it may never make it worse.

**The engine's own message stays in English.** That is not an oversight and not
new: every engine reason reaching Studio's interface arrives verbatim
(`events.rs` puts a `JobResult::Failed` reason on screen unchanged). Translating
`LeylineError` is a separate decision about the *engine's* surface, and
[ADR 0019](0019-distribution-i18n.md) does not ask for it.

### 5. And two failures that should never have been failures

Found while listing what can refuse to start, and corrected in the same change,
because the best error window is the one nobody sees:

* `preferences_path()?` aborted startup when no config directory could be
  resolved — three lines under a comment that says *"A config directory that
  cannot be read or written is not a reason to refuse to start — every failure
  here lands on the defaults"*;
* `recent_libraries_path()?` and `save_recent_libraries(...)?` did the same,
  under a comment that says *"A failure to read/write this file is never fatal
  to opening the library itself"*.

In both cases the reading half was already tolerant and only the path lookup
and the write were not. A photographer must not be kept out of their own
library because "Open Recent" could not remember something.

### 6. Out of scope

* **A log file.** Worth having, and a decision of its own — where it lives, what
  it holds, how it is rotated, and what it must never contain.
* **Any kind of report sent anywhere.** [`vision.md`](../vision.md): no network,
  and an error path is not an exception to that.
* **Turning warnings into windows.** Only a refusal to *start* gets one. What
  Studio can carry on without keeps going to the status line, as it does now.

## Consequences

* One more exported Slint component, generated beside `StudioWindow` by
  `slint::include_modules!()`. It carries its own fixed size and is themed like
  the dialogs, so nothing about the main window's geometry constraints
  ([ADR 0045](0045-studio-ui-modularisation.md) §3) applies to it.
* Six new strings in the `.pot`, and their French translations.
* The classification — refused versus loop failure — is ordinary Rust and is
  unit-tested. The window itself is smoke-tested under Xvfb, by pointing Studio
  at a path that is not a library.
* No engine change, no schema change, no stage version, and not one pixel.

## Alternatives rejected

* **Keep `eprintln!` and tell users to run Studio from a terminal.** That is
  advice one can only give to someone who already reached you, which is exactly
  what a silent failure prevents.
* **Give the release build a console on Windows.** It would print the message
  and reintroduce the console-owns-the-process problem that
  `windows_subsystem = "windows"` exists to solve.
* **Never fail: fall back to a fresh library whenever the requested one cannot
  be opened.** It hides the very thing the user needs to know, and would
  silently create a second, empty library beside a catalog that is merely
  unreadable today — the surest way to make someone believe their photographs
  are gone.
