# ADR 0133 — A word that names nothing

**Status:** Accepted — 2026-09

## Context

[ADR 0054](0054-first-run-and-basic-mode.md) §4 refused tooltips in one
sentence:

> Software that needs to be explained on top of its interface has a problem in
> its interface.

[ADR 0127](0127-hints-on-wordless-controls.md) then reopened exactly one door
and nailed the rest shut: a hint exists **only** for a control that carries no
words. `Exposure` gets none; `⟲` does. Its test is *could this control have
carried a label?* — and if the answer is yes, the bug is the missing label.

Both are right about what they were aimed at. Neither answers the case that has
since accumulated. Counted on the source: Develop holds **78 sliders**, and
among their labels are

> `Texture`, `Clarity`, `Dehaze`, `Highlight roll-off`, `Sharpen masking`,
> `Roughness`, `Blending`, `Balance`, `Red / cyan`, `Range softness`,
> `Hue width`.

Every one of them carries a word. Not one of them names what it does to a
photographer who does not already know.

*Texture* and *Clarity* sit one row apart, both add local contrast, and nothing
on screen says which spatial scale each works at — that is not a missing label,
it is a missing **sentence**, and no label can hold it. *Dehaze* sounds like
ordinary language and is not: it is a contrast operation that will apply itself
happily to a photograph containing no haze. *Whites* and *Highlights* are two
sliders four rows apart, both about the bright end, and the difference between
them — one sets where clipping begins, the other bends what is below it — is
the single most-asked question about this kind of panel.

ADR 0127 §1's test returns *"it already has a label"* and stops. The question it
never asks is whether the label **means anything**.

## Decision

### 1. A second admission rule, beside ADR 0127 §1's

A control also gets a hint when its label is a **term of art**: a word whose
meaning is a convention of the craft rather than of the language.

The test, applied one control at a time:

> Could a photographer say what moving this control will do, from its label
> alone, before moving it?

* `Exposure`, `Contrast`, `Rotation`, `Opacity`, `Crop left`, `Amount` — yes.
  No hint.
* `Texture`, `Clarity`, `Dehaze`, `Roughness`, `Blending`, `Masking` — no.
  Hint.

And a label is judged **in the company it keeps**. `Whites` is a perfectly
clear English word and still fails the test, because `Highlights` is four rows
above it and the two are indistinguishable from their labels. Where a pair is
confusable, both members are hinted, and each names the difference — hinting
only one of them answers the question for whichever slider the reader happened
to point at.

This does not reopen ADR 0054 §4. What that refused was a *layer of explanation
over a labelled interface* — a guided tour, a first-run assistant, a tooltip
that repeats the label. A sentence saying what `Texture` means is not a
repetition of the word `Texture`; it is the only place that information exists
at all.

### 2. One sentence, and the neighbour it is confused with

A hint is not documentation. One sentence, at most ~110 characters, saying what
the control does — and, where §1's second paragraph applies, naming the
difference:

* **Clarity** — *Contrast over broad shapes. Coarser than Texture, gentler than
  Dehaze.*
* **Texture** — *Contrast over fine detail: skin, bark, fabric. Unlike
  Sharpening, it leaves edges alone.*
* **Whites** — *Where the brightest tones clip. Highlights bends what is below
  that point; this sets the point.*

No numbers, no ranges, no units: the number is on the slider and the range is
under the finger. A hint that needed two sentences is a control that needs
redesigning, and that is worth knowing.

### 3. It is on the **word**, not on the control

Hovering a slider's **label** shows its hint. Hovering its track does not.

Three reasons, and the first is mechanical: the track is where every gesture
lives — the drag, the wheel ([ADR 0126](0126-slider-precision.md) §3), the
double-click back to neutral — and a hint that appears in the middle of aiming
is precisely what ADR 0127 §3 already refuses when it forbids one during a
drag.

The second is that the word is what the question is *about*. Pointing at the
word one does not understand **is** the question, and no other gesture states
it as exactly.

The third is that it costs the panel nothing: the label column is 88px of
otherwise inert text, and it acquires a use.

### 4. The list of hints is a list of debts, and a test counts it

ADR 0127 §1's consequence is inherited and sharpened. The hints in the source
are the places where the interface is not self-evident, and a list of debts
nobody counts grows quietly.

So a test reads `develop.slint`, collects every slider's label, and requires
each one to be **either hinted or named in a written list of labels held to be
self-evident**. A new slider fails the test until somebody classifies it, and
neither answer is the default. The list of self-evident labels is short and
lives beside the test, where adding to it is a visible act.

### 5. Out of scope

* **The library.** A different vocabulary, and the survey that would find its
  terms of art has not been done. `Pick`, `Shot`, `Root` are all candidates;
  none is guessed at here.
* **Group headings.** *Presence*, *Detail*, *Effects* name a place in the
  panel, not an operation; what they contain explains them.
* **A `?` per group opening a longer text.** That is a manual with extra
  steps, and §2's one-sentence cap exists to stop it becoming one.
* **A manual, a tour, a first-run assistant.** ADR 0054 §4 stands.

## Consequences

* The hint layer has to **wrap**. It was a single 22px row sized to
  `preferred-width`, which was right for `Show highlight clipping    J` and
  would have drawn a 900px sliver for a sentence. It gains a maximum width and
  a height that follows its text.
* About thirty-five sliders and two rows of chips gain a sentence. The two rows
  are *highlight reconstruction* and *demosaic*, whose labels are `Clip`,
  `Blend`, `Rebuild`, `AHD`, `VNG`, `DCB`, `DHT` — four of which are acronyms,
  and therefore the most opaque controls in the application by some distance.
  `FilterChip` already carries a `hint` property from ADR 0127, so they cost
  nothing but the writing.
* Hints are translated like every other string, which makes them the largest
  single addition to the `.po` since the interface was first extracted.

## Alternatives rejected

* **Hinting every slider**, self-evident ones included. It would make the list
  stop meaning anything — §4's whole value is that it is short — and it would
  put a hint under the pointer everywhere one moved it.
* **A permanent description under each slider**, as darktable offers. It
  triples the height of a panel whose design premise is that the tonal set fits
  on one screen (ADR 0054 §2).
* **Hovering the whole row.** §3. It collides with three gestures on the
  track, and it is also less precise as a question.
* **Leaving it to documentation.** The photographer is in the panel with the
  photograph in front of them; a document they must go and find is a document
  they do not read, and the sentence is twelve words long.
