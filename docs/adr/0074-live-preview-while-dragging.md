# ADR 0074 — The rendering follows the slider

**Status:** Accepted — 2026-08

## Context

One develops a photo blind.

`EditSlider` (`ui/widgets/controls.slint`) emits `edited(…)` only on
`PointerEventKind.up`: throughout the drag, the handle and the number move,
**the image does not**. One releases, looks, and starts again. Setting an
exposure or a clarity therefore takes a series of successive attempts where
every program on the market shows the result under the finger.

### What that reveals

[ADR 0041](0041-interactive-preview-rendering.md) speaks, from beginning to
end, of the "**cost of a slider move**": it quantifies what a render costs "on
every slider move" (§1), and its consequences announce that this cost "falls by
two factors that multiply". Its whole reasoning presupposes **continuous
rendering during the gesture**.

That rendering did not exist. The engine was made fast for an interaction the
interface never wired — and it stayed invisible precisely because both halves
were correct on their own. It is the second time in the pre-freeze pass that a
gap between a decision and its implementation hides in a silence rather than in
a contradiction.

The figure that makes the correction possible is already measured: ADR 0041
§3's stage cache brings an end-of-pipeline slider down to **~14 ms** on a
1024×683 test card. §3 below shows that live rendering actually costs four
times that — for a reason that is not the pipeline — but the order of magnitude
stays that of an interaction, not of a wait.

## Decision

### 1. During the drag, the rendering follows

`EditSlider` gains a second signal, emitted **during** the move, beside the one
on release. The first shows, the second commits:

| Signal | When | What it does |
|---|---|---|
| `previewing(value)` | on every move, bounded by §3 | renders and displays, **writes nothing** |
| `edited(value)` | on release, and on the double-click reset to neutral | what it already did: applies and **commits** |

### 2. No catalog write during a gesture

That is the constraint that makes the rest safe, and it was already provided
for: `EditSession::set` is "a value in memory, a real-time preview — no catalog
write" ([`engine-api.md`](../engine-api.md) §10.1). Live rendering therefore
takes that path — set the value into a session, read its `Settings`, render,
**abandon the session without committing** — and the commit stays on release,
unchanged, amendment window included.

Three things follow, all of them intended:

* **no revision is created by a drag.** A slider traversed end to end produces
  one revision, not fifty;
* **the preview cache is not touched.** A live render is neither written to
  disk nor recorded as a revision's valid preview: it is a *view*, on the same
  footing as the comparison's "before" or the mask overlay (ADR 0071). The disk
  does not grow by a byte while one is adjusting;
* **the §5.1 promise is not in play**: none of this is an export render, no
  stage version appears, and no pixel is recorded.

### 3. Live rendering goes through the stage cache, and is bounded in time

It borrows `render_scaled_cached` — ADR 0041 §3's path, the one the cache was
built for: moving an end-of-pipeline slider replays only what is downstream,
and the decode comes from `DecodeCache`.

Rendering being **synchronous** on the interface thread, a move event waits for
the previous render. One bound therefore suffices, and only one: **at most one
live render every 40 ms** (25 frames per second at most, the limit being the
eye and not the machine). A move arriving within that delay is ignored — never
queued: what matters is the slider's **current** position, not the path taken
to get there. The final render, for its part, is guaranteed by the release's
commit.

**Measured, on real files** (`live_preview_keeps_up_with_a_finger`,
`--release`, a `Small` preview):

| File | Slider | Per frame |
|---|---|---|
| Canon 60D, 10 Mpx | exposure (rank 40) | **59 ms** |
| Canon 60D, 10 Mpx | sharpening (rank 190) | 64 ms |
| Canon 5D IV, 30 Mpx | exposure | **54 ms** |

That is ~17 frames per second: frankly usable, and beyond comparison with the
absence of feedback. But **two things stand out, and it is better to write them
than to discover them**.

First, the end-of-pipeline slider is **not** faster than the head one, when ADR
0041 §3 measured 14 ms against 60 on that same gap. The stage cache works — it
simply is no longer the dominant term: the per-frame cost is taken back
**upstream of it**, by the reduction of the decoded buffer to display size
(`proxy`), redone on every frame. That is ADR 0041 §1, which decides to reduce
*before* developing without saying that the result could be kept.

Second, the 30 Mpx is not slower than the 10 Mpx: the `half_size` decode and
that same reduction bring both down to the same number of developed pixels.

**What follows, and is not done here**: keeping the reduced proxy in cache,
beside the `DecodeCache` that already keeps the decoded buffer. The expected
gain is the largest in this whole document, and it touches no pixel — but it is
a caching decision of ADR 0041's, not of this ADR, and it deserves to be taken
with its own measurement.

> **A sequel, 2026-08-05.** It was: [ADR 0076](0076-proxy-cache.md) caches the
> proxy and brings those 59 ms down to **15 ms** (10 Mpx) and 54 to **11 ms**
> (30 Mpx). The table's figures above stay the ones that motivated the
> decision, they no longer describe the engine.

The 40 ms bound keeps its meaning either way: it does not bite today, where
each frame costs more than that, and it will bite the day the proxy is cached.

### 4. Scope: the sliders, and them alone

The develop panel's other gestures stay on release, and that is not an
oversight: a curve point, a spot, a gradient, a brush dab **create an entry** in
a list. They have no continuum to follow — there is nothing to show between the
start and the end of a click. The slider is the only control whose intermediate
value has a visual meaning.

## Consequences

* **One adjusts while looking at the image**, which is how a photo is
  developed. It is the most visible usage gap that remained against the
  established programs, and it demanded no rendering work — only wiring up what
  ADR 0041 had made possible.
* **~17 frames per second, measured** (§3), and the same figure from a 10 Mpx
  body to a 30 Mpx one. That is not a Lightroom's fluidity, it is the
  difference between seeing and not seeing. The dominant cost is identified and
  it is not the pipeline: the next measurement bears on caching the proxy.
* **Nothing changes for the CLI or the SDK.** `preview_live` is added beside
  `preview_before` and the overlay: the engine's third *view*, with the same
  rule — nothing hidden, nothing recorded.
* **A session is opened and then abandoned on every live render.** That is
  explicitly what its contract permits; if it became costly, the answer would
  be to keep the session open during the gesture, not to give up the rendering.

## Alternatives rejected

* **Keeping the render on release** (the current state). That is the blindness
  described in the context, and it was justified only by a rendering cost ADR
  0041 divided by five.
* **Rendering in a background thread** and displaying when ready. That may be
  the sequel — at 59 ms a frame there is a perceptible lag behind the finger —
  but not before trying to cache the proxy (§3), which attacks the cause rather
  than its perception and introduces no arrival order to manage. Asynchronous
  rendering must guarantee that a stale frame does not overwrite a fresher one;
  that is complexity one takes on only if the per-frame cost resists.
* **Queueing the moves** rather than ignoring those that arrive too early. It
  would replay the slider's path after the fact, behind the finger: the
  opposite of what the decision seeks.
* **Committing on every move**, relying on the amendment window to merge them.
  It would make the history's integrity depend on a duration setting, would
  write to the catalog on every mouse pixel, and would invalidate the preview
  cache continuously.
