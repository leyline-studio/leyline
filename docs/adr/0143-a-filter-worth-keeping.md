# ADR 0143 — A filter worth keeping

**Status:** Accepted — 2026-09

## Context

Smart collections have existed for a long time in everything but the
interface. `docs/catalog.md` §26 specifies them, `SmartRules` parses and
validates them, `Catalog::create_smart_collection` writes them, the grid query
evaluates them, and Studio's sidebar marks them with a `◈` and refuses them as
drop targets.

No client can create one. Studio's *New collection* dialog makes a manual
collection and nothing else; the CLI has no collection verb at all. A finished
feature, tested, and reachable only by writing Rust against the SDK.

The obvious answer — build a rule editor: a row per criterion, a combo, an
add button — is the wrong one twice over. It is a second place to express what
the filter bar already expresses, and it invites rules the engine cannot
evaluate.

## Decision

### 1. A dynamic collection is *this filter, kept*

The *New collection* dialog gains two chips: **Manual** and **Dynamic — the
current filter**. The dynamic one saves what the grid is showing: the stars,
the camera, the keywords, the pick flag. Under it, in words, exactly what will
be remembered — « Elle contiendra toujours les photos notées 3 ou plus. »

So the rule editor is the filter bar, which the photographer has already
learned and can see behind the dialog while choosing. Nothing new to
understand, and the collection is described in the same words as the filter it
came from.

Every dialog opens on **Manual**: a dynamic collection is a deliberate answer,
never what a distracted `Enter` produces.

### 2. A criterion that cannot be kept is refused by name

`GridQuery` carries more criteria than `SmartRules` can express: a colour
label, a lens, four shot ranges, a text search, a capture range, a folder, a
parent collection. And two of the three pick states — « not flagged as a pick »
covers *rejected* and *unflagged* alike, so only `Pick` has an exact
equivalent.

Saving such a filter as rules would produce a collection that answers with
photographs the filter excluded — and that would look right while doing it. So
the dialog **names the offending criterion and refuses**: « Le filtre
« étiquette de couleur » ne peut pas être conservé : une collection dynamique
connaît la note, le boîtier, les mots-clés et le drapeau de sélection, et rien
d'autre. » The Create button is disabled before the click, not after it.

This is the rule ADR 0042's pipeline already applies to a setting a pinned
stage version cannot express: **refuse, never silently narrow**. An empty
filter is refused for a smaller reason — a dynamic collection holding the whole
library is a second name for *All photos*.

### 3. The rules stay readable afterwards

`Catalog::smart_rules` is new — the rules were written and evaluated, never
read back — and the status line shows them beside the count whenever a dynamic
collection is selected: « 50 photos · notées 3 ou plus ».

Without it the rules would be visible exactly once, in the dialog that created
them, and a collection whose contents come from a rule nobody can read is a box
that fills itself for reasons the photographer has to remember.

Keywords are stored by **path** and not by id, which `docs/catalog.md` §26
already required: a rule that says `Nature/Oiseaux` still means something after
the row it points at is renamed, and a rule holding a number does not.

## Consequences

* The gap the UX review named — *a finished feature with no surface* — is
  closed for Studio. **Not for the CLI**, which still has no collection verbs
  at all; that is a wider gap than this ADR, and it is now the only client that
  cannot do this.
* `SmartRules` is written from the query in one place (`smart_rules_from_query`),
  which is also the list of criteria a future rule format would extend. A
  criterion added to `GridQuery` and forgotten here is refused rather than
  dropped — the refusal is the default branch.
* **Editing** an existing collection's rules is out of scope: filter, and make
  another. Revisit when someone reports rebuilding the same collection twice.
* One more use for the filter bar, and one more reason for it to stay the one
  place a query is expressed.

## Alternatives rejected

* **A rule editor in the dialog** — a second grammar for the same question,
  and the only way to offer criteria the engine cannot evaluate.
* **Saving the filter and dropping what does not fit.** The collection would
  be *wider* than what was on screen when it was saved, and nothing would say
  so.
* **Widening `SmartRules` to cover the whole query.** A bigger format, a
  migration, and rules older engines refuse (§26 already refuses unknown
  criteria on purpose) — for criteria nobody has asked to keep. The refusal
  message is the measurement: if one criterion keeps coming back, that is the
  moment to extend the format.
* **Creating the collection from the selection** rather than the filter. That
  is a manual collection, and it already exists.
