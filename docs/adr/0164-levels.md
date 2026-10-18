# ADR 0164 — Levels

**Status:** Accepted — 2026-10

## Context

[ADR 0112](0112-four-panel-affordances.md) refused a levels tool as an
ergonomic affordance, and said why: « levels is a tone operator wearing a
display's clothes, and it would need a stage, a version and a golden case.
It is a develop decision ». The Phocus parity report keeps it open as its
fourth gap: numeric black, gamma and white under the histogram, and output
limits. Photographers who learnt on Photoshop, Capture One or Phocus reach
for it first; it is the one tonal control whose numbers they already know.

Leyline can already produce every levels result with the point curve
([ADR 0030](0030-tone-curve.md), [0098](0098-per-channel-tone-curves.md)):
two end points and a midpoint. What it cannot do is let the photographer
*say* « black at 12, white at 240, gamma 1.2 » and read those numbers back
tomorrow. A curve that happens to implement a levels setting does not
remember that it was one.

## Decision

### 1. A stage of its own, rank 78

`levels`, version 1, at **rank 78**: after the parametric curve (75) and
before the point curve (80). The broad tonal sliders first, then the
endpoints and the midtone, then the precise curve that has the last word —
the order Capture One and Phocus apply levels and curve in, and the reason
[ADR 0137](0137-parametric-tone-curve.md) put the parametric curve before
the point curve.

A new stage and not a version of `tone_curve`, for the reason of
[ADR 0103](0103-red-eye-correction.md): absent from the stage map of every existing
revision, it changes none of their pixels, and `active` is false at
neutral, so the golden manifest gains entries and loses none.

### 2. The operator

On the display axis, like both curves (`in_display` + `display_curve`,
[ADR 0044](0044-linear-wide-gamut-working-space.md)), identically on every
channel:

```
t = clamp((x − in_black) / (in_white − in_black), 0, 1)
y = out_black + (out_white − out_black) · t^(1 / gamma)
```

with the four limits divided by 255. Gamma above 1 brightens the midtones,
which is Photoshop's convention and the one the numbers are known in.
Headroom above 1 is carried by the curve's own gain at white, as
`display_curve` does for every curve.

### 3. The settings

```rust
pub struct Levels {
    pub input_black: i32,   // [0, 254]
    pub input_white: i32,   // [1, 255], > input_black
    pub gamma: f64,         // [0.10, 9.99]
    pub output_black: i32,  // [0, 255]
    pub output_white: i32,  // [0, 255], >= output_black
}
```

Neutral: `0, 255, 1.0, 0, 255`, which `active` reads as « off ». The limits
are on the **0–255 scale** although the pipeline is floating point: it is
the scale the numbers are known in, and it is only a scale — nothing is
quantised to 8 bits. An inverted output (`output_black > output_white`) is
refused: it is a negative, not a level, and a negative is not a tonal
decision.

Serialised only when not neutral (`skip_serializing_if`), so a revision that
never touched it stores nothing.

### 4. Composite only

One set of levels for all three channels. Per-channel levels are the
per-channel point curves of [ADR 0098](0098-per-channel-tone-curves.md),
which already exist; a second way to cast a colour would be a second place
to look for one.

### 5. Presets, clients

* Presets: in the **Tone curve** group with both curves
  ([ADR 0137](0137-parametric-tone-curve.md) §5) — one block of the panel,
  one idea, *the tonal shape of this photograph*. Absent from a preset
  written before this ADR, which then leaves levels alone.
* Studio: a **Levels** group under the tone curve in Full mode, five
  sliders, a reset (⟲) and the look without it (◐,
  [ADR 0163](0163-a-look-without-one-group.md)).
* CLI: `develop <library> <version> levels <in-black> <in-white> <gamma>
  [<out-black> <out-white>]`, and `levels reset`.
* MCP: `apply_edits` takes `levels` as it takes every settings key.

## Consequences

* One stage, one module (`stages/levels/v1.rs`, frozen from its first
  commit), one golden case, no migration (a settings field with a default
  is read from every older `settings_json` as neutral).
* `stages/tests.rs::everything()` activates it, as every stage must.

## Rejected

* **Handles drawn on the histogram.** The histogram is a measurement, and a
  control drawn on it makes it look like one. Named, not built: if it comes,
  it is a view of these same five numbers.
* **Per-channel levels** — §4.
* **Auto levels.** An automatic tool is a choice, never a default
  ([ADR 0084](0084-assisted-culling.md), [0088](0088-auto-tone-and-black-and-white.md));
  `Auto` on the Basic panel already sets the endpoints from the histogram.
* **A version of `tone_curve`** — §1.
