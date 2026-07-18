# ADR 0007 — Modèle de développement inspiré de Git

**Statut :** Accepté — 2026-07

## Contexte

Le développement non destructif exige un historique fiable : undo/redo, snapshots, copies virtuelles. Un unique JSON écrasé à chaque réglage (modèle initial) rendait l'historique impossible sans refonte.

## Décision

Les réglages forment un graphe de **révisions immuables** (`parent_revision_id`), les **versions** sont des branches nommées (pointeur de tête), la version courante est un simple pointeur. Une copie virtuelle est une branche, pas une ligne d'asset.

Détails : `catalog.md` §16–18, coalescence §17, process versions dans `pipeline.md` §3.3.

## Conséquences

* Undo/redo = déplacement de pointeur ; historique complet gratuit ; previews revalidées par identifiant de révision.
* Copies virtuelles sans duplication de fichier ni de métadonnées.
* Volume maîtrisé par la coalescence (une révision = une intention, jamais un événement d'interface).
* Complexité assumée : deux tables de plus qu'un simple champ JSON.

## Alternatives écartées

* **JSON unique écrasé (Lightroom)** : pas d'historique, contradiction avec les objectifs du projet.
* **Historique en pile linéaire (Darktable)** : pas de branches, copies virtuelles dupliquées.
* **Deltas plutôt qu'états complets** : rejouer une chaîne de deltas pour chaque rendu, fragilité en cas de corruption d'un maillon.
