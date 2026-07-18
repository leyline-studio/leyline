# ADR 0009 — GPL-3.0 + CLA, modèle double licence

**Statut :** Accepté — 2026-07

## Contexte

Objectif à terme : une version community open source pérenne **et** une offre commerciale. Une licence permissive est irréversible (le code publié en MIT le reste) ; sans propriété du copyright, aucun relicenciement n'est possible après la première contribution externe.

## Décision

* Code sous **GPL-3.0** (LibRaw pris sous sa branche LGPL, la CDDL étant incompatible GPL).
* **CLA** obligatoire pour toute contribution : le projet conserve le droit de distribuer sous d'autres licences.
* Modèle double licence type Qt : community intégralement GPL, licences commerciales vendues par le projet.
* La marque « Leyline » est hors licence du code (`TRADEMARK.md`).

## Conséquences

* Le projet — et lui seul — peut vendre des exceptions propriétaires ; un fork reste GPL.
* La GPL crée la demande commerciale : intégrer le moteur SDK dans un produit fermé exige une licence payante.
* Assouplir plus tard (GPL → LGPL/MIT) reste possible ; durcir ne l'aurait pas été.
* Friction CLA assumée et annoncée dès le premier jour (pas de changement de règles en cours de route).
* Dépôt de la marque (INPI/EUIPO, classes 9/42) à faire avant la version commerciale.

## Alternatives écartées

* **MIT/Apache-2.0** : adoption maximale mais aucune exclusivité — un concurrent peut commercialiser le moteur.
* **AGPL-3.0** : pertinente pour du SaaS, superflue pour un logiciel desktop local-first.
* **BSL / licences source-available** : non open source, contraire à la vision du projet.
