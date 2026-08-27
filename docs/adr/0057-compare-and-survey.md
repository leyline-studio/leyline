# ADR 0057 — Choosing between two photos: a Compare view with linked zoom, and a Survey view

**Status:** Accepted — 2026-08

## Context

[ADR 0055](0055-library-navigation.md) §3 delivered the grid and the loupe, and
set aside Lightroom's two other layouts, deferring their decision here. The
reason to handle them now is a precise gesture Leyline cannot perform today:

> two photos of the same scene, taken a second apart. Which one is sharp? On
> which eye? At full screen neither of them says; they have to be seen **in the
> same place, at the same magnification, at the same time**.

Today that means entering the loupe, memorizing, coming back, navigating, and
magnifying again. It is exactly the work the computer should be doing. And it
is the most frequent gesture of a culling session, the one that precedes any
development.

**What is already there and is not at issue**: multi-selection (Ctrl/Shift
click) exists and already feeds the batch actions; flags, stars and labels
exist; the loupe exists. What is missing is a way of *looking*, not a way of
classifying.

## Decision

### 1. Two more layouts, of the same kind as the loupe

**Compare** (`C`) shows two photos side by side; **Survey** (`N`) shows the
whole selection at once. Like the loupe, and for the same reason:

* **no edit session is opened** — these are the cached previews, the ones the
  loupe and develop already use, so nothing is rendered twice;
* **nothing is written**: no revision, no history, no preference. The state
  lives for the window's lifetime
  ([ADR 0045](0045-studio-ui-modularisation.md) §2);
* `G` returns to the grid, and the classification keys (1-5, 6-9, P/X/U) go on
  acting on the current photo.

### 2. Zoom and panning are **linked**, in image coordinates

That is the decision that makes this ADR exist. Two independent loupes side by
side serve no purpose: what is compared is the *same place* in both images.

The link is expressed in **normalized image coordinates** — the point looked at
is "62 % of the width, 41 % of the height" — and not in screen pixels nor in
image pixels. That is what keeps two photos of different dimensions (a crop and
its original, a RAW and its JPEG) on the same area. A single magnification
factor applies to both.

Two levels, like develop's loupe: **fit** and **100 %**. No continuous zoom: the
gesture served here is "show me the pixels", and a zoom slider is a slower
version of it.

### 3. Left = the select, right = the candidate

In Compare, the left photo is the one selected; the right one is its neighbour,
and the arrow keys move **only the candidate**. One key swaps them: the
candidate becomes the select, and the culling moves on.

That is Lightroom's model (*Select* / *Candidate*), and it is better than "the
two selected photos": it gives the gesture a direction — one defends a
title-holder against challengers — instead of requiring two selections before
anything can be looked at.

### 4. Survey shows the selection, and serves to reduce it

`N` shows the **multi-selected** photos side by side, or the single selected
photo if there is only one. Clicking a thumbnail's cross **removes it from the
selection** without deleting anything: it is a funnel, one starts with twelve
photos and keeps two.

It is the only place in this decision that writes anything — and it writes only
into the selection, which is not stored.

### 5. What these views do not do

* **They do not develop.** No setting is editable there; for that there is
  develop, one key away.
* **They do not compare a before and after.** That is `\` in develop (the
  Compare Before/After view), and it is another question: the same file in two
  states, not two files.
* **They show no more than two photos in Compare.** Three views with linked
  zoom barely fit on a screen, and Survey covers the "several" case.

## Out of scope

* **Continuous zoom** (a progressive wheel, a slider): §2.
* **A full-screen mode with no menu bar**: ADR 0055 §6 says why the bar stays.
* **Comparing two *versions* of one photo** side by side. The data model would
  allow it (a version is the unit,
  [ADR 0008](0008-version-as-library-unit.md)), but that is an entry through
  the catalog and not through the selection; to be decided on its own.
* **Synchronizing classification between the two views** (rating the left one
  also rating the right one). No: that is precisely what one is trying to
  distinguish.

## Consequences

* **Culling becomes feasible inside Leyline** without leaving the application
  or opening two windows.
* **No new writing** and no new catalog access: both views read the previews
  already cached and the selection already in memory.
* **A shared display component** appears (a magnifiable, pannable image whose
  zoom is driven from outside); ADR 0055's loupe takes it up and therefore
  gains the zoom it did not have, with no duplicated code.
* **Two more shortcuts** (`C`, `N`), identical to Lightroom's. `N` was not
  free: it opened "new collection", which moves to `Ctrl+N` — the same
  judgement call as `E`/`Ctrl+E` in
  [ADR 0055](0055-library-navigation.md) §4, and for the same reason: the
  *view* key is the one pressed without thinking, and the deliberate action
  already has a menu and a button. The shortcuts dialog, the Library menu and
  the View menu all say so.

## Alternatives rejected

* **Two independent loupes side by side.** That is what one gets without §2,
  and it does not answer the question asked.
* **Linking panning in image pixels.** Two photos of different sizes go out of
  alignment immediately; normalized coordinates are what keeps a comparison
  between an original and its crop still legible.
* **Comparing the last two selected photos**, with no notion of select and
  candidate. Two selections are then needed before anything can be seen, and
  nothing says which one is being defended.
* **Making Survey a filtered grid** ("show the selection only" in the filter
  bar) rather than a view. That would be one more filter in a bar that already
  has six, and it would have to be removed by hand afterwards.
