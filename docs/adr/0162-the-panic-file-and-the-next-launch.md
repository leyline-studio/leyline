# ADR 0162 — The panic file, and the window that waits for the next launch

**Status:** Accepted — 2026-10

## Context

[ADR 0152](0152-a-panic-is-not-an-error-message.md) §5 asked for two things
that slice 5 (`eac1cf6`) could not deliver as written:

* the hook writes the message and the backtrace "to stderr **and to the log
  that the problem report joins**" — there is no such log.
  [ADR 0123](0123-problem-report-bundle.md) §5 states it: the report carries
  no log because there is none, and *when a log exists, it joins the folder
  as a fourth file*;
* "Studio shows it in a window built like the startup-failure window" — but
  [ADR 0122](0122-startup-failure-window.md) §1 refused exactly that for a
  loop that has run: after `window.run()` the event loop is spent, and
  whether a second one may start in the same process is a backend question
  that an error path must not depend on. A panic on the UI thread unwinds
  out of that loop. Nothing guarantees a window can follow it.

What the hook prints today reaches a terminal nobody launching Studio from a
menu has open. The panic is said, and heard by no one.

ADR 0152 is accepted and the project is public: its §5 is not edited, it is
made precise here.

## Decision

### 1. One file, not a log

The hook writes **`last-panic.txt`** in the config directory
(`~/.config/leyline/` and its platform equivalents), as well as stderr. It
holds: the five build lines of `Help ▸ About ▸ Build` (the same string, so it
cannot drift), the thread's name, the panic message and the forced
backtrace.

* **One file, overwritten.** The last panic is the one being reported; a
  history of panics is a log by another name, and a log is a separate
  decision about what Leyline records of a session — not taken here.
* **Studio only.** The CLI's user is at the terminal the backtrace goes to;
  a file beside it would be a second copy nobody asked for.
* **Best-effort.** A failure to write it is ignored: the hook runs while
  something is already wrong, and must never make it worse (ADR 0122 §4's
  rule).
* No clock in the text: the file's own date says when.

### 2. The window waits for the next launch

No window is opened inside the process that panicked. **At the next start**,
once the library is open and the main window is shown, Studio finds
`last-panic.txt` and shows a notice: *Leyline stopped unexpectedly last time.*
with two actions:

* **Report the problem…** opens the problem-report dialog (ADR 0123) with
  `last-panic.txt` already in its attachment list;
* **Dismiss** deletes the file.

The next launch is the one moment Studio is known to be healthy, with a
working event loop and the translations loaded. It is also when the
photographer is ready to say what they were doing — the description is the
part of a report only they can write.

A panic on a **worker thread** ends that thread, not the window: batches
already isolate one photograph (ADR 0152 §5), and the file is written all the
same. The notice then appears at the next launch too, which is late but
honest; telling the running interface is a channel this ADR does not build.

### 3. The report shows it like any attachment

`last-panic.txt` enters the report through ADR 0123 §3's list — by name,
with its size and its ×. It is **pre-listed, never silently included**: a
panic message can contain a file path, and the list is the promise. It is
the fourth file ADR 0123 §5 left room for; that section stays true.

The file is deleted once a report containing it has been written, or on
Dismiss. Removing it from the list with × keeps it on disk.

### 4. Slices

1. The hook writes the file (Studio only, best-effort); a test with a
   temporary config directory.
2. The notice at startup, its two actions and the pre-listed attachment,
   verified on screen in English and French.

## Consequences

* ADR 0152 §5's "log" is this file, and its window is this notice; nothing
  else in 0152 changes.
* A panic that kills Studio still closes the window without a word in that
  session. What changes is that it is no longer lost.
* A second panic before the next launch overwrites the first. Accepted: the
  most recent is the likeliest to match what the photographer remembers.

## Alternatives rejected

* **A window in the panicking process** — after an unwind out of the loop,
  or a fresh loop from `main`: depends on the backend letting a second loop
  start, which ADR 0122 §1 already refused to depend on.
* **A rolling log file** — the general session log is its own decision
  (what is recorded, how long it is kept, whether it names photographs);
  answering it as a side-effect of crash handling would decide it badly.
* **A file per panic** — piles up unseen, and the report would have to ask
  which one.
* **Including the file in the report without listing it** — contrary to
  ADR 0123 §3.
* **Telling the running window about a worker's panic** — a new channel from
  the hook to the UI thread, for a case batches already contain.
