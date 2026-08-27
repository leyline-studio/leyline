# ADR 0069 — A closed extension attaches, it does not duplicate the repository

**Status:** Accepted — 2026-08

## Context

Leyline wants a paid feature (assisted masks, C2 of
`measured-findings.md`) without ceasing to be free software. The question asked
was: should the **repository be cloned** and two projects maintained carrying
"essentially the same code", one open and one paid?

No, and the refusal is not primarily economic.

### Why a clone is the worst of choices here

An "almost identical" fork costs a lone maintainer a cherry-pick per fix,
forever. But Leyline has a harder reason than fatigue.

`pipeline.md` §5.1 promises that a stage version renders **identically,
everywhere and forever**. That promise is carried by frozen code: `sharpen::v1`
is literally the same code in every binary. **Two repositories cannot both own
`sharpen::v1`.** At the first drift — a fix applied on one side, a
`renders.json` blessed twice — the promise is broken, and it is broken
*silently*: nobody notices before a photo from 2026 renders differently in
2031.

The clone therefore duplicates not merely code, it duplicates **the very thing
the project promises never to duplicate**.

### What the project has already decided

`CLA.md` exists and says what follows: Leyline "intends, over time, to offer
additional commercial licenses alongside the GPL community edition". The
necessary rights are therefore already gathered — contributors included. What
was missing was not permission, it was **the seam**: where a closed extension
attaches without touching the open repository.

## Decision

**The public repository stays whole and unique. A closed extension is a
separate crate, in its own private repository, which attaches through a
boundary the engine never crosses.**

### 1. The rule: an extension produces *settings*, never pixels

That is the ADR's core, and everything else follows from it.

An extension may **read** a decoded image and **write** into a revision. It
does not take part in rendering. It is not a stage, it has no stage version,
and it does not appear in the `stages` map.

Architecturally, an assisted mask is therefore **the same thing as the brush
tool**: something that produces mask data, which the open pipeline then
renders. The brush is driven by a mouse, that one by a model; from the engine's
point of view, the difference does not exist.

Three consequences, and they are what make the decision safe:

* **§5.1 is out of reach.** No closed component enters the render path, so no
  stage version depends on code the public cannot read. The project's dearest
  promise never meets the licence boundary.
* **The free version renders everything.** A photo retouched with an assisted
  mask opens, renders and exports **identically** on a build without the
  extension: the mask is data in `settings_json`, like a brush stroke. What the
  free version lacks is the tool that *proposes* the mask, never the one that
  applies it.
* **The catalog format does not split.** That is the previous point's
  corollary, and the red line: the day a file written by the paid edition were
  no longer readable by the free edition, non-destructiveness
  (`pipeline.md` §6) would be broken *against our own users*.

### 2. The attachment is the SDK, not the engine

An extension is a **client** of `leyline-sdk`, on the same footing as Studio or
the CLI (`architecture.md`: `Studio → SDK → Engine → Core`). It asks for a
preview, computes, and writes through an ordinary edit session.

The engine therefore gains **no extension surface**: no plugin registry, no
callback trait invoked during a render, and no dynamic loading. What does not
exist cannot become a channel through which closed code creeps into the
pipeline — that is §1's guarantee, made structural rather than promised.

### 3. What the engine must nevertheless learn

For a computed mask to be *expressible* as data, `Mask` must be able to carry a
computed coverage, and not only a parametric geometry (`Radial`, `Gradient`,
`Brush`, `Everything` — ADR 0029, ADR 0048).

That is an addition to the **open** engine, with its own ADR and its own
`local_adjustments` stage version: a stored coverage is a rendering the frozen
versions do not know how to produce, and the capability rule applies as it
stands — a stage version that cannot express a setting **refuses** it, it does
not silently ignore it.

That stage is open, free, and renders everyone's masks. It is what makes §1
hold.

### 4. The GPLv3 §7 additional permission

A proprietary crate linked to `leyline-sdk` forms a combined work the GPLv3
governs. The project holds the necessary rights (§Context), so it can authorize
it — but **that must be written down**, without which the public repository
says one thing and the shipped binary does another.

The form retained is an *additional permission* in GPLv3 §7's sense, recorded
in a file of the project's own. The GPL's text itself is **never** modified: it
stays verbatim in `LICENSE`.

**Recorded on 2026-08-27** in `LICENSE-EXCEPTION.md`, at the root, and
referenced from `README.md`'s License section. The file reproduces the wording
below word for word; what surrounds it is explicitly marked there as
explanatory and not operative.

The wording retained:

> **Additional permission under GNU GPL version 3 section 7**
>
> The copyright holders of Leyline give you permission to combine Leyline with
> software released under terms of your choice, and to convey the resulting
> work, provided that every part of Leyline itself remains governed by the GNU
> General Public License version 3 and is conveyed under those terms.
>
> This permission does not extend to modified versions of Leyline: if you
> modify Leyline, this additional permission does not apply to your modified
> version, and you may remove it.

### 5. What is **not** decided here

**The paid licensing system.** Key verification, activation, a free edition
against a paid one: none of that is settled by this ADR, and none of it needs
to be in order to start.

That is deliberate. The boundary above costs almost nothing and constitutes a
better architecture independently of any commercial question — it lets work on
assisted masks begin without the model having been decided. Licence checking
will come, if it comes, **behind** that boundary and without touching the
engine.

Two points will remain to be settled that day, and it is better to name them
now:

* verification will have to be **offline** — `vision.md` (Local First) forbids
  a server call, and a key that phones home would contradict the project's most
  legible promise;
* `specification.md` §4 today files subscription among the deliberate
  exclusions. A paid edition will have to correct that text rather than work
  around it.

## Consequences

* **One public repository, whole.** Nothing is taken out of it, and no feature
  is amputated from it in order to be resold elsewhere.
* **The closed crate is small**: it proposes masks, it renders none. Everything
  expensive and delicate — decoding, the pipeline, colour, export — stays open
  and shared.
* **No fork, hence no cherry-picking.** The extension follows the SDK's
  published versions like any client.
* **The dependencies will have to be re-checked** before the first closed
  release: the GUI brick (Slint) offers several licences including the GPLv3
  option, which stops being suitable as soon as a shipped binary is no longer
  GPLv3; LibRaw and Lensfun are LGPL, which requires letting the user relink —
  a build decision, which the Windows cross-build already touches.
* §4's permission will have to be **added to the repository** before the first
  combined release, not after.

## Alternatives rejected

* **Cloning the repository** (the original question). Rejected in §Context: two
  owners for one stage version, hence a silent breach of §5.1 — plus the
  perpetual cherry-picking that would already suffice on its own.
* **A plugin called during rendering.** The intuitive shape, and exactly what
  §1 forbids: a revision's rendering would then depend on a component whose
  version is not in the `stages` map and whose freezing nobody can audit. That
  is §5.1 abandoned for architectural convenience.
* **Removing the feature from the open repository** (open core by
  subtraction). The engine would lose a stage, and a photo edited with the paid
  edition would cease to be rendered by the free one — the catalog format
  splits, and non-destructiveness is broken for the user unlucky enough to go
  back.
* **Dual-licensing the whole engine**, Qt-style, with no closed extension. That
  is the model `CLA.md` keeps open and it remains possible; it simply does not
  answer the question asked here, because it monetizes redistributors — and
  Leyline's users are photographers, who redistribute nothing.
* **A separate binary communicating through the catalog.** It avoids the
  linking question, at the price of a second process, a second life cycle and a
  bypass of the façade `engine-api.md` §13 exists to prevent. §4's permission
  costs three paragraphs and avoids all of it.
