# ADR 0075 — Le cache d'aperçus garde une fenêtre, pas tout l'historique

**Statut :** Accepté — 2026-08

## Contexte

Un aperçu est indexé par `(asset, révision, kind)` et **un commit ordinaire
laisse en place ceux de la révision précédente**. C'est délibéré :
[`catalog.md`](../catalog.md) §20 en fait une propriété — « un undo qui ramène
la tête sur une révision déjà prévisualisée revalide automatiquement les
anciens aperçus : aucune régénération n'est nécessaire ».

Ce qui n'était pas décidé, c'est **quand ils s'en vont**. Réponse : jamais.
Seul un *amendement* de la tête supprime les siens
(`Catalog::remove_revision_previews`). Il n'existe ni plafond de taille, ni
éviction, ni commande de purge, ni dans Studio ni dans la CLI.

### Ce que ça coûte, mesuré

Sur une photo Canon 5D IV du corpus réel :

| | Poids |
|---|---|
| Le CR2 | ~35 Mo |
| Aperçu 1024 px (celui de la vue develop) | **0,68 Mo** |
| Aperçu 2048 px | 2,47 Mo |
| Aperçu 4096 px | 9,03 Mo |

Trois copies virtuelles d'une photo coûtent donc ~2 Mo d'aperçus contre 35 Mo
de RAW : **+6 %**, et non ×3 — un point qui vaut d'être écrit, parce que la
crainte spontanée est que « les photos soient doublées », alors qu'une version
de développement est une **ligne** qui référence l'asset et qu'un retraitement
écrit une révision JSON.

Le vrai risque est ailleurs et il est réel : **cent retouches sur une même
photo laissent ~70 Mo d'aperçus périmés, davantage que le RAW lui-même**. Sur
une bibliothèque travaillée pendant des années, c'est là que le disque part.

### Ce qu'on sait déjà, et qui décide de la forme

Un aperçu froid coûte de l'ordre de la **seconde** (décodage compris), et le
cache est **reconstructible par définition** — le supprimer ne perd rien. Une
fenêtre glissante suffit donc : au-delà, on régénère plutôt qu'on ne garde.

## Décision

### 1. Une fenêtre par photo, et les têtes toujours

Sont **conservés**, pour un asset donné :

1. l'aperçu de la **tête de chaque version** (copie virtuelle) — une copie
   parquée sur une révision ancienne doit garder son aperçu, sans quoi la
   grille se remettrait à rendre à chaque défilement ;
2. les aperçus des **trois révisions les plus récentes** de cet asset.

Tout le reste est évincé : ligne supprimée, fichier supprimé.

La seconde règle est ce qui garde undo *et* redo instantanés autour du point
de travail : après un undo, la tête est une révision récente, et celle qu'on
vient de quitter — la cible du redo — l'est aussi. Les deux sont dans la
fenêtre sans qu'on ait à raisonner sur le sens du déplacement.

**Trois**, parce que c'est ce qu'il faut pour couvrir le va-et-vient d'un
réglage sans mémoriser une session entière. Le nombre est une constante
nommée, pas un réglage : un utilisateur n'a pas à arbitrer une taille de cache,
et la fenêtre coûte au plus ~2 Mo par photo *effectivement retouchée*.

### 2. Au-delà de la fenêtre, on régénère — et on ne précharge pas

Remonter plus loin dans l'historique rend l'aperçu manquant. Le chemin
existant s'en charge déjà sans une ligne de plus : `latest_preview` sert
l'image périmée la plus récente, marquée `Preview::Stale`, pendant que la
bonne se calcule (`engine-api.md` §11).

**Rien n'est préchargé en arrière-plan**, et c'est un choix : le cas ne se
présente qu'au quatrième undo consécutif — les trois premiers tombent dans la
fenêtre — et il coûte alors une seconde, une fois. Construire une anticipation
pour cela reviendrait à rendre des images que personne ne regardera, ce qui est
exactement le gaspillage que cette ADR corrige.

### 3. L'éviction a lieu là où le cache grossit

Après chaque enregistrement d'un aperçu (`Library::preview`), et là seulement.

Pas de balayage périodique, pas de tâche de fond, pas de « vider le cache » à
la charge de l'utilisateur : le seul moment où le cache peut dépasser sa
fenêtre est celui où on vient d'y ajouter quelque chose. Une purge attachée à
ce point est bornée par construction et n'a besoin d'aucun ordonnanceur.

Corollaire assumé : une bibliothèque qu'on n'ouvre plus ne se nettoie pas
toute seule. C'est cohérent — un cache qui ne grossit plus n'a rien à rendre.

### 4. Ce que l'éviction ne touche jamais

* **Les photos.** Rien de ce document ne concerne les fichiers importés : le
  cache vit dans `Cache/`, et une bibliothèque dont on efface entièrement ce
  dossier rend exactement les mêmes pixels, une seconde plus tard.
* **Les révisions.** Aucune n'est supprimée : l'historique reste entier et
  reste rejouable. On évince des *images dérivées*, jamais une intention.
* **La promesse §5.1.** Un aperçu n'est pas un rendu d'export ; il n'y a ni
  version d'étage ni pixel garanti dans cette affaire.

## Conséquences

* **Le cache devient `O(photos retouchées × 3)` au lieu de
  `O(retouches)`.** C'est le changement de forme qui compte : le premier est
  prévisible et proportionné à la bibliothèque, le second croît avec le temps
  passé à travailler.
* **Un défaut silencieux disparaît.** Rien, dans l'interface, n'aurait signalé
  un cache de 40 Go : ni erreur, ni ralentissement — seulement un disque plein
  un jour, avec une cause introuvable.
* **`catalog.md` §20 gagne son revers.** Le document disait quand un aperçu
  redevient valide ; il dit maintenant aussi quand il cesse d'exister.
* **Aucune migration.** La règle porte sur des lignes qu'on supprime, pas sur
  le schéma : une bibliothèque existante se met dans sa fenêtre au premier
  aperçu qu'elle enregistre.

## Alternatives écartées

* **Un plafond global en gigaoctets, avec éviction LRU.** La première idée, et
  moins bonne : elle demande un réglage à l'utilisateur, une comptabilité de
  taille, et elle évince par ancienneté d'accès — donc potentiellement
  l'aperçu de la photo qu'on regarde, sur une bibliothèque au plafond. La
  fenêtre par photo ne peut pas se tromper de cible.
* **Ne garder que la tête.** Une révision, un aperçu : simple, et il rend
  chaque undo coûteux d'une seconde alors que le va-et-vient sur un réglage
  est le geste le plus ordinaire du développement.
* **Précharger la révision suivante en arrière-plan.** Rend des images que
  personne ne regardera dans le cas courant ; voir §2.
* **Une commande « vider le cache ».** Ne résout rien — elle déplace le
  problème sur l'utilisateur, qui doit d'abord découvrir qu'il en a un. Rien
  n'interdit de l'ajouter plus tard comme confort ; ce n'est pas la réponse.
* **Purger à l'ouverture de la bibliothèque.** Fait payer un balayage complet
  au démarrage pour un dépassement qui, lui, arrive un aperçu à la fois.
