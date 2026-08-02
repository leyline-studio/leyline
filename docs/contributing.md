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

## Commandes

Un `Makefile` à la racine rassemble ce qui revient souvent — `make` seul liste les cibles. Rien n'y est obligatoire : chaque cible n'est qu'une enveloppe autour d'un `cargo` ou d'un script de `packaging/`, et tout reste lançable à la main. L'intérêt est de garder les options exactes en un seul endroit, plusieurs étant faciles à se rappeler de travers.

La seule à connaître par cœur :

```bash
make check      # fmt + clippy + tests — à passer avant chaque commit
```

Une CI GitHub Actions ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)) rejoue ces trois étapes — `fmt`, `clippy -D warnings`, `test --workspace` — sur chaque push vers `main`, chaque tag `v*` et chaque pull request, sur Linux, Windows et macOS. La branche Windows est en `continue-on-error` : le livrable Windows est compilé de façon croisée depuis Linux ([ADR 0019](adr/0019-distribution-i18n.md)), il n'existe pas encore de build natif MSVC. Elle arrive après coup : `make check` reste ce qui sépare une erreur de `main`.

Les autres, au besoin : `make run` / `make cli` (avec `ARGS=…`), `make golden` et `make golden-bless` (rendus de référence, section suivante), `make test-raw LEYLINE_TEST_RAW=…` (les tests ignorés qui exigent un vrai RAW), `make bench`, `make i18n` (voir plus bas), et `make windows` / `make appimage` / `make dmg` pour les paquets ([ADR 0019](adr/0019-distribution-i18n.md)).

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
make golden          # vérifier
make golden-bless    # ajouter les entrées manquantes
```

Le premier de ces gardes ne tourne que sur la plateforme de référence, celle où les empreintes ont été bénies : comparer des octets entre plateformes reviendrait à promettre ce que [`pipeline.md`](pipeline.md) §5.2 refuse d'affirmer. Les deux autres tournent partout.

Le bénissage est **additif** : il n'écrase jamais une entrée existante. Si une empreinte déjà au manifeste change, c'est un défaut — du code gelé a été touché — et il se corrige dans le code, pas dans le manifeste. Les cas eux-mêmes sont gelés pour la même raison : exercer un opérateur autrement, c'est un nouveau cas.

## Toucher à l'interface de Studio

La découpe des fichiers est décrite dans [`architecture.md`](architecture.md#à-lintérieur-de-studio). Trois règles s'y ajoutent, dont deux sont faciles à enfreindre sans s'en apercevoir.

### L'état d'interface : global ou local ?

> **Un `global` ne porte que l'état qui traverse la frontière Rust ↔ UI. L'état qui ne concerne qu'un panneau reste une propriété privée de ce panneau.**

C'est la règle qui empêche `ui/state/` de redevenir la surface plate de 111 propriétés qu'ADR 0045 a démontée. Un accordéon replié, l'outil de glisser actif, le menu ouvert : Rust ne les lit jamais, donc ils n'ont rien à faire dans un global. Si un panneau doit malgré tout exposer quelque chose à la fenêtre, il le fait par sa propre surface — une propriété `in-out`, une `public function`, un `callback` — et non en élargissant un global.

Le test est mécanique : si aucun `.get_x()`/`.set_x()`/`.on_x()` côté Rust ne correspond à la propriété, elle ne doit pas être dans `ui/state/`.

### Ne jamais poser de géométrie autour du contenu

Slint 1.13 a, dans ce projet, un défaut de planification de repaint : donner à `keys` (le `FocusScope` de `studio.slint`) ou à un ancêtre de ses repeaters — la grille, la liste des collections — **un override de position ou de taille, même inerte**, suffit à faire rester des zones blanches jusqu'à ce qu'un changement structurel force un rafraîchissement. C'est pourquoi les panneaux héritent du type d'élément qu'ils remplacent et ne posent aucune géométrie, et pourquoi une colonne qui doit dégager la hauteur de la barre de menus le fait avec un `Rectangle` d'espacement en enfant supplémentaire. Le commentaire au-dessus de `menu-row` dans `studio.slint` détaille le diagnostic.

### La fenêtre ne doit jamais hériter d'un maximum de son contenu

Slint déduit les contraintes d'une fenêtre de ce qu'elle contient, et le
backend winit les transmet au gestionnaire de fenêtres. Une colonne faite de
lignes à hauteur fixe annonce donc un **maximum** borné, qui remonte jusqu'à
`StudioWindow` et devient un `program specified maximum size` : la fenêtre ne
peut plus être maximisée, et chaque recomposition qui change cette borne —
ouvrir un dialogue, en fermer un, créer une collection — la ré-applique, ce
qui ramène brutalement une fenêtre maximisée à la taille du contenu.

`StudioWindow` déclare pour cette raison un `max-width`/`max-height`
volontairement énorme : c'est la seule façon d'exprimer « pas de maximum »
en Slint 1.13, et ça neutralise la classe entière de régressions.

Pour vérifier, sous X11 :

```bash
xprop -id "$(xdotool search --name 'Leyline Studio' | head -1)" WM_NORMAL_HINTS
```

Un `program specified maximum size` autre que celui déclaré signale qu'un
panneau a recommencé à contraindre la fenêtre.

### Le catalogue de traduction se périme en silence

`slint-tr-extractor` inscrit le fichier et la ligne de chaque `@tr(...)`, et le `msgctxt` est **le nom du composant**. Déplacer une chaîne d'un composant à un autre change donc sa clé et la détache de sa traduction, sans le moindre avertissement. Après toute tranche d'interface :

```bash
make i18n
```

puis reporter les traductions existantes dans `translations/fr/LC_MESSAGES/leyline-studio.po`. Vérifier en lançant Studio avec `LANG=fr_FR.UTF-8` : un catalogue qui compile n'est pas un catalogue qui traduit.

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

