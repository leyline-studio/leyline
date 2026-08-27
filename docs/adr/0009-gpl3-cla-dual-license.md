# ADR 0009 — GPL-3.0 + CLA, a dual-licence model

**Status:** Accepted — 2026-07

## Context

The eventual goal is a durable open-source community edition **and** a commercial offering. A permissive licence is irreversible (code published under MIT stays MIT); and without owning the copyright, no relicensing is possible after the first external contribution.

## Decision

* Code under **GPL-3.0** (LibRaw taken under its LGPL branch, the CDDL being GPL-incompatible).
* A **CLA** required for every contribution: the project keeps the right to distribute under other licences.
* A Qt-style dual-licence model: the community edition entirely GPL, commercial licences sold by the project.
* The "Leyline" mark falls outside the code licence (`TRADEMARK.md`).

## Consequences

* The project — and it alone — can sell proprietary exceptions; a fork stays GPL.
* The GPL is what creates the commercial demand: embedding the SDK engine in a closed product requires a paid licence.
* Loosening later (GPL → LGPL/MIT) stays possible; tightening would not have been.
* CLA friction accepted, and announced from day one (no changing the rules midway).
* Trademark registration (INPI/EUIPO, classes 9/42) to be done before the commercial edition.

## Alternatives rejected

* **MIT/Apache-2.0**: maximum adoption but no exclusivity — a competitor could sell the engine.
* **AGPL-3.0**: apt for SaaS, superfluous for local-first desktop software.
* **BSL / source-available licences**: not open source, contrary to the project's vision.
