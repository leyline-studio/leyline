# ADR 0021 — Menus contextuels (clic droit)

**Statut :** Accepté — 2026-07

## Contexte

ADR 0020 règle la découvrabilité globale (barre de menu), mais une action visée sur *un objet précis* — une photo de la grille, le canevas de développement — reste plus naturelle en clic droit qu'en cherchant l'objet correspondant dans un menu global déjà ouvert. Les applications de bureau habituelles (et l'attente d'un utilisateur venant d'un autre logiciel photo) proposent les deux : barre de menu pour les commandes globales, clic droit pour les commandes qui portent sur ce qui est sous le curseur.

## Décision

Deux menus contextuels, tous deux strictement des raccourcis vers des actions déjà décidées (ADR 0020) ou déjà implémentées — même contrainte : **aucune fonctionnalité nouvelle**, uniquement un second point d'accès.

**Grille de la bibliothèque, clic droit sur une vignette :**

* Develop *(D)*
* ——
* Rate ▸ (0–5), Label ▸ (couleurs), Flag ▸ (Pick/Reject/Clear) — `docs/catalog.md` §8, mêmes actions que `classify.rs`
* ——
* Add to Collection *(B)*, Remove from Collection *(Shift+B)*
* ——
* Reprocess — **pas** le chemin `EditSession::reprocess` du raccourci `R` (qui exige une session de développement ouverte), mais `Library::reprocess` (§10.4) appliqué à la version cliquée (ou à la sélection courante si plusieurs vignettes sont sélectionnées) : le même appel que `Shift+R`, juste borné à un sous-ensemble au lieu de toute la bibliothèque. Aucun changement d'API requis.

Si plusieurs vignettes sont sélectionnées et que le clic droit tombe sur l'une d'elles, le menu contextuel agit sur toute la sélection (convention standard) plutôt que sur la seule vignette cliquée.

**Canevas, clic droit en mode développement :**

* Reset Crop — remet `crop` à `None` (le même effet que le bouton *Reset* déjà dans le panneau Geometry)
* Compare Before/After — bascule le même état que le contrôle déjà présent en haut du canevas
* ——
* Reprocess *(R)*
* ——
* Back to Library *(G)*

## Conséquences

* Aucune nouvelle capacité moteur : chaque entrée appelle un chemin déjà décidé par ADR 0020 ou déjà câblé dans `main.rs`/`leyline-engine`.
* La grille des collections/mots-clés (panneau gauche) n'a **pas** de menu contextuel dans cette V1 : `leyline-engine` n'expose pas aujourd'hui de renommage/suppression de collection ou de mot-clé côté façade — ajouter un clic droit là inventerait une capacité qui n'existe pas encore. À revisiter si/quand cette capacité est décidée séparément.

  > **Correction, 2026-08-05.** La condition posée ici s'est réalisée, et la
  > revisite a eu lieu : la façade expose `rename_collection`,
  > `move_collection` et `delete_collection`, et l'arbre des collections porte
  > désormais le menu contextuel correspondant — trois entrées qui ouvrent
  > chacune un dialogue, parce que chacune a besoin d'une donnée que Rust doit
  > chercher d'abord (le nom courant, les parents légaux, la taille du
  > sous-arbre). C'est exactement le « si/quand » que cette phrase prévoyait,
  > et le motif de clic droit décidé ici qui s'y applique.
  >
  > **Les mots-clés, eux, n'ont toujours pas de menu contextuel**, et pour la
  > raison d'origine restée intacte : la façade sait créer un mot-clé et
  > l'attacher, pas le renommer ni le supprimer.
* Reprocess depuis la grille utilisant l'API batch (plutôt que l'API session unique) évite d'exiger que la photo soit ouverte en développement juste pour la retraiter — cohérent avec l'esprit de `Shift+R` (retraiter sans ouvrir).

## Alternatives écartées

* **Un seul menu contextuel générique partagé partout** : perdrait la distinction entre actions qui portent sur une photo (grille) et actions qui portent sur l'état d'édition courant (canevas) — les deux listes n'ont presque aucun recouvrement.
* **Reprocess via `EditSession::reprocess` (comme `R`) au lieu de `Library::reprocess`** : forcerait à ouvrir une session de développement pour chaque photo cliquée, plus lent et plus intrusif que le retraitement par lot déjà conçu pour cet usage.
