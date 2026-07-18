# ADR 0005 — Lensfun et LittleCMS

**Statut :** Accepté — 2026-07

## Contexte

La correction optique exige une base de profils d'objectifs ; la gestion colorimétrique exige des transformations ICC fiables.

## Décision

* `leyline-lens` s'appuie sur **Lensfun** (LGPL-3.0 ; base de données CC-BY-SA, attribution requise).
* `leyline-color` s'appuie sur **LittleCMS** (MIT).

Chaque dépendance est confinée à son crate, derrière une API Leyline.

## Conséquences

* Profils d'objectifs maintenus par la communauté, corrections (distorsion, vignettage, aberrations) immédiatement disponibles.
* Gestion ICC éprouvée (LittleCMS est l'implémentation de référence de l'industrie).
* Mêmes obligations LGPL que LibRaw pour une future version propriétaire (linkage dynamique).

## Alternatives écartées

* **Profils maison** : impossible de rattraper la couverture de Lensfun.
* **qcms / moxcms** : moins complets que LittleCMS pour les usages photo (profils v4, intents).
