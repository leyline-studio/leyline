# ADR 0140 — Labels that fit their column, in the language the panel is read in

**Status:** Accepted — 2026-09

## Context

The develop panel's label column is 88px wide with `overflow: elide`, a width
[ADR 0126](0126-slider-precision.md) arrived at by subtraction: 300px of panel,
minus a 46px value column, minus paddings, leaves 118px of track — and 88px was
what the labels seemed to need.

Seemed to need. Measured on the built binary with the French translation
loaded, at the font Linux actually renders (DejaVu Sans 12): **twenty of the
panel's fifty-five slider labels are cut**, from `RdB lumina…` to
`Récupération des hautes lumières` at 206px — more than twice the column. And
the same measurement in **English** finds eleven, `Horizontal perspective`
first at 135px: this was never only a translation problem, it was a column
nobody had measured in either language.

The Detail group, the most technical one in the panel, has four truncated
labels out of five: a photographer reading French cannot tell which
slider sharpens and which reduces noise.

Two of those twenty were self-inflicted in a way worth naming: `Luminance NR`
and `Color NR` are already abbreviations, invented to fit the column, and their
French translation « RdB luminance » is an abbreviation of an abbreviation — a
form that exists in no French photographic software.

The mitigation the interface already has is real but partial: hovering a label
shows its hint ([ADR 0133](0133-a-word-that-names-nothing.md)). A hint answers
*what does this do*; it does not restore a label you cannot **scan**.

## Decision

### 1. The column is 108px, and the twenty pixels come from the panel

The column goes from 88px to 108px, and the panel from 300px to 320px, so the
track keeps the 118px [ADR 0126](0126-slider-precision.md) measured its
precision against. Buying label width out of the track would have spent, on
legibility, exactly what that ADR spent a whole decision buying back.

108px is not a round number: it is « Hautes lumières » (98px) plus room for the
next language. The window's minimum stays 900px, where the panel is now a
larger share of a small window — accepted, because ADR 0125 §1 already folds
the *left* column below 1150px and the settings panel is what one works with.

### 2. What does not fit the column moves into a sub-heading

Twenty pixels do not fit « Récupération des hautes lumières ». Nothing would:
the phrase is a sentence, and the label column is not where sentences go.

So the long labels are **split**, using the `SubHeading` this panel already has
(*Tone*, *Presence*, and the optics group's own sections):

| Was | Is |
|---|---|
| `Luminance NR`, `Color NR` | **Noise reduction** ▸ `Luminance`, `Color` |
| `Sharpen amount/radius/masking` | **Sharpening** ▸ `Amount`, `Radius`, `Masking` |
| `Crop left/top/width/height` | **Crop** ▸ `Left`, `Top`, `Width`, `Height` |
| `Vertical/Horizontal perspective` | **Perspective** ▸ `Vertical`, `Horizontal` |
| `Luminance noise`, `Color noise` (mask) | **Noise reduction** ▸ `Luminance`, `Color` |
| `Defringe purple/green` (mask) | **Defringe** ▸ `Purple`, `Green` |
| `Range from/to/softness`, `Hue width/softness` | `From`, `To`, `Softness`, `Width` — the chip above already says which range |
| `Highlight roll-off` | `Roll-off` |

The word that was repeated on every row is now said **once**, above them. That
is shorter *and* better: five rows reading « Réduction du bruit — … » spend
five lines saying the same thing, and the group they belong to becomes visible
only after reading all five.

The rule this sets: **a label is judged with the heading above it.** `Amount`
alone is a question; `Sharpening ▸ Amount` is not.

### 3. `Default` is the one version name we wrote ourselves

The catalog writes the string `Default` as the name of every asset's first
version (`leyline-catalog/src/assets.rs`). It is data, so no `@tr` reaches it —
and a French interface therefore showed an English word nobody typed.

The interface translates that **one** name, at the point of display, and leaves
every other name exactly as the photographer typed it. Not renamed in the
catalog: the stored string is an identifier a library already carries and the
CLI already prints, and a migration to translate data would make the catalog's
content depend on the locale of whoever created it.

### 4. The guard learns about headings

`hints.rs` enforces ADR 0133 §4 — every develop slider is hinted or listed as
self-evident — by matching **label text**. Shortening labels broke it in both
directions at once, which is exactly what a guard is for: `Luminance` became
both hinted (noise) and listed (the HSL band), and the four crop numbers lost
the names the list knew them by.

The guard now keys on `Heading ▸ Label`, tracking the `SubHeading` in scope and
clearing it at a group or a conditional block. So the list reads the way the
panel does, and a future `Width` under some other heading is a new entry to
classify rather than one silently covered by this one.

## Consequences

* Fifty-five labels, none cut, in both shipped languages: measured, not
  eyeballed — the check is `convert -font DejaVu-Sans -pointsize 12 label:…`
  against 108px, and it is worth re-running when a translation lands.
* The develop panel is 20px wider. At 1280px, the width the window prefers,
  the photograph loses 20px of a 610px viewer.
* Sixteen new translatable strings, all one word; four long ones retired.
* The `SELF_EVIDENT` list is now written in the panel's own vocabulary, so
  reading it says which parts of the interface lean on their heading.

## Alternatives rejected

* **A wider column alone.** No column width fixes a 206px label, and every
  pixel of column is a pixel of track — the thing ADR 0126 was written to
  protect.
* **Truncating deliberately, with the hint as the answer.** That is ADR 0133 §1
  read backwards: the hint exists for controls that carry *no words*, not as a
  repair kit for words the layout cut.
* **Two lines per setting** (label above, track below). It doubles the height of
  a panel that already scrolls, against ADR 0054 §2's "a setting is one line".
* **Shortening only the French.** Eleven English labels were over the column
  too — `Horizontal perspective` at 135px, `Sharpen masking` at 107px — and a
  per-language layout is a per-language interface.
* **Renaming `Default` in the catalog.** §3.
