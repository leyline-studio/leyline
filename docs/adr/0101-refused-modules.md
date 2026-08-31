# ADR 0101 — The modules Leyline will not have

**Status:** Accepted — 2026-08

## Context

The parity survey against Lightroom Classic ended with a list of whole
*modules* Leyline lacks: Video, Book, Slideshow, Web, Publish Services,
and export to DNG and PSD. Each has sat on that list as an open question,
which is the worst state for a question to be in — it gets re-asked, and
each re-asking costs the same thought.

This repository has an established practice of the **documented
refusal** — content-aware healing (ADR 0032), mozjpeg (ADR 0083), the
two mask detectors ADR 0073 named and set aside, a profile `Amount`
slider (ADR 0089). This ADR extends it to the modules, so the answer
survives the person who worked it out.

Nothing here is a statement about what the project may *ever* do. It says
what the delivered application does not contain and why, which is exactly
the register `docs/specification.md` §4 is written in.

## Decision

### 1. Video: refused

Leyline is a **RAW development platform**. A video module is not a
feature but a second application sharing a catalog: decoding (a codec
stack per format), a timeline, scrubbing, audio, and an export path with
none of the still pipeline's properties. `docs/pipeline.md` §5.1's
bit-for-bit promise has no meaning for a frame nobody can address.

What is not refused, because it is not the same thing: *cataloguing* a
video file as an opaque asset the library can list. That would be a small
decision of its own the day someone wants it, and it would not make
Leyline a video editor.

### 2. Book, Slideshow, Web: refused

Three layout-and-output modules, legacy in Lightroom itself — Adobe has
not meaningfully developed them in a decade, and their output formats
(Blurb, HTML galleries) date them precisely. Each is a **layout engine**,
which ADR 0036 §V2 already identified as materially larger than the
printing it declined to grow into. A photographer who wants a book uses
a book tool, and what they need from Leyline is an export folder — which
exists.

### 3. Publish Services: refused

Uploading to Flickr, SmugMug or a social network means credentials,
tokens, per-service APIs that change under you, and a background process
talking to the network. Every one of those contradicts Local First in
spirit even where it would not violate it in letter: the application
authenticates against nothing (`specification.md` §4), and a publish
service is an account by another name.

Export to a folder, plus whatever tool the photographer already uses to
upload, does the job without making Leyline responsible for someone
else's API deprecation.

### 4. DNG in writing: refused. PSD: refused

Leyline **reads** DNG (ADR 0004, through LibRaw) and that stays. Writing
one is a different project: DNG is a TIFF/EP profile with mandatory
colorimetric tags, opcode lists, optional lossy compression and an
embedded preview whose correctness is judged by other people's software.
Writing a *nearly* correct DNG is worse than not writing one, because the
file looks fine until it does not.

PSD is a layered-composition format for an application Leyline is not.
Sixteen-bit TIFF, already exported, is what carries a developed image
into a compositing tool.

### 5. Quick Develop: refused, and the reason is that it already exists

Lightroom's Quick Develop applies *relative* corrections to a selection
from the grid. In Leyline the two gestures it exists for are already
delivered and are better: batch preset application (ADR 0058), which
records provenance, and copy/paste of develop settings, which acts on a
whole selection. Both produce ordinary revisions with ordinary history;
a third path with its own semantics would add a way to change a photo
that undo, history and provenance treat differently.

## Consequences

`docs/specification.md` §4 gains a row per refusal, in the register that
section is written in: what the delivered application does not contain.
The parity survey's open module list is closed.

## Rejected

* **Leaving them unanswered** — the state the survey was in. An open
  question is re-asked, and the second answer is rarely the same as the
  first.
* **"Later"** — for these, later is a way of not deciding. HDR and
  panorama are genuinely deferred (`specification.md` §4 says so, and an
  ADR will settle them); these are not on that list, and pretending they
  are would devalue the entries that really are waiting.
