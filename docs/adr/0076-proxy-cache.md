# ADR 0076 — The display proxy gets cached

**Status:** Accepted — 2026-08

## Context

[ADR 0074](0074-live-preview-while-dragging.md) §3 wired rendering to the
gesture, measured it on real files, and ended on a sentence that names what
comes next:

> "The dominant cost is identified and it is not the pipeline: the next
> measurement bears on caching the proxy."

The proxy is [ADR 0041](0041-interactive-preview-rendering.md) §1's buffer: the
decode reduced to the preview class's size **before** entering the pipeline.
ADR 0041 decides to reduce before developing; it says nothing of what becomes
of the result. Nothing became of it — `preview::proxy` rebuilt the reduced
buffer on every call, including for the fifty calls of a slider drag on the
same photo, at the same class, from the same decode.

That is an **entirely redundant** computation: the proxy is a pure function of
the source file and the requested size. Neither the settings, nor the revision,
nor the stage version enters into it. It therefore has exactly the nature of
what `DecodeCache` already keeps — and it was the only term of the live path
kept nowhere.

### What it cost, measured

`live_preview_keeps_up_with_a_finger` (`--release`, a `Small` preview,
i9-9900K), on two files from the real corpus, averaged over 20 frames after the
first:

| File | Slider | Before |
|---|---|---|
| Canon 60D, 10 Mpx | exposure (rank 40) | 55.4 ms |
| Canon 60D, 10 Mpx | sharpening (rank 190) | 56.8 ms |
| Canon 5D IV, 30 Mpx | exposure | 48.6 ms |
| Canon 5D IV, 30 Mpx | sharpening | 50.1 ms |

That table says two things. First, that the slider at the **end** of the
pipeline costs as much as the one at the head, when ADR 0041 §3 measured 14 ms
against 60 on that same gap: the stage cache does its job, but it now covers
only a fraction of the time. Second, that a 30 Mpx costs no more than a 10 Mpx
— both are brought down to the same 1024 px buffer before developing. Both
observations point at the same term: the reduction itself, redone on every
frame, independent of everything that follows it.

## Decision

### 1. The proxy is kept where the decode is already kept

`DecodeCache` stops being a cache of decodes and becomes a cache of **source
buffers**: the decodes, and the proxies derived from them. Two MRU lists in the
same object, under the same lock, discarded together with the `Library`.

A proxy entry is indexed by `(asset, DecodeParams, max_edge)`:

* `DecodeParams` is already the decode's key — it carries `half_size` and
  everything the `input` stage version asks of the decoder (ADR 0050, 0061,
  0066). The proxy therefore cannot survive a change that would modify the
  buffer it derives from;
* `max_edge` is the preview class, the reduction's only other parameter.

Nothing else enters the key, because nothing else enters the computation. That
is what makes this cache safe: like the stage cache, it is purely **derived**,
and discarding it at any moment changes no pixel.

**A hit skips the reduction *and* the decode.** The proxy is
self-sufficient: it survives the decoded buffer it came from if that one is
evicted first. That is intended — keeping 4 MB to avoid keeping 90 MB is the
right trade on the preview path.

`PreviewKind::Full` has no `max_edge` and therefore no proxy: that path renders
the decode itself, at scale 1.0, as before. An image already small enough does
not pay for a second copy either — the decoded buffer is registered as its own
proxy.

### 2. The ceiling is in bytes, not in entries

The preview classes span 250×: a `Thumbnail` proxy weighs 0.26 MB, a `Small`
4.2 MB, a `Large` 67 MB. A ceiling in number of entries would therefore mean
two incompatible things depending on the class. The proxy list is bounded in
**memory — 64 MB**, which holds a dozen `Small`s (the develop view's class, the
one being dragged) or a single `Large`.

**The most recent entry is always kept**, whatever its size: evicting the
buffer the caller is about to use would not give memory back and would lose the
cache.

The decode keeps its ceiling in entries (2): its sizes vary only by a factor of
3, and that ceiling is already written and understood.

### 3. What does not change

* **No pixel.** The proxy served is byte for byte the one a fresh reduction
  would produce, as the decode served is a fresh decode's. `docs/pipeline.md`
  §5 is not in play, nor is ADR 0012.
* **Export and printing** do not go through it: they render at full
  resolution, without a proxy (ADR 0041 §Decision).
* **ADR 0074 §3's 40 ms bound.** It was not biting; it bites now, which is
  exactly what ADR 0074 announced.

## Consequences

**Measured, same files, same machine, same test:**

| File | Slider | Before | After | |
|---|---|---|---|---|
| Canon 60D, 10 Mpx | exposure | 55.4 ms | **15.5 ms** | −72 % |
| Canon 60D, 10 Mpx | sharpening | 56.8 ms | **14.4 ms** | −75 % |
| Canon 5D IV, 30 Mpx | exposure | 48.6 ms | **11.3 ms** | −77 % |
| Canon 5D IV, 30 Mpx | sharpening | 50.1 ms | **12.6 ms** | −75 % |

* **We go from ~18 to ~70 frames per second** on the rendering itself. That is
  beyond what the 40 ms bound lets through: the slider is now limited by ADR
  0074's decision (25 frames/s, "the limit being the eye and not the machine")
  and no longer by the render's cost. The perceptible lag behind the finger ADR
  0074 noted disappears, and with it the reason it gave for contemplating
  asynchronous rendering: at 14 ms, the complexity of an arrival order to
  manage no longer pays for itself.
* **The end-of-pipeline slider becomes the cheapest again**, by a little. ADR
  0041 §3's stage cache takes back the place it had in its own measurement:
  what was masking it has gone.
* **64 MB more at worst**, beside the ~144 MB the decode cache can already
  hold. The figure is a ceiling, not a consumption: a develop session on one
  photo holds one `Small` and one `Thumbnail`, that is ~4.5 MB.
* **A third derived cache on the preview path**, after the decode and the
  stages. All three share the same property and that is what makes them
  tenable: discarding them is always correct, never necessary. None is
  persistent, and none has invalidation to write — the key describes the
  computation entirely.
* **The live render's dominant cost is no longer identified.** The remaining
  ~14 ms are spread between the downstream pipeline, the conversion to 8 bits
  and the display; none stands out enough to justify one more measurement while
  the 40 ms bound is what limits. The next preview performance question is no
  longer this one.

## Alternatives rejected

* **Keeping the proxy in the edit session**, as ADR 0041 §3 first provided for
  the stage cache. The same mistake, corrected in the same place: the develop
  view renders through `Library::preview_live`, which opens and abandons a
  session per frame (ADR 0074 §2). A cache carried by the session would be
  empty on every call.
* **A proxy cache separate from `DecodeCache`.** Two objects, two locks, one
  more parameter threading through four functions — for two lists of which one
  is computed from the other and whose key shares `DecodeParams`. The plumbing
  cost paid for nothing.
* **Keeping the proxy alone and discarding the decode.** Appealing on the
  preview path (the proxy suffices there) and wrong as soon as one leaves it:
  `PreviewKind::Full` and a change of preview class start again from the
  decoded buffer, which would then have to be re-decoded — ~1 s, against 90 MB
  kept.
* **A ceiling in number of entries**, like the decode's. It would have had to
  be sized either for `Large` (and keep only one `Small`) or for `Small` (and
  let 800 MB of `Large` through). The 250× spread between classes makes a
  number of entries meaningless here.
* **Persisting the proxies to disk.** The same answer as ADR 0041 for the
  stages: the reduction costs less than serializing and re-reading it, and it
  would add a cache artefact to invalidate between engine versions.
