# ADR 0167 — Leyline has no stacks

**Status:** Accepted — 2026-11

## Context

Stacks have sat on the open list since the parity survey against
Lightroom Classic. They have never been shipped or refused. The closest any
ADR came is [ADR 0079](0079-raw-jpeg-pairing.md), which rejected a general
`stacks` table *as the way to serve RAW+JPEG* and said: "the day stacks
arrive, they will find `companion_of` in place and will decide what to do
with it." That leaves the question open. An open question gets asked again,
and the second answer is rarely as good as the first
([ADR 0101](0101-refused-modules.md) says the same about modules).

A Lightroom stack is a group of photos collapsed under a chosen *top* photo
into a single grid cell. In practice it serves four uses, and each one
already has an answer in this repository:

| Use | Where Leyline already answers it |
|---|---|
| A RAW and the JPEG the camera wrote beside it | [ADR 0079](0079-raw-jpeg-pairing.md): one photo, `companion_of`. The pair is formed with no user action, because the master is obvious |
| Virtual copies, stacked with their original | [ADR 0007](0007-git-develop-model.md) and [ADR 0008](0008-version-as-library-unit.md): a virtual copy is a **version**, not an asset row. Without a collection filter the grid shows each asset's *current* version (`grid.rs`, `develop_current`), so a photo with five versions is one cell carrying a count, not five cells |
| The frames of an HDR or panorama bracket | The merges they feed are deliberate exclusions (`specification.md` §4). Grouping the inputs of a feature that does not exist would be the cart before the horse |
| A burst | [ADR 0084](0084-assisted-culling.md) §4: near-duplicates are grouped from capture time and a perceptual hash, the sharpest frame is proposed, and the photographer applies the pick/reject. After the cull, a filter on the pick (`GridQuery::pick`) shows the kept frames and nothing else |

So the only thing a stack would add is the burst case seen from the other
side: hiding frames under a top frame **without** judging them.

## Decision

### 1. Stacks: refused

Leyline has no stack: no `stacks` table, no "collapsed" state on a grid cell,
and no top photo picked by hand. A burst is culled, not stacked.

The reason is the one ADR 0084 §2 already gives for culling. **A frame hidden
in a collapsed stack does not look hidden. It looks absent.** A pick/reject
is a judgement: it is recorded on the version, a filter shows it, and it can
be undone. A collapsed stack hides frames with no judgement behind it, and the
grid's count no longer matches what the photographer sees. ADR 0079 §5 refused
"grouping at display time only" because it makes the catalog lie to the
interface. A stack is that same lie, with a button to undo it.

### 2. What a stack would cost, stated so it is not underestimated

The grid has **one** clause that hides a photo, `companion_of IS NULL`, in the
`FROM`/`WHERE` trunk that the grid and its count share (`grid.rs`, `core`). Everything downstream follows it without knowing pairs exist:
the count, the shot filters, full-text search and smart collections. That works
because a companion is never a photo of its own. A stack member is a photo of
its own, sometimes shown and sometimes not, so every reader has to know about
stacks:

* **selection**: does rating a collapsed stack rate the top frame or all of
  them? Lightroom answers "it depends on whether it is expanded", and that
  answer would then apply to every bulk action;
* **export, batch presets, copy/paste of settings**: the same question for each
  one;
* **smart collections and filters**: does a member that matches a filter show
  up when its top does not?
* **three clients**: Studio, the CLI and the SDK (and the agent surface of
  [ADR 0154](0154-an-agent-surface-over-stdio.md)) would each need an answer,
  where `companion_of` needed none.

Automatic stacking by capture time does not avoid this. It adds a time
threshold that bursts at 1/500 s and slow sequences break in opposite ways,
which is the measurement that rejected time-only pairing in ADR 0079.

### 3. What would reopen it

This is a refusal of the *feature as Lightroom shapes it*, not a promise that
photos are never grouped. Two things would justify a new ADR:

* **HDR or panorama arriving.** Each would need to group its inputs, and would
  decide that grouping for itself. It would find `companion_of`, ADR 0084's
  burst grouping and this ADR in place.
* **A measured culling workflow that pick/reject plus a filter cannot serve**:
  for example, a photographer who needs a burst to stay together in the grid
  *after* the cull. It has to be a real case on a real library, not a parity
  line.

## Consequences

* `specification.md` §4 lists stacks among the deliberate exclusions, with this
  ADR as the reason.
* ADR 0079's sentence "the day stacks arrive" is answered here. ADR 0079 itself
  is accepted and frozen and stays as written.
* No code changes: the catalog, the grid and the clients stay as they are.

## Alternatives rejected

**Ship stacks.** Rejected by §1 and §2: they serve one of four uses that is
already served, they hide frames with no judgement, and they add a
shown-or-hidden state to every reader of the grid in three clients.

**Stacks as a display option only (collapse by capture time, nothing stored).**
Rejected for the reason ADR 0079 §5 gave about pairs: the catalog would keep
N photos while Studio shows fewer, and the gap would show up in every count and
every filter. It would also be built three times, once per client.

**Leave it open, "later".** Rejected: an open question gets asked again, and
for this one "later" is a way of not deciding. If HDR or panorama arrive,
§3 already says what happens.
