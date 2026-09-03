# ADR 0117 — What surrounds the photograph

**Status:** Accepted — 2026-09

## Context

Leyline shows every photograph on the same near-black ground, in the loupe,
in develop and in compare. That ground is not decoration: it is what the eye
adapts to, and therefore part of every judgement made about the image on top
of it. A photograph looks lighter and flatter against black than against
white, and the difference is large enough that print shops and standards
bodies specify the surround rather than leave it to taste.

Two situations make the fixed dark ground actively wrong:

* **Judging tone.** A dark surround makes shadows look open and highlights
  look strong; the same file against a neutral mid-grey reads differently,
  and mid-grey is what image-evaluation practice asks for.
* **Judging a soft proof.** A proof simulates ink on paper, and paper is
  white. Against black it flatters — the darkest ink looks deeper than any
  print can be.

A comparison with a camera maker's own application found the same gap from
the other side: it offers background *and* margin options, separately for
its normal and its proofing view.

## Decision

### 1. Three grounds, not a colour picker

The surround is a **preference with three values** — `dark` (today's ground,
and the default), `grey`, `white` — and not an arbitrary colour.

A colour picker would invite a choice that has no right answer and several
wrong ones: a coloured surround shifts the adaptation of the eye that is
about to judge white balance. The three values are the three that mean
something — dark for looking, neutral mid-grey for judging tone, white for
judging a print — and each is a *neutral*, so none of them lies to the eye
about colour.

### 2. The proof gets its own, and it defaults to white

A second preference, applied whenever a soft proof is in effect
([ADR 0034](0034-softproofing-watermark-print.md)), defaulting to **white**.

That default is the decision: a proof exists to answer "what will this look
like printed", and a print is looked at on paper. Leaving the proof on the
same dark ground as everything else would make every proof look better than
its print, which is the one thing a proof must not do.

### 3. Where it lives, and what it is not

In `preferences.json` beside the language and the update check
([ADR 0078](0078-preferences.md) §5) — it belongs to the installation, not to
the catalogue and never to a revision. It changes **no pixel**: it is a
property of the room, not of the photograph, and nothing about it reaches
`settings_json`, an export or a print.

**No margin setting**, and the refusal is not laziness. A margin exists to
separate the photograph from the panel around it, and that is exactly what a
surround the eye can see already does — a white or grey ground makes the
frame's edge obvious where a dark one hid it. A fixed margin, meanwhile,
takes pixels away from the photograph in the one view whose whole purpose is
to show it as large as the window allows.

## Consequences

* The three viewers — loupe, develop, compare — paint their ground from the
  preference, and the proofing one when a proof is in effect.
* Two more values in `preferences.json`; absent means the defaults, which are
  today's dark and a white proof.
* No engine surface, no stage, no schema. A photograph exported before and
  after changing this preference is byte-identical.

## Alternatives rejected

* **An arbitrary colour.** §1: no right answer, several wrong ones, and a
  coloured surround corrupts the very judgement the viewer exists for.
* **One preference for both views.** It would make the proof's ground a
  compromise between two different questions. The proof's job is specific
  enough to deserve its own answer, and its default is the whole point.
* **Storing it per photograph.** It is a property of the room and the light
  in it, not of the picture; a revision that carried it would make the same
  photograph "look different" on another machine for a reason that has
  nothing to do with the file.
