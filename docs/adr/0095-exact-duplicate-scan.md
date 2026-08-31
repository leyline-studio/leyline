# ADR 0095 — Exact duplicates: asking for the fingerprint on purpose

**Status:** Accepted — 2026-08

## Context

The parity survey listed "duplicate detection" as missing. Measured
against the code, it is not: the import computes a BLAKE3 fingerprint of
every candidate and refuses one the catalog already holds, whatever the
file is called and wherever it sits. Verified on 2026-08-31 — the same
bytes under a different name, in a different folder, are refused with
*duplicate of asset 1*. There is exactly one path that registers an
asset and it always asks that question first, so **a library cannot hold
two assets with identical content**, and a sweep looking for them would
be a sweep that provably finds nothing.

What the same measurement *did* find is a real defect, one step
sideways. `scan` (ADR 0065) previews what an import would take, and marks
a candidate `already_imported` from the catalog's **names and sizes** —
an index comparison, no byte read. ADR 0065 §3 chose that deliberately
and its reasoning stands: fingerprinting a full card before the user has
chosen anything means reading 20 GB. But the consequence was never
stated: **a renamed duplicate is announced as importable and then
refused.** On the corpus this project actually targets — 38,389
photographs accumulated over fourteen years, referenced in place —
renamed copies are not the exotic case, they are the ordinary one. A
preview tool whose whole job is to say what the import will do gets it
wrong, silently, on the most common mess a real archive contains.

## Decision

### 1. The default does not move

`ScanOptions::exact` is `false` by default and the name-and-size hint
keeps answering, unchanged, for the gesture ADR 0065 §3 was written
about: a card in a reader, a choice to be made in two seconds. Nothing
about that path becomes slower, and §3's reasoning is preserved rather
than overturned.

### 2. Exactness is asked for, and its price is stated

With `exact: true` the scan fingerprints each candidate and answers from
the same index the import consults. It costs a full read of every file
scanned — that is the price ADR 0065 refused to pay *by default*, and
paying it on purpose is a different act from paying it by surprise. The
mode exists because a second question exists, and it is not the card
question: *of the files in this archive, which do I already hold?* That
one is asked once, deliberately, about a local folder, by someone
willing to wait.

### 3. A candidate that duplicates says which asset it duplicates

`ImportCandidate::duplicate_of: Option<AssetId>` — filled only in exact
mode, because only the fingerprint can name it. `already_imported`
keeps its meaning ("the library already holds this file") and is
answered by whichever method was asked for; the docstring says which.
Naming the asset is what turns a refusal into an answer: *this file is
the one you already have as asset 4 231*, which is what a person sorting
a disk actually needs.

Nothing is deleted, nothing on disk is touched, no report is stored. The
scan stays what it is — a way of looking — and the file-hygiene decision
stays the photographer's, which is the same boundary `docs/vision.md`
draws everywhere else.

### 4. Three clients

CLI: `leyline scan <library> <source> [--exact]`, `=` marking a match and
the asset id printed beside it. Studio: an `Exact` chip next to `Look
first`, off by default, which says in the dialog that it reads every
file. SDK: the field and the option are re-exported.

## Consequences

* The parity survey's §2.3 is corrected rather than implemented: exact
  duplicate detection was delivered at the only place it can act, and
  what was missing was the *preview* agreeing with it.
* `scan --exact` is also the honest answer to "do I have this photo
  twice on disk", without Leyline touching a single file.

## Rejected

* **A library-wide duplicate sweep** — it would report nothing, ever,
  since the import cannot admit a second copy. Building it would be
  building a feature whose emptiness is a property of the schema.
* **Making the fingerprint the scan's default** — ADR 0065 §3's cost is
  real and unchanged; a card preview that reads 20 GB is not a preview.
* **Visual (near-)duplicate detection** — a different question needing a
  model, and it belongs behind the boundary of ADR 0069/0073, not in the
  import path.
