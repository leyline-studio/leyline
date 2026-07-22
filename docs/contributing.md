# Contributing

## Philosophie

* Lisibilité avant optimisation.
* Pas de code mort.
* Documentation obligatoire des API publiques.
* Tests pour chaque fonctionnalité.
* Architecture en couches.
* Pas de dépendances circulaires.

## Style

* rustfmt
* clippy sans warnings
* CI verte obligatoire.

## Commits

Conventional Commits.

## Licence et contributions

Leyline est publié sous **GPL-3.0**.

Des licences commerciales seront proposées à terme : Leyline suit un modèle de double licence (type Qt), la version community restant intégralement GPL.

Pour rendre ce modèle possible, toute contribution est soumise au **CLA** (Contributor License Agreement) décrit dans [`CLA.md`](../CLA.md) : le contributeur accorde au projet le droit de distribuer sa contribution sous d'autres licences, tout en gardant son copyright.

En soumettant une pull request, vous acceptez les termes du CLA.

Le nom « Leyline », le logo et la marque restent la propriété du projet et ne sont pas couverts par la licence du code — voir [`TRADEMARK.md`](../TRADEMARK.md).

## Code de conduite et sécurité

Les contributeurs et participants aux issues/PR sont tenus au
[`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md).

Les vulnérabilités de sécurité se signalent en privé, pas par une issue
publique — voir [`SECURITY.md`](../SECURITY.md).

## Dépendances

* LibRaw est utilisé sous sa branche **LGPL-2.1** (la branche CDDL est incompatible avec la GPL).
* Lensfun (LGPL-3.0) et sa base de données (CC-BY-SA) exigent l'attribution.
* Le décodage RAW est isolé derrière l'API de `leyline-raw` afin de rester substituable.

## Objectif

Construire un moteur photographique pérenne, pas seulement une
application.

