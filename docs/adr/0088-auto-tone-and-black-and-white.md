# ADR 0088 — The two remaining buttons of Basic: Auto, and black & white

**Status:** Accepted — 2026-08

## Context

Lightroom's Basic panel carries three buttons above the sliders: `Auto`,
`B&W` and `HDR`. Leyline has none of them.

`HDR` is out: `docs/specification.md` excludes high dynamic range from V1
deliberately, and nothing here reopens that.

The other two are real gaps, and they are **not the same kind of gap** —
which is the whole of this decision:

* **Auto** does not render anything. It looks at a photo and decides where
  six sliders should sit. Its output is *settings*.
* **Black & white** renders. Its output is *pixels*, and pixels are what
  `docs/pipeline.md` §5.1 makes a promise about.

That difference decides everything about each of them: what has to be
versioned, what has to be frozen, what a golden fixture has to cover, and
what can simply be a function.

There is also a trap worth naming, because it is the obvious wrong answer:
**desaturating is not a black-and-white conversion**. Dragging `saturation`
to −100 collapses every hue to the same grey by the same rule, and a red
jumper and a green hedge of equal luminance come out indistinguishable. What
a photographer means by "black and white" is control over *which colour
becomes which grey* — Lightroom's B&W Mix, eight sliders, one per hue band.

## Decision

### 1. Auto produces settings, never pixels

`Library::auto_tone` reads the photo and returns the six tone values it
would set. It writes nothing by itself; the client feeds them through the
ordinary `EditSession`, so an automatic tone lands in the history as **one
revision like any other**, undoable, visible, and re-appliable.

Nothing about it enters the pipeline: no stage, no stage version, not a line
of `settings_json` that did not already exist. `docs/pipeline.md` §5.1 is
untouched by construction rather than by inspection — the same shape
ADR 0069 §2 gave closed extensions ("an extension produces settings, never
pixels") and ADR 0084 gave assisted culling ("a culler produces
keystrokes").

It follows that Auto is **not** part of the reproducibility promise, and does
not need to be: what is promised is that a revision renders the same pixels
forever, and a revision produced by Auto records the six numbers it chose,
not the fact that a machine chose them. Improving the algorithm later
changes what the *next* press produces and leaves every existing revision
exactly where it is.

### 2. What Auto actually computes

From the histogram of a small proxy render of the photo at its **current**
settings — not the neutral render: pressing Auto after moving the white
balance should answer for the photo as it now is.

Four decisions, in this order, each from a percentile and each clamped to
the slider's own range:

| Slider | From | Aim |
| --- | --- | --- |
| `exposure` | the median luma | bring it to middle grey (0.18 linear), in EV, clamped to ±2 EV |
| `whites` | the 99.5th percentile | just below clipping |
| `blacks` | the 0.2nd percentile | just above zero |
| `highlights` / `shadows` | mass above 0.9 / below 0.05 | recover only what is actually crowded there |

`contrast` is deliberately **left alone**. It is the one tone slider whose
right value depends on intent rather than on the histogram, and an Auto that
guesses it is an Auto people stop pressing.

The percentiles are computed on a proxy, which makes the result depend on the
proxy's size. That is stated rather than hidden: Auto is a suggestion, and
two proxies of the same photo suggesting values a hair apart is not a defect
in something whose output the photographer then edits. What must not vary is
the *render*, and the render is pinned by the six numbers Auto wrote down.

### 3. Black & white is a stage, and the mixer is the one already there

A new `monochrome` stage, ranked **145** — after `hsl` (140), before
`color_grading` (150). It collapses each pixel to its luma in the working
space and writes that to all three channels.

Its position is the whole design. Placed there:

* the eight **HSL luminance sliders become the B&W mix**, with no new field
  and no new panel. Raising `hsl[Red].luminance` brightens the reds *before*
  they become grey, which is exactly what Lightroom's B&W Mix red slider
  does. The answer to "desaturating is not a mixer" is not a second set of
  eight sliders — it is putting the collapse **after** the eight that exist;
* `color_grading` still runs on the grey image, so split-toning a black and
  white — sepia, selenium, a cool shadow — works without a line of new code;
* `saturation` and `vibrance` (ranks 120 and 130) run before the collapse and
  therefore stop mattering, which is correct: in black and white they have
  nothing left to act on.

One new field, `Settings::monochrome: bool`, neutral `false`. No schema bump:
every field added since schema 1 has been added the same way, with a serde
default, and an older revision that has never heard of it deserialises to
`false` and renders exactly as before.

### 4. A new stage is backward-compatible by construction

`pin` records the version of every **active** stage. `monochrome` is active
only when the flag is true, so:

* every revision written before this ships has no `monochrome` entry, has
  `monochrome: false`, and renders through exactly the stages it always did;
* a revision that turns it on records `monochrome: 1`, and keeps rendering
  through v1 forever, whatever v2 may one day do.

Nothing about this touches a published `(name, version)` pair, which is the
one thing the stage registry forbids. The golden manifest gains entries — it
never moves one.

### 5. The button is a toggle, not a preset

`B&W` sets one boolean. It does not zero the saturation, does not touch the
HSL mixer, does not "apply a black and white look". Pressing it twice
returns the photo to colour with every other setting where it was, because
the only thing that changed was the flag.

This is the difference between a button and a macro, and it is worth the
sentence: a button that quietly moved four other sliders would make its own
undo a lie.

## Consequences

* `leyline-core`: `Settings::monochrome`, and `SettingsGroup::Presence`
  gains it — a black and white *is* a presence decision, and a preset that
  captures presence should carry it.
* `leyline-engine`: the `monochrome` stage (`stages/monochrome/v1.rs`,
  rank 145), and `Library::auto_tone(asset) -> Result<AutoTone>` returning
  the six values.
* `leyline-cli`: `leyline develop <lib> <version> monochrome on|off`, and
  `leyline auto-tone <lib> <version>` which prints what it chose and commits
  it — a command line is where one checks what Auto decided.
* Studio: `Auto` and `B&W` beside the Basic group's header, where Lightroom
  puts them.
* `docs/pipeline.md` gains the stage in its operation order; `docs/presets.md`
  gains the field under Presence.
* The golden manifest gains entries for `monochrome::v1`. Existing entries do
  not move, and are not permitted to.

## Alternatives rejected

* **A dedicated eight-slider B&W mix**, separate from the HSL mixer. It is
  what a first reading of Lightroom suggests, and it duplicates eight
  settings, eight sliders, a preset group and eight more things to keep in
  step — to obtain, when black and white is off, eight sliders that do
  nothing. Ranking the collapse after the existing mixer gets the same
  control for one boolean.
* **Black & white as a develop preset** (saturation −100). No new stage, no
  new field, shipped in an afternoon — and it is the exact thing §Context
  calls the wrong answer: one grey per luminance, no control over which
  colour lands where.
* **Auto as a new stage** that decides at render time. It would make the
  photo's appearance depend on an algorithm rather than on stored numbers,
  so improving the algorithm would silently re-develop everything already
  developed with it — the exact reverse of ADR 0058 §7 and of the project's
  promise.
* **Auto guessing `contrast`, and a white balance**. Both are defensible and
  both are intent, not measurement. They can be added later without changing
  anything decided here, because Auto's output is a set of numbers and the
  set can grow.
* **`HDR`**: excluded from V1 by `docs/specification.md`, and not reopened
  here.
