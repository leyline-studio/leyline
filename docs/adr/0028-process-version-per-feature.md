# ADR 0028 — One process version per pixel feature: keeping per-module duplication

**Status:** Superseded by [ADR 0042](0042-versioned-stage-pipeline.md) — 2026-07

> This document described the convention in force until July 2026: one process
> version per feature, each in a `processN.rs` that is a complete copy of the
> previous one. Its last consequence anticipated its own reopening "with real
> data"; ADR 0042 does that, on the basis of 14,968 cumulative lines and 70–93 %
> duplication between consecutive modules. Kept as it was, for the record of the
> reasoning.

## Context

`docs/v2-scope.md` §9 notes a fourth cross-cutting thread, non-blocking but
structural: the **proliferation of process versions**. The house duplicates a
complete `processN.rs` module per version — `process2.rs` copies `process 1`,
`process3.rs` copies `process 2`, and so on — changing only the one new or
different operator. That is a deliberate choice, argued in the "Alternatives
rejected" sections of ADR 0013 and ADR 0016: sharing code is exactly what
would risk a future fix to one operator silently altering the frozen
rendering of an earlier version, which is meant to be fixed forever — "the
same pixels in ten years" (`docs/pipeline.md` §3.3).

Five versions exist today, all tied to lens correction or to the transfer
functions (ADR 0013, 0016–0018). Most items in V2's scope are
*pixel-affecting* and would each demand a new process version under this
convention (`docs/v2-scope.md` §2, §3, §4, §5, §6, §8: all marked "Process
+1"). Stacking six or more items would therefore mean as many new duplicated
`processN.rs` modules added to the five existing ones.

§9 poses the open question: one process per feature (the status quo, more
duplicated modules) or a grouped "consolidated V2 process" bump (fewer
modules, but features that can no longer ship independently)? Rather than let
every future V2 feature ADR settle that strategy again, this document freezes
it once.

The real cost of duplication to date (`wc -l crates/leyline-engine/src/process*.rs`):

| Module | Lines |
|---|---|
| `process1.rs` | 451 |
| `process2.rs` | 560 |
| `process3.rs` | 726 |
| `process4.rs` | 773 |
| `process5.rs` | 923 |
| **Total** | **3433** |

## Decision

**The convention stays unchanged: one process version per pixel feature, each
in its own frozen `processN.rs` module, created by copying the previous
module whole and changing only the new or different operator** — exactly what
ADR 0013 and ADR 0016 established. V2 introduces **neither** a grouped bump
(a multi-feature "V2 process") **nor** a shared operator library that
versions would compose instead of duplicating.

This document decides no V2 feature: it freezes the *versioning strategy*
that every future feature ADR (tone curve, HSL, local adjustments, spot
removal, dehaze, DCP…) will apply without relitigating it. Every pixel
feature ready to ship takes the next available process number and its own
module.

## Consequences

* Every pixel-affecting V2 feature can ship **independently**, as soon as it
  is ready, without being blocked by another feature of the same theoretical
  batch — versioning never becomes a synchronization point between unrelated
  pieces of work.
* A revision's `process` field keeps its **semantic legibility**: `process: 3`
  today means exactly "lens distortion correction active", a single readable
  fact; it will go on designating an identifiable feature rather than an
  opaque bundle of unrelated ones.
* The number of `processN.rs` modules grows by one per pixel feature — a
  bounded and known cost (3433 cumulative lines for five versions, above).
  The codebase already tolerates five versions without friction; the
  trajectory stays linear and predictable.
* The freezing guarantee is preserved in full: no earlier version's module is
  touched when a new one ships, since no render code is shared between
  versions (the "same pixels in ten years" contract stays mechanically
  unfalsifiable, `docs/pipeline.md` §3.3).
* Should the number of modules one day become a real burden — a number well
  above today's five, after several V2 features have actually shipped — a
  future ADR can reopen the question **with real data**. This document does
  not preempt that decision; it only refuses to anticipate it speculatively
  today.

## Alternatives rejected

* **A grouped "consolidated V2 process" bump** (several pixel features
  gathered under a single new process version, hence one extra module):
  it does not save the future duplication it claims to avoid. Rule §3.3
  requires that *one* fix to *one* of the grouped features, once the version
  is published, forces an entirely new process version anyway — hence one
  more complete module. Grouping therefore merely **delays** the growth in
  modules (by blocking the independent release of features already ready)
  without ever avoiding it. It degrades the legibility of the `process` field
  into the bargain, which would no longer announce a single semantic fact but
  an opaque parcel of heterogeneous features. Rejected: all the costs, none
  of the supposed benefits.
* **A shared operator library** (extracting the common operators so that
  versions *compose* them instead of *duplicating* them, reducing the total
  line count): it contradicts head-on the very reason for complete
  duplication, argued in ADR 0013 and ADR 0016. Shared code is precisely what
  exposes a frozen rendering to the risk that a future, unrelated change
  silently alters it — per-module duplication is not an oversight to
  optimize, it is the very mechanism that makes the freeze unfalsifiable.
  Rejected: it would reintroduce exactly the danger the convention exists to
  remove.
* **Deferring the decision and letting each V2 feature ADR choose** its own
  strategy: that was the implicit status quo, but `docs/v2-scope.md` shows
  six or more items converging on the same versioning choice. Settling it
  once now prevents every future ADR from re-deriving — or worse, deciding
  differently — the same answer, at the risk of a versioning convention that
  is inconsistent from one item to the next.
