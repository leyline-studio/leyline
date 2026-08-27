# ADR 0005 — Lensfun and LittleCMS

**Status:** Accepted — 2026-07

## Context

Optical correction demands a database of lens profiles; colour management demands reliable ICC transforms.

## Decision

* `leyline-lens` builds on **Lensfun** (LGPL-3.0; the database is CC-BY-SA, attribution required).
* `leyline-color` builds on **LittleCMS** (MIT).

Each dependency is confined to its own crate, behind a Leyline API.

## Consequences

* Community-maintained lens profiles, so corrections (distortion, vignetting, aberrations) are available immediately.
* Proven ICC handling — LittleCMS is the industry's reference implementation.
* The same LGPL obligations as LibRaw for a future proprietary edition (dynamic linking).

## Alternatives rejected

* **Profiles of our own**: catching up with Lensfun's coverage is out of reach.
* **qcms / moxcms**: less complete than LittleCMS for photographic use (v4 profiles, rendering intents).
