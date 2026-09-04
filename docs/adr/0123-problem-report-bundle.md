# ADR 0123 — Reporting a problem: Leyline assembles, the photographer sends

**Status:** Accepted — 2026-09

## Context

A private test phase is starting, and a tester who meets a defect has to
describe it from memory. What actually makes a report usable is known and
small: what they were doing, what the window looked like, and which build they
were running — the last of which nobody can be expected to transcribe by hand
(`Help ▸ About ▸ Build` holds five lines including the *linked* LibRaw version,
which [ADR 0086](0086-decoder-in-the-promise.md) makes a term of the
[`pipeline.md`](../pipeline.md) §5.1 promise and which therefore cannot be
inferred from the version number).

The obvious shape for this is the one every application uses: a dialog that
**uploads** a screenshot and a log. That word is the problem.
[`vision.md`](../vision.md) refuses the network, and this is not a corner where
an exception costs nothing: an application that has never opened a socket and
then opens one *to send a picture of the user's photographs* has changed
category. [ADR 0122](0122-startup-failure-window.md) §6 already refused the
smaller version of the same thing for the startup error window.

But the *need* behind the word is not the network. It is that the report should
not be a blank page. Those separate cleanly.

**What does not exist yet.** There is no log. Not a file, not a framework: the
workspace has no `log` or `tracing` dependency, no call to either, and Studio's
ten `eprintln!` write to a console that a Windows release build does not have
([ADR 0122](0122-startup-failure-window.md)). Deciding what a log holds, where
it lives, how it is rotated and what it must never contain is a decision of its
own, deferred by 0122 §6 and still deferred here.

## Decision

### 1. Leyline assembles the report; the photographer sends it

`Help ▸ Report a problem…` writes a **folder**, and opens it. Leyline never
opens a socket, never names a destination, and never asks who the user is. What
the folder is then attached to — an email, an issue, a message — is the
photographer's business and Leyline does not need to know.

This is not a lesser version of uploading. It is the only version that leaves
the user in the position `vision.md` promises them: they can read every byte
that will leave their machine, before it leaves.

### 2. A folder, not an archive, and it is opened

A `.zip` would be tidier to attach and hides its contents behind a step. The
folder is opened in the system file manager for exactly the reason the archive
would be worse: **the point is that they look.** A screenshot of a photo
library *is* somebody's photographs, and the person who took them is the only
one who can say whether this particular frame may be sent to a stranger.

Opening the folder is best-effort — a spawned `xdg-open`/`explorer`/`open`, no
new dependency, and no failure of it matters. The dialog shows the path either
way, so a file manager that will not start costs a copy-paste and nothing else.

### 3. Two texts Leyline writes, and screenshots the photographer attaches

Always written:

* **`description.txt`** — what they typed, verbatim.
* **`build.txt`** — the five lines `Help ▸ About ▸ Build` shows, plus the
  library's path and schema version, the window size, and **the names of the
  attachments**. Assembled from the same string the About dialog reads, so the
  two cannot drift; naming the attachments is what lets someone holding only
  `build.txt` notice that something did not reach them.

Then, **optionally**, files the photographer attaches — `Attach a
screenshot…`, their own, taken with whatever tool they already use. Optional
is part of the decision and the dialog says so: a description on its own is a
report, and a feature that makes people hunt for a screenshot before they can
complain will collect fewer complaints.

Leyline does **not** take the screenshot itself (see the alternatives). The
one it could take is of its own window, and the photographer is the one who
knows what is worth showing.

**The list is the promise.** Every attachment is shown by name — with its size,
so a 60 MB file they forgot is visible before they send it rather than after it
bounces — and each one carries a × that takes it back out. It is shown before
the folder exists. A report that surprises its author when they open it has
already failed at the only thing this feature is for.

Two files chosen from two folders may share a name; the second is written
beside the first under a suffixed name, never over it. Losing evidence is the
one failure this feature cannot afford.

