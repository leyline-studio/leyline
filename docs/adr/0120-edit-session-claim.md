# ADR 0120 — A session claims a version, it does not hold the catalog

**Status:** Accepted — 2026-09 · **Built** — 2026-09-20

## Context

`Library::edit` returns `EditSession<CatalogWrite<'_>>`, and `CatalogWrite`
is a `MutexGuard` over the catalog. The session therefore holds the
catalog's only lock for as long as it lives.

The cost is not theoretical. `CatalogWrite`'s own documentation states the
rule — *"one operation, then drop"* — and `Library::edit` is the one place
in the façade that breaks it. While a session is open, **every** other call
on that `Library` blocks: a grid query, a preview, a keyword, a job. Not
with an error, not with a timeout: it stops, silently, until the session is
dropped. That has already been hit in this repository, from a test that held
a session across a `preview()` call, and the symptom was indistinguishable
from a hang in whatever code happened to be new that day.

The lock is also far too coarse for what it protects. What must be exclusive
is the session's claim that its in-memory `Settings` are authoritative — and
that claim is about **one version**. Nothing about the other 38 000
photographs in the library needs protecting from a session editing this one.

Two things now press on it. [ADR 0121](0121-remote-engine-boundary.md) puts a
client on the other side of a network, where a session's end cannot be
observed at all. And the same coarseness is what makes the local application
single-threaded in practice at exactly the moment a photographer is working.

## Decision

### 1. A session claims a version; the catalog lock goes back to its rule

The session no longer carries a `MutexGuard`. It carries a **claim on a
`VersionId`**, registered with the `Library`, and takes the catalog lock the
way every other façade method does: for one operation, then drops it.

The seam already exists and was built for this: `EditSession<C: DerefMut<Target = Catalog>>`
is generic over *how the write handle is held* — "a plain `&mut Catalog`, or
the lock guard a shared `Library` hands out". The claim is a third such
handle, and the session's own logic — coalescing, the amendment window,
undo, redo — does not change by a line.

### 2. A second session on the same version is refused, never blocked

By name, with its own error, in the family of `RootOffline` and
`NewerSettings`: *this version is being edited*. Two sessions on one version
is a real mistake — the second's commit would overwrite the first's
authoritative state — and a mistake that is *blocked* is a mistake the user
watches as a freeze. An interface can say "this photograph is open on
another device". It cannot say anything at all about a mutex.

Sessions on *different* versions no longer interfere in any way, which is
the whole point.

### 3. `Drop` stays the mechanism; a deadline exists only where `Drop` cannot

In process, the holder disappearing is perfectly observable: `Drop` runs, the
pending state commits, the claim is released. That is the existing promise —
*"closing the session commits the pending state: nothing is ever lost"* — and
nothing here weakens it.

Over a network there is no such signal, and pretending otherwise is how a
remote session becomes a permanent lock. So a claim taken by a holder whose
disappearance cannot be observed carries a **deadline**, renewed by every
call that holder makes.

**The asymmetry is the decision, not an omission.** A deadline exists
precisely because a network has no `Drop`; imposing one on a local session
would mean a photographer who pauses to think loses their claim, which is a
new failure invented to make two cases look alike.

### 4. An expired claim commits what the engine holds

Not discards. Every `set` reached the engine — that is what `set` *is*, an
in-memory update on the engine's side — so the pending state is the engine's
own, and committing it neither loses the user's work nor invents anything
they did not do. It is the drop the holder could not perform, performed for
them.

The revision that results is an ordinary revision, in the ordinary history,
undoable by the ordinary undo. Nothing marks it as having come from a lost
connection, because nothing about it is different.

### 5. Thirty seconds, renewed by any call

The number needs a reason, so here it is, from both ends:

* **Far above the amendment window** (`DEFAULT_AMEND_WINDOW`, 2 s), so an
  expiry can never cut a coalescing chain in half and turn one intention into
  two revisions.
* **Below a coffee break**, so a client that crashed does not hold a
  photograph hostage while its owner walks to another room and picks up the
  tablet.
* **Never reached by a client that is alive**, because every call renews it,
  and a client with nothing to say sends a heartbeat, which is the cheapest
  message a transport has.

## What was built

The decision above was taken in 2026-09 and built on 2026-09-20, unchanged in
substance. Two notes from the building, both of them consequences of §1 rather
than departures from it:

* The seam `EditSession` is generic over is no longer `DerefMut<Target = Catalog>`
  but a `CatalogAccess` trait with one method, `with(|catalog| …)`. A reference
  cannot be handed out without being *held*, which is the whole defect; a
  closure can, so the handle takes the lock inside the call and gives it back.
  `&mut Catalog` implements it in one line, so the engine's own batches and
  every test are untouched. `EditSession::history` becomes `&mut self`, reading
  the history being an operation like any other.
* §2's refusal had to be extended to the **batches**. Before the claim, a
  preset application or a reprocess simply blocked behind an open session,
  because everything shared one lock; afterwards they would have written behind
  its back, which is precisely the mistake §2 refuses. They now leave a claimed
  version alone and report it, in the per-version failure list those batches
  already carry.

## Consequences

* **The local hang disappears**, and with it a whole class of diagnosis where
  a new feature is blamed for a lock an old one was holding. `CatalogWrite`'s
  documented rule becomes true of the entire façade, with no exception left
  to remember.
* **`Library::edit`'s return type changes**, and with it the one place in each
  client that opens a session. `EditSession`'s own API does not move.
* **Concurrency arrives for free where it was already wanted**: a preview, a
  grid query or an export job no longer waits behind an open develop panel.
* **No schema change, no stage version, no golden entry, no migration.** This
  ADR moves a lock; it does not touch a pixel or a stored byte.
* [`engine-api.md`](../engine-api.md) §10.1 gains the claim and the refusal;
  `catalog.md` is untouched, since nothing about the database changes.

## Rejected

* **A re-entrant lock.** It would let the deadlocked call through and leave
  the real error — a second session on the same version — silent again. The
  problem was never re-entrancy; it was granularity plus silence.
* **A mutex per version.** It gets the granularity right and keeps the
  silence: the second session would still *wait*, and an interface that waits
  is an interface that has nothing to display. §2's refusal is the point.
* **Keeping the guard and documenting the hazard.** It was documented. It was
  hit anyway. A rule that a single method in the façade is allowed to break is
  not a rule.
* **A deadline for local sessions too**, for symmetry. Symmetry between a case
  where disappearance is observable and one where it is not is a false
  economy: it buys uniform code by inventing a way for a live client to lose
  its work.
* **Refusing to commit an expired session's pending state**, on the grounds
  that the user never asked. They asked with every `set`; what they did not
  do is close the session, and §4 does that for them — which is exactly what
  `Drop` already does locally.
