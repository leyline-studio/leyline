# ADR 0151 — A guide that ships with the binary

**Status:** Accepted — 2026-09

## Context

`docs/` holds eleven reading documents, a hundred and fifty ADRs and not one
page written for a photographer. Everything there answers *why the project is
built this way*; nothing answers *what happens to my files when I import them*.

The gap is not cosmetic. Leyline's three load-bearing ideas are exactly the
three a photographer cannot guess from the interface:

* the originals are never modified;
* the library **references** photographs, it does not contain them — unless
  import was asked to copy;
* one photograph can carry several versions.

The Help menu offers a keyboard card, an update check and a bug report. A
photographer who wonders whether ticking *Copy into library* will move their
card's files has nowhere to look, and the answer decides how their archive is
laid out for years.

The same absence is the last open row of the UX review that ADRs 0139–0146
closed, and it is a release blocker in a way the others were not: an
application shipped to someone who is not its author, with no page explaining
its model, teaches that model by accident.

## Decision

### 1. One self-contained HTML page per language, written by hand

Not Markdown rendered at build time: rendering Markdown means a crate in the
dependency tree, in a project that builds one pinned LibRaw from source
precisely so a deliverable holds no surprises ([ADR 0086](0086-decoder-in-the-promise.md)).
Not a PDF: it cannot reflow on the window the reader has, and it dates the day
a shortcut changes.

An HTML file with its stylesheet inside it opens in any browser on the three
platforms, reflows, prints, and is one file to copy. `docs/guide/<tag>.html`,
one per shipped language, written and translated like prose rather than
extracted like a string: a guide is paragraphs, and `.po` entries are the wrong
grain for paragraphs.

### 2. Embedded in the binary, never read from disk

`include_str!`, so the guide is in the executable. Three reasons, in order of
how much each has already cost this project:

* **Packaging cannot forget it.** Three deliverables are built three ways
  (AppImage, NSIS, dmg); a resource that must be listed in each is a resource
  that will be missing from one, and missing silently.
* **The version on screen is the version running.** A file beside the binary
  can be a guide from an older install; a guide compiled in cannot.
* **It costs nothing to check.** The page is a few tens of kilobytes of text
  next to a binary that embeds a RAW decoder.

Opening it writes the chosen page into the cache directory and hands the path
to the platform opener — `xdg-open`, `open`, `explorer` — the mechanism
[ADR 0123](0123-local-problem-report.md) §2 already uses for the report folder.
Best-effort in the same way: a browser that will not start is a browser that
will not start, and the failure says so.

### 3. The interface's own language chooses the file

Not the system locale, not the stored preference: the guide's tag is
`Tr.guide-language()`, a translated string whose English source is `en` and
whose French translation is `fr`.

That is the decision, not a trick. The language actually on screen is the one
Slint resolved, after the stored preference, the system locale and the fallback
to the source language have all had their say ([ADR 0019](0019-i18n-and-distribution.md)).
Asking Rust to redo that arithmetic is asking it to disagree with the menu bar
one day. And the day a third language ships, its `.po` carries its own tag: no
Rust line to remember.

A tag with no guide falls back to English rather than refusing — a missing
translation is a reason to read the guide in another language, never a reason
to be shown nothing.

### 4. It describes the application, not the project

The guide answers in the order a first session asks: what Leyline does with
your files, the first hour, culling, developing, output, organising, what to do
when something goes wrong, the keyboard. It names no crate, no ADR and no
process version.

Where it overlaps a project document, the project document stays authoritative
and the guide stays short: the reproducibility promise is three sentences about
reprocessing here and a specification in `pipeline.md` §5 there.

## Consequences

* The Help menu gains one row, above the keyboard card: the card answers *which
  key*, the guide answers *why*.
* `docs/guide/` is a new kind of document in a `docs/` tree that until now held
  only project documents. `docs/readme.md`'s map says so, so the distinction is
  not left to a reader's guess.
* Two files to keep in step with the interface. The keyboard section is the
  part that rots first, and it says where the authoritative list lives — the
  shortcut card, which is generated from the application itself.
* No engine change, no schema change, no stage version, no golden entry.

## Alternatives rejected

* **A web page on a site.** Leyline works offline, with no account and no
  network call the user did not ask for ([ADR 0077](0077-update-check.md)); a
  guide that needs a connection contradicts the product in its first sentence.
* **A help panel inside Studio.** It would need scrolling, styling, a search
  box and translation plumbing — a browser, rebuilt worse — and it would
  compete for the window with the photograph.
* **Markdown rendered at build time.** A build dependency and a second format
  to style, to save writing thirty lines of CSS once.
* **Extracting the guide into the `.pot`.** Paragraphs are not interface
  strings: a translator needs the page in front of them, and a `msgid` holding
  four sentences is a `msgid` nobody proofreads
  ([ADR 0019](0019-i18n-and-distribution.md) §3 keeps `.po` for the interface).
* **Deriving the shortcut section from the code.** The card already does that,
  and the guide links to it; duplicating the generation would put two lists in
  the deliverable that can disagree.
