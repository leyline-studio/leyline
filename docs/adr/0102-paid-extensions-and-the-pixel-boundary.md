# ADR 0102 — Paid extensions, and what happens when an extension makes pixels

**Status:** Accepted — 2026-08

## Context

ADR 0069 drew the boundary for closed extensions in one sentence: **an
extension produces settings, never pixels.** ADR 0073 built the socket
that shape allows — a mask detector is a separate executable proposing a
coverage the free pipeline then renders like any other mask. Together
they cover the half of Lightroom's AI that matters most (Subject, Sky,
People masking) with nothing left to decide.

The other half does not fit: **AI denoising, lens blur, super-resolution
and content-aware removal produce pixels.** They are also, by the survey's
own measurement, where Adobe is pulling ahead fastest. So the boundary
has to be either extended or held, and this is the decision.

ADR 0069 §5 also left three things named and unsettled, and this ADR owes
them an answer: the paid licensing system, the requirement that
verification be offline, and the correction of `specification.md` §4's
subscription row.

## Decision

### 1. The boundary holds, and gains its second half

An extension produces settings, never pixels — and an extension that
*must* produce pixels produces a **new asset**, never a stage.

The denoised photograph enters the library as its own file, imported like
any other, developable by the free pipeline like any other. The original
is untouched and stays exactly where it was.

`docs/pipeline.md` §5.1 is therefore untouched **by construction**, for
the third time in this repository's history (ADR 0088's Auto, ADR 0084's
culler, ADR 0069's settings rule): no stage, no stage version, nothing in
`settings_json` that did not already exist.

### 2. Why an external *stage* is refused, and it is not squeamishness

The obvious alternative — let a paid extension supply a pipeline stage —
fails on a specific, checkable consequence rather than on principle.

A revision citing that stage cannot be rendered without the extension.
The repository already has a rule for a setting a pinned version cannot
express: refuse explicitly, never silently (the capability rule,
ADR 0048 §5, applied again in ADR 0096 and ADR 0098). Applied here, that
refusal would read *"buy this to see your own photograph"* — a purchase
becoming the condition of rendering something already developed.

`specification.md` §4 says, of subscription: **"never the right to run
what you have."** An external stage would make that sentence false. That
is the whole argument, and it survives any amount of enthusiasm about the
feature.

The same test disposes of a subtler variant: a first-party
`leyline-ai` crate, free and in-tree, whose *weights* are the paid
download. The stage would be free and the render would still be
impossible without a purchase. Same failure, better disguise.

### 3. The shape is corroborated, not invented

Lightroom's own AI Denoise does exactly this: it writes a **new DNG**
beside the original rather than adding a slider to the develop panel.
Adobe, with no reproducibility promise to keep and every commercial
reason to prefer a slider, still chose a new file — because a
neural denoise is not a parameter, it is a different photograph.

This is worth recording because the shape looks like a compromise and is
not one. The honest cost is stated instead: a derived asset is not a
slider you can dial back, and it doubles the storage of the photographs
it is used on. Both are consequences of what the operation *is*.

### 4. What may be sold, and what may never be

May be sold: a mask detector (ADR 0073's socket), a pixel extension
producing derived assets (§1), an adaptive-preset extension proposing
settings, hosting.

May never be sold, and this is the list that matters:

* the right to run what is already installed;
* the ability to render a revision the library already holds;
* any feature whose absence degrades the free application below what it
  does today.

The free application stays **complete and check-free**. It contains no
licence code, calls no verification, and does not know whether an
extension is paid — that is ADR 0069 §2's architecture (an extension is
an SDK client, the engine has no extension surface) carried to its
commercial conclusion.

### 5. The licence: offline, and it reuses what already exists

Verification lives **in the extension**, never in Leyline. Its shape is
the one the repository already trusts for updates (ADR 0077 §2): a
minisign key pair, the private key signing at issue time, the public key
compiled into the extension that checks it.

A licence is a small signed document naming the extension, its version
range and its holder. The extension verifies the signature against its
own embedded public key and starts, or does not. **No network call, ever**
— Local First forbids it, and a key that phones home would contradict the
project's most legible promise (ADR 0069 §5's first named point,
answered).

What this deliberately does not attempt: making the licence
unforgeable-in-practice. A signature check in a binary the user runs is
defeatable by the user, and pretending otherwise would drive the design
toward exactly the things this project refuses — obfuscation, phoning
home, hardware binding. The licence marks the honest customer, and that
is enough.

> **Settled in [ADR 0107](0107-licensing-a-paid-extension.md)** (2026-09):
> the shop (a merchant of record), the issuing (a local tool, the private key
> never on a server) and the document's exact format.

### 6. `specification.md` §4 is corrected, not worked around

ADR 0069 §5's third named point. The subscription row today says the
application "is not sold by subscription, does not expire and holds no
licence check". That stays true of **the application** and becomes
explicit that a paid *extension* may exist alongside it, without any of
those three properties changing for what is installed.

## Consequences

* The AI axis is unblocked in the order ADR 0073 already implied:
  detectors first (they need no new decision at all), pixel extensions
  second (they now have their shape).
* No engine work is required by this ADR. A pixel extension imports its
  output like any other file — a path that has existed since ADR 0010.
* When the first paid extension ships, `LICENSE-EXCEPTION.md` (ADR 0069
  §4) already authorizes the combined work; nothing further is needed on
  the licensing side of the GPL.

## Rejected

* **An external stage provider** — §2: it makes a purchase the condition
  of rendering an existing revision.
* **A first-party `leyline-ai` with paid weights** — §2: the same
  failure, harder to see.
* **Shipping an AI denoiser free, in-tree** — not refused on principle,
  and genuinely tempting; refused *for now* on a fact: it would put a
  neural runtime and a weight file into the three packaging chains of
  ADR 0019, which ADR 0083 already declined to do for a fourth C library
  buying sixteen milliseconds. The day someone wants to pay that cost,
  §1's shape applies unchanged — a derived asset, not a stage.
* **Online activation** — §5, and `vision.md`.
