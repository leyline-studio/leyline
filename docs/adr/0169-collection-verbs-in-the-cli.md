# ADR 0169 — Collection verbs in the CLI

**Status:** Accepted — 2026-11

## Context

Collections (`docs/catalog.md` §24–§26) have been in the engine since V1
and in Studio since the beginning, dynamic ones since
[ADR 0143](0143-a-filter-worth-keeping.md). The CLI had **no collection
verb at all**: it could not list them, create one, fill one, or list what one
holds. ADR 0143 named that gap and left it open; the UX review of 2026-09-10
kept it as item 4.

`docs/engine-api.md` §1 says the CLI proves "API before GUI": everything
Studio does, the CLI does. For collections it did not, and a script that
wanted to put the picks of a shoot in an album had to open Studio.

Every verb needed already exists on `Library` (`create_collection`,
`create_smart_collection`, `add_to_collection`, `remove_from_collection`,
`collections`, `smart_rules`, `rename_collection`, `move_collection`,
`delete_collection`) and on `GridQuery::collection`. This ADR decides the
surface, not new engine behaviour.

## Decision

### 1. The verbs, named like the existing ones

Following `versions`/`version-create` and `presets`/`preset-save`:

| Command | What it does |
|---|---|
| `collections <library>` | the tree, indented, one line each: id, `manual` or `smart`, member count, name; a smart one also prints its rules |
| `collection-create <library> <name> [--parent <id>] [rules…]` | a manual collection, or a smart one when a rule flag is given (§2) |
| `collection-add <library> <collection-id> <version-id>...` | adds versions to a manual collection |
| `collection-remove <library> <collection-id> <version-id>...` | removes them |
| `collection-rename <library> <collection-id> <name>` | |
| `collection-move <library> <collection-id> (--parent <id> \| --root)` | one of the two is required: a bare move is not "to the root" by accident |
| `collection-delete <library> <collection-id>` | prints how many collections went; no version, revision or file is touched |
| `ls <library> --collection <id> …` | the members, in the collection's order, with every other `ls` filter still applying |

Ids are the catalogue's numbers, printed by `collections`, as `ls` already
prints version ids.

### 2. Smart rules are flags, the same four ADR 0143 keeps

`--rating <1-5>`, `--camera <name>`, `--keyword <path>` (repeatable) and
`--pick`. Any one of them makes the collection smart; none makes it manual.
These are exactly the criteria `SmartRules` can evaluate, so the CLI has no
"criterion that cannot be kept" to refuse: a flag it does not take is an
unknown flag. `--pick` has no negative form, for the reason ADR 0143 §2 gives:
"not flagged as a pick" covers rejected and unflagged alike, and nobody asked
for that set.

A keyword is given by **path** (`Nature/Oiseaux`), as the rules store it
(`docs/catalog.md` §26), and a path the library does not hold is **refused by
name**: a rule naming a keyword that does not exist would make a collection
that is silently empty, which is the "silently narrow" ADR 0042 refuses.

No JSON payload: four flags cover the whole format, and a newer engine's
criteria arrive as new flags with their own help line.

### 3. The engine refuses, the CLI repeats it

Adding to a smart collection, moving a collection under its own descendant, or
an out-of-range rating are refused by the catalogue already; the CLI prints
that message and exits 1, as every other verb does.

## Rejected

- **A single `collection <sub-command>` verb**: the CLI has no sub-command
  level anywhere; `version-create` set the pattern.
- **Rules as a JSON argument** (`--rules '{"rating":{"gte":3}}'`): exposes the
  storage format as the interface, for four fields.
- **Names instead of ids**: two collections may share a name under different
  parents; ids are what `ls` and `versions` already use.

## Consequences

- The CLI covers collections, which closes UX review item 4 and the gap ADR
  0143 left for this client. MCP is unchanged: collection tools there are a
  separate decision ([ADR 0154](0154-an-agent-surface-over-stdio.md)).
- The member count in `collections` is one `Catalog::count` per collection,
  the query Studio's status line runs for the selected one; a library holds
  tens of collections, not thousands.
