# ADR 0147 — What the file already says about its author

**Status:** Accepted — 2026-09

## Context

The detail panel has two blocks. Above: *Camera*, *Lens*, *Exposure* — what the
**file** says, read from EXIF into `metadata` and replaced wholesale every time
the file is read again. Below, under *Description*: *Title*, *Caption*,
*Creator*, *Copyright* — what **someone wrote**, stored in `asset_descriptions`,
which [ADR 0099](0099-authored-descriptions.md) §1 keeps deliberately out of
reach of anything that reads a file.

The separation is right and stays. What is wrong is what the reader sees. Most
camera bodies let their owner set an Artist and a Copyright once, in the menus,
and then stamp them into every frame. Leyline **reads** that tag
(`exif.rs`, `Tag::Artist`), **stores** it (`metadata.artist`, `metadata.copyright`),
**indexes** it for search ([ADR 0144](0144-one-search-box.md)) and **exports**
it — `xmp.rs` writes `dc:creator` from the authored creator *and falls back to
the EXIF artist when there is none*. Measured on this project's own corpus:

```
$ identify -format "%[EXIF:Artist]|%[EXIF:Copyright]"  _MG_0997.JPG
Blackmorth|Tous droits reserves
$ sqlite3 catalog.db "select artist, copyright from metadata limit 1"
Blackmorth|Tous droits reserves
```

Eight files out of eight carry it. The one surface that says nothing about it is
the panel with a box labelled *Creator*. A photographer whose name is written
into every frame he owns looks at an empty box and concludes the library lost
it.

This is the fourth surface in two weeks where the engine knew something the
interface did not say — after [ADR 0143](0143-a-filter-worth-keeping.md),
[ADR 0145](0145-a-proposal-that-says-why.md) and
[ADR 0146](0146-the-details-one-notices-first.md) §1.

## Decision

### 1. The empty box shows what the file says, as a hint

*Creator* and *Copyright*, while empty, draw the file's value in grey:
« Blackmorth — d'après le fichier ». Typed text replaces it; the hint returns if
the box is emptied.

It is a **hint and never a value**. Nothing is written, `asset_descriptions`
gains no row, and the box is still empty in every sense the catalog cares about.
What the hint states is a fact about the export: that string is literally what
will be written to `dc:creator` if nothing is typed, because `xmp.rs` already
falls back to it. The box was not lying by being empty — it was silent about a
fallback that already exists.

*Title* and *Caption* get no hint: no EXIF tag means the same thing as either
of them, and inventing a correspondence would be worse than the silence.

Two measurements, both made on screen. The four description rows used a 72 px
label and 8 px of spacing after this change, which is what `DetailRow` uses for
every row above them: they were at 90 px and 6 px, so the labels of one block
did not line up with the labels of the other, and the field was 20 px narrower
than the panel could give it. And the wording is « — du fichier » rather than
the « — d'après le fichier » it started as, because the longer one did not fit:
at 300 px of panel the box holds about 172 px of text, and the first version
elided to « Blackmorth — d'après le fi… », cutting off exactly the half that
explains. A long *value* still elides — « Tous droits reserves — du … » — and
that is accepted: the value is the useful half there, and what carries "nobody
typed this" is the grey, not the words.

### 2. Rust writes the sentence

`DetailState` gains `file-creator-hint` and `file-copyright-hint`, already
assembled — the panel interprets nothing, the rule `detail.slint` states for
every other row. The wording comes from `Tr.from-the-file`, so it lives in the
`.pot` like every other string ([ADR 0019](0019-distribution-i18n.md)); a
sentence built in Rust is a sentence no catalogue ever learns about.

## Consequences

* The name a camera stamps into every frame stops being invisible, and the
  reader can see that Leyline read it.
* `asset_descriptions` is untouched, so ADR 0099's property holds exactly as
  before: nothing that reads a file writes authored text.
* Two properties, one `Tr` function and the label column of four rows. No
  engine, no schema, no migration.

## Alternatives rejected

* **Pre-filling the box with the EXIF artist.** It copies a file fact into the
  table of written text, which is the one thing ADR 0099 §1 exists to prevent —
  and the copy would then survive a re-read of a file that no longer says it.
* **A read-only *Author* row in the block above**, beside Camera and Lens. It is
  the more literal answer, and it puts two fields called "author" four lines
  apart: the present confusion, drawn twice.
* **Showing the hint on Title and Caption too**, from `ImageDescription` or
  `XPTitle`. Those tags mean something else in practice (a scanner's comment, a
  Windows Explorer field), and a hint that is wrong half the time teaches the
  reader to stop reading hints.
