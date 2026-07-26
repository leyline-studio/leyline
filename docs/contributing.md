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

## Ajouter ou corriger un étage de rendu

C'est la contribution la plus contrainte du projet, parce que c'est celle qui touche à la promesse « mêmes pixels dans dix ans » ([`pipeline.md`](pipeline.md) §5.1). Le *quoi* est spécifié en §3.3 du même document ; voici le *comment*.

Tout vit dans `crates/leyline-engine/src/stages/` : un module par version d'opérateur (`sharpen/v1.rs`), le registre `STAGES` qui les compose, et `golden.rs` qui les gèle.

### Règle unique

> Une fois publiée, une version d'étage ne bouge plus. Ni son corps, ni son rang, ni la liaison `apply` qui la désigne dans `STAGES`.

Corriger un rendu, c'est donc **ajouter** `v2`, jamais éditer `v1`. Une optimisation qui produit exactement les mêmes octets n'est pas un changement de rendu et reste dans `v1`.

### Corriger un opérateur existant

1. Créer `stages/<opérateur>/v2.rs` et le déclarer dans `stages.rs`. Repartir d'une copie de `v1.rs` est la norme, pas un aveu d'échec : la duplication est le prix du gel ([ADR 0042](adr/0042-versioned-stage-pipeline.md)).
2. Ajouter une entrée `Version` **à la fin** du tableau `versions` de cet étage — la dernière est celle que le moteur épingle pour les nouvelles révisions.
3. Choisir son rang : le même que `v1` si la position ne change pas, un rang libre entre deux dizaines sinon. Déplacer un opérateur, c'est ce choix-là, pas une édition du rang existant.
4. Attention au corps partagé : plusieurs étages passent par `kernel::v1`. Le corriger déplacerait le rendu de tous. Un correctif y crée `kernel::v2`, que seules les nouvelles versions d'étages appellent.
5. Tester les deux versions dans `stages/tests.rs` : ce que `v2` fait de mieux, et que `v1` fait toujours ce qu'elle faisait.

### Ajouter un opérateur

1. Le réglage d'abord, dans `leyline-core` : champ de `Settings`, valeur neutre dans `Default`, bornes dans `validate()`, documentation. Un paramètre optionnel à valeur neutre n'incrémente pas `schema` (`pipeline.md` §3.4) ; un changement de type, d'unité ou de plage, si.
2. Le prédicat `active` du `Stage` ne lit **que** les réglages — jamais l'image, le boîtier ou le profil résolu. Il décide à la fois ce qui s'exécute et ce qu'une révision inscrit, et une révision s'écrit sans image en main. L'indisponibilité au rendu (pas de profil Lensfun, pas de DCP) se traite dans `apply`, en laissant les pixels tels quels.
3. Un étage à sa valeur neutre ne s'exécute pas et n'apparaît pas dans la carte `stages`. C'est ce qui rend un rendu neutre bit-pour-bit identique à l'image décodée.
4. Un rayon exprimé en pixels se multiplie par `ctx.scale` ([ADR 0041](adr/0041-interactive-preview-rendering.md)), sans quoi la préversion et l'export ne montreront pas le même effet. Ce qui est normalisé dans `[0, 1]` l'ignore.
5. Déterminisme : pas d'horloge, pas d'ordre d'itération de `HashMap`, aucune réduction flottante inter-threads — la parallélisation se fait par lignes disjointes ([ADR 0012](adr/0012-rayon-data-parallelism.md)).
6. Une décision structurelle s'écrit en ADR **avant** le code, et la ligne de la table de `pipeline.md` §3.3 fait partie du même changement.

### Les rendus de référence

`stages/golden.rs` épingle, dans `tests/golden/renders.json`, l'empreinte BLAKE3 d'un rendu par famille d'opérateurs **et la carte `stages` qui l'a produite**. Chaque entrée est rejouée à travers sa propre carte : une `v2` ne peut donc pas déplacer une empreinte existante, elle en ajoute une.

Trois gardes :

* chaque entrée épinglée rend toujours exactement ses pixels ;
* ce que le moteur épinglerait aujourd'hui figure au manifeste ;
* aucune paire `(étage, version)` publiée n'y échappe — un opérateur qu'aucun cas n'active fait échouer les tests.

```bash
cargo test -p leyline-engine --lib stages::golden          # vérifier
LEYLINE_BLESS_GOLDEN=1 cargo test -p leyline-engine --lib stages::golden   # ajouter les entrées manquantes
```

Le bénissage est **additif** : il n'écrase jamais une entrée existante. Si une empreinte déjà au manifeste change, c'est un défaut — du code gelé a été touché — et il se corrige dans le code, pas dans le manifeste. Les cas eux-mêmes sont gelés pour la même raison : exercer un opérateur autrement, c'est un nouveau cas.

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

