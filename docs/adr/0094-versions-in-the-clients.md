# ADR 0094 — Versions in the clients: exposing an engine already paid for

**Status:** Accepted — 2026-08

## Context

ADR 0007/0008 gave the catalog a richer model than the virtual copies and
snapshots it answers to elsewhere: revisions are an immutable graph,
versions are named branches over it, and `create_version`,
`set_current_version`, `versions` have sat in the catalog API — tested,
migrated, documented in `docs/catalog.md` §revisions — since the
beginning. None of it is reachable from a client. A photographer using
Studio cannot make a second development of a photo, cannot see that one
exists, cannot switch; the CLI has no verb for any of it. The history
panel (a list of the *current* version's revisions, clickable) shipped
earlier; the branches did not.

This ADR adds **no engine capability**. It decides how the existing one
appears in three clients — which is mostly deciding what *not* to build.

## Decision

### 1. The grid keeps showing one cell per asset

The grid lists each asset's current version (ADR 0082's pagination is
built on it); a Lightroom-style cell-per-virtual-copy would duplicate
every photo in every folder view for the benefit of the rare asset with
branches. Instead a cell whose asset has more than one version wears a
**count badge** (`×2`), next to the `RAW+J` and edited badges it already
wears. The count is one correlated `COUNT` on the `asset_id` index, a
thirteenth column of the page query — measured habits from ADR 0081
apply: by position, declared in `GRID_COLUMNS`, guarded by the
column-order test. Collections still list member versions explicitly —
that has always been the one place two branches sit side by side.

### 2. Creating and switching live where the photo is looked at

* **Grid context menu**: `New Version` — a static item (a dynamic list of
  versions in a Slint `MenuItem` title is the generator panic ADR 0089
  paid for). It branches from the cell's version at its head, names the
  branch `Version N`, and makes it current so the next glance shows it.
* **Develop panel**: a `Versions` row above the history — one chip per
  branch of the open asset, the current one lit; clicking a chip is
  `set_current_version` plus reopening the edit session on it; a `+`
  chip creates a branch from what is being looked at. The chips are the
  selector the context menu deliberately is not.
* Switching **rewrites nothing**: the graph is untouched, the pointer
  moves — that has been the model since ADR 0008, the clients just
  finally say it.

### 3. The CLI gets the three verbs

`leyline versions <library> <version-id>` lists the branches of that
version's asset, the current one starred. `leyline version-create
<library> <from-version-id> [name]` branches (auto-name when absent) and
makes it current. `leyline version-switch <library> <version-id>` moves
the pointer. All three are the catalog calls with argument parsing —
nothing new below the surface, which is the point.

## Rejected

* **A cell per version in the ordinary grid** (§1) — duplication as
  default presentation, paid by every asset for the few with branches.
* **Renaming from the context menu now** — `rename_version` exists, but a
  text-entry dialog for a name shown in one chip row is polish; the CLI
  path (`version-create` with an explicit name) covers deliberate naming
  until it earns its dialog.
* **A version selector in the context menu** — the MenuItem-title panic
  above; and a menu that lists branches poorly duplicates chips that list
  them well.