**What Leyline never puts in by itself**: the catalog or any part of it, a
photograph file, `preferences.json`, and the recent-libraries list — that last
one names *other* libraries, which is to say other directories of other
people's photographs, none of which is about the defect at hand. What the
photographer attaches is their own decision, and the list shows it back to
them.

### 4. Where the folder goes

`Documents/Leyline Reports/leyline-report-<seconds since the epoch>/`, falling
back to the config directory and then to the temporary directory.

The stamp is a plain epoch second rather than a formatted date: it sorts, it is
unique, and it does not put a date library in the workspace for the sake of one
directory name. The date a reader actually wants is the file's own.

Not inside the library: a library is portable and self-contained
([`catalog.md`](../catalog.md) §37), and a bug report is not library content —
it must not travel with the photographs, nor be one of the things a user
wonders about when they copy the folder to another disk. Not inside the config
directory by preference either: this is a document the user is meant to find,
open and attach, which is what `Documents` is for.

### 5. No log, and what that costs

The report carries no log because there is none to carry (§Context). Stated
here rather than left implicit, because it is the one thing a reader of this
ADR will look for: when a log exists, it joins the folder as a fourth file and
this decision does not change shape.

The cost is real and bounded: a defect that leaves no trace on screen and no
trace in the description is a defect this report will not explain. That is the
argument for deciding the log question — separately, on its own merits, and not
by smuggling a file format in through a feedback dialog.

### 6. Out of scope

* **Sending anything, by any means**, including opening a mail client or a
  pre-filled issue URL. That was weighed and left out: it delegates the network
  to the browser rather than removing it, and the folder is already attachable
  anywhere.
* **Catching panics** to report themselves. A crash handler is a different
  mechanism with different constraints, and this dialog needs a running
  application to be useful.
* **Any identifier of the machine or the user.** There is no report id, no
  installation id, and nothing that would let two reports be recognised as
  coming from the same person.

## Consequences

* No new dependency, and none needed: the folder is `std::fs`, the picker is
  `rfd` and the paths are `directories`, all already Studio's.
* Eight strings in the `.pot` and their French translations.
* One more dialog in `panels/dialogs.slint` and one menu entry, following the
  same shape as every other ([ADR 0045](0045-studio-ui-modularisation.md) §3).
* The report's *content* is assembled by pure functions — the `build.txt` text,
  the free name for a colliding attachment, the copy of one chosen file — so
  what goes in the folder is unit-tested without a display; showing and opening
  are the thin part.
* No engine change, no schema change, no stage version, and not one pixel.

## Alternatives rejected

* **Upload to a service.** The first derogation from the refusal of the network,
  for the convenience of the person receiving reports rather than the person
  sending them. It would also need a service, a retention rule and a privacy
  statement — three things this project does not have and does not want.
* **A pre-filled GitHub issue URL.** Tempting, since the repository is where
  reports would land anyway. But it opens a browser on a URL carrying the
  user's text, requires an account they may not have, and the repository is
  private, so the link would greet most testers with a 404.
* **A `.zip`.** Better to attach, worse to inspect, and inspection is the point
  (§2).
* **Leyline taking the screenshot itself.** Built first, then removed. It cost
  a real trap — `slint::Window::take_snapshot()` returns what has been
  *painted*, not the state of the model, so the first report ever made carried
  the Help menu that had asked for it, and the capture had to be deferred a
  frame behind a timer — and it bought less than it looked. Leyline can only
  photograph *its own window*: not an interface that has stopped repainting,
  not a second screen, not the file manager beside it, not the moment before
  the photographer thought to report. Their own screenshot tool sees all of
  that, they already know how to use it, and what they choose to show is a
  better signal than what Leyline would have guessed.
* **Attaching the photograph being developed, automatically.** It is the
  single most useful thing for a rendering bug and the single most private: a
  RAW file is the user's work, and nothing takes it out of their folder on
  their behalf. They can attach one when they judge it useful — which is the
  difference between a decision they made and one made for them.
