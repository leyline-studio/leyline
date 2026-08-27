# ADR 0061 — Choosing the demosaicing algorithm

**Status:** Accepted — 2026-08
**Followed by:** `input::v3`, which this ADR creates, is no longer the current
version: [ADR 0066](0066-sensor-white-level.md) takes the white level from the
sensor rather than from the photo's content (`v4`). The demosaicing choice
decided here passes through that change unmoved.

## Context

Demosaicing is the very first rendering decision: reconstructing three channels
per pixel from a sensor that measures only one. Its choice shows on fine detail
and repetitive patterns — foliage, fabric, masonry — where one algorithm
produces moiré and another does not.

Leyline does not choose it. `params.user_qual` is neither exposed nor even
written: LibRaw's default is taken, silently.
[ADR 0050](0050-highlight-reconstruction.md) had left the question open in so
many words. `docs/measured-findings.md` §A2 takes it up as the project's
cheapest quality lever: it is **already in the dependency**, and only needs
driving.

Two facts were verified before deciding, and both narrow the scope relative to
what the plan assumed.

**AMaZE and LMMSE are not available.** They live in the GPL2/GPL3 *demosaic
packs*, removed from LibRaw's main distribution since 0.19 and absent from the
library linked here (0.20.2: `user_qual` and `dcb_iterations` are present, and
no pack symbol is). The plan cited RawTherapee, which bundles them separately.
Offering them would amount to offering a choice that silently falls back to AHD
— worse than not offering it.

**Demosaicing has no effect on small previews.** The `Thumbnail` and `Small`
classes decode at `half_size` (`preview.rs`), and LibRaw's half-size mode takes
one pixel per 2×2 Bayer group: **the interpolation is simply short-circuited**.
The setting therefore changes nothing until one is at `Medium` or beyond, or at
export. That is not a flaw to fix — it is what makes navigation fast — but it
is a fact the interface must state, on pain of offering a slider that "does
nothing".

## Decision

**Demosaicing becomes a named setting, written into the revision, carried by a
new version of the `input` stage.**

### 1. Four algorithms, not seven

`Settings` gains a `demosaic` field, whose values are named by what they do,
never by LibRaw's number:

| Value | LibRaw | Why it is there |
|---|---|---|
| `ahd` *(default)* | 3 | LibRaw's and Leyline's historical default. Good everywhere, excellent nowhere. |
| `vng` | 1 | Gentle on gradients, less maze artefacting on flat areas. |
| `dcb` | 4 | Better rendering of sharp edges; the choice when moiré is a nuisance. |
| `dht` | 11 | The finest on high-frequency detail, the slowest. |

Deliberately rejected: **AMaZE and LMMSE**, unavailable (see Context);
**linear (0) and PPG (2)**, strictly worse than AHD without being fast enough
for it to matter, the preview path already holding its speed through its proxy;
**AAHD (12)**, too close to AHD to justify a fifth entry in a list the user
must be able to scan at a glance.

### 2. `input::v3`, and the refusal that goes with it

Changing demosaicing changes the pixels. It is therefore a **new version of the
`input` stage**, never a modification of `v2`: existing revisions cite `v1` or
`v2` and go on rendering exactly as today (`docs/pipeline.md` §5.1).

`v3`'s default stays **AHD**, so that moving a revision to `v3` without
touching the setting moves no pixel. Choosing a "better" default would have made
new photos diverge from old ones without anyone asking.

A non-AHD setting on a revision pinned at `input: 1` or `2` is **refused by
`validate()`**, naming the version that would be needed — the capability rule
already applied by ADR 0050 to `highlight_reconstruction`. Never a silence,
never a discreet fallback.

### 3. What the interface must say

The setting lives in the Detail group, beside noise reduction, and **announces
itself that it is not visible at this preview size** while the displayed
preview is `Thumbnail` or `Small`. A setting whose effect is invisible without
explanation is a setting people believe is broken.

All three clients expose it, like the rest of the pipeline: Studio, the CLI
(`leyline develop <version> demosaic <ahd|vng|dcb|dht>`) and the SDK.

## Consequences

* One more stage version (`input::v3`), hence one more entry in
  `stages/golden.rs`'s reference renders, with the previous ones unchanged.
* `DecodeParams` gains a field, and the C shim a parameter — the same shape as
  what ADR 0050 did for `highlight`.
* The field enters `settings_json` and the Detail preset group.
* **The benefit shows only at `Medium` and beyond, and at export.** No quality
  measurement will therefore be conclusive on a small preview.
* `dcb_iterations` and `dcb_enhance_fl` stay at their defaults: they are
  settings of a single algorithm, and exposing them would bring a tree of
  options into a place where the project wants a flat list.

## Alternatives rejected

* **Exposing nothing and changing the default** to an algorithm judged better:
  it moves everyone's pixels without saying so, and still deprives the user of
  the choice. The worst of both worlds.
* **Exposing LibRaw's seven values**, including those that fall back to AHD for
  want of the GPL pack: a menu that lies.
* **Compiling LibRaw with the GPL2/GPL3 demosaic packs** in order to offer
  AMaZE: it would impose an in-house build of LibRaw on all three platforms,
  where ADR 0004 insists on a substitutable system `.so`, and would reopen a
  settled licence question. To be taken up in its own ADR if the demand comes.
* **An application-wide setting rather than a per-photo one**: it would
  contradict reproducibility — a revision must carry everything that decides
  its pixels (`docs/pipeline.md` §5.1), and a setting outside the revision is
  precisely what that guarantee forbids.
