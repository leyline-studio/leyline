# ADR 0026 — Référentiel de coordonnées des masques et corrections locales

**Statut :** Accepté — 2026-07

## Contexte

`docs/v2-scope.md` §2, §5 et §9 identifient deux familles de fonctionnalités
à venir — réglages locaux/masqués (brosse, filtres radial/gradué/linéaire)
et suppression de tache — qui ont toutes deux besoin de stocker de la
géométrie (points, rayons, rectangles) par révision. Les deux partagent le
même problème non résolu, signalé comme premier « verrou » transversal du
§9 : sur quel référentiel de coordonnées cette géométrie est-elle définie ?

L'ordre fixe du pipeline (`docs/pipeline.md` §3.1) place Rotation/Recadrage
en **dernier**, après tous les opérateurs tonals. Pourtant `crop` lui-même
est déjà défini « relatif à l'image **après** rotation »
(`crates/leyline-core/src/settings.rs:99`, `docs/pipeline.md` §3.2) : des
coordonnées normalisées dans le canevas tourné mais pas encore recadré —
pas le cadre final visible, pas le cadre brut décodé non plus. C'est le
seul précédent existant pour de la géométrie normalisée dans `settings_json`.

Plutôt que de laisser chaque future ADR de fonctionnalité (masques,
suppression de tache, color grading régional) retrancher ce choix
indépendamment, ce document le tranche une fois, en amont.

## Décision

**Toute géométrie de masque ou de correction locale se stocke en
coordonnées normalisées `[0,1]` relatives à l'image après rotation, avant
recadrage — exactement le même référentiel que `crop`.** Aucun nouveau
référentiel n'est introduit ; celui déjà accepté pour `crop` est étendu.

Les étages de rendu qui évaluent cette géométrie (bloc tonal/local, avant
l'étage Rotation/Recadrage de §3.1) tournent sur un tampon de pixels encore
dans l'orientation décodée/corrigée-objectif — **avant** que la rotation ne
soit appliquée. Le moteur doit donc faire remonter la géométrie stockée
dans ce référentiel pré-rotation en lui appliquant la transformation
**inverse** de la rotation en attente, avant rastérisation ou évaluation —
la même technique de remapping arrière que celle déjà utilisée par `rotate`
(`process2.rs`) et par la correction d'objectif (`process3.rs`, ADR 0016),
simplement parcourue en sens inverse et appliquée à une géométrie d'entrée
plutôt qu'à des coordonnées d'échantillonnage de sortie. Aucune technique
nouvelle : la même famille de remapping backward, un troisième
consommateur.

Ce choix ne préjuge pas du format de stockage (JSON compact vs table
dédiée) ni de la coalescence des gestes utilisateur — ces points restent à
trancher par l'ADR propre à chaque fonctionnalité (`docs/v2-scope.md` §9).

## Conséquences

* **Stable sous édition du recadrage** : recadrer ne fait que rogner le
  canevas post-rotation, jamais le repositionner — les masques et taches
  restent alignés avec le contenu photographié quel que soit le rectangle
  de recadrage choisi ensuite, exactement comme `crop` lui-même reste
  cohérent sous cette convention.
* **Hérite du comportement existant de `crop` sous édition de la
  rotation** : si l'angle de rotation change après la pose d'un masque ou
  d'une tache, la géométrie stockée se réinterprète contre le nouveau
  canevas tourné — une limitation déjà acceptée pour `crop` (§3.2), pas une
  régression introduite ici.
* Le moteur gagne un utilitaire de transformation de coordonnées
  (rotation directe/inverse) partagé entre `rotate` et le futur étage
  masques/taches — extraction d'infrastructure depuis `process2.rs`, sans
  changer le rendu gelé de `rotate` lui-même.
* Ce référentiel s'applique identiquement au color grading régional
  (`docs/v2-scope.md` §4, extension future de l'item 3 sous masque) sans
  décision supplémentaire.

## Alternatives écartées

* **Coordonnées relatives au cadre brut décodé (avant rotation)** :
  évaluation triviale côté moteur (déjà le référentiel du tampon à cet
  étage), mais reporte l'inversion de la rotation sur Studio, qui devrait
  alors la recalculer à chaque interaction de masque/tache en plus de la
  faire déjà pour les poignées de recadrage — complexité déplacée vers la
  couche la moins outillée pour la porter, et rupture avec le précédent de
  `crop`.
* **Coordonnées relatives au cadre final (après rotation ET recadrage)** :
  la plus intuitive pendant l'édition (c'est ce que l'utilisateur voit),
  mais rend le recadrage destructif pour les masques/taches : rétrécir ou
  déplacer le rectangle de recadrage change l'origine et l'étendue du
  référentiel, invalidant silencieusement toutes les positions stockées —
  écarté, contredit l'édition non destructive du recadrage indépendamment
  des autres réglages.
* **Réordonner le pipeline pour exécuter masques/taches après
  Rotation/Recadrage** : rendrait le référentiel trivialement cohérent
  (l'étage tournerait dans le même cadre que la géométrie), mais tout
  changement d'ordre d'un pipeline existant impose déjà une nouvelle
  process version (§3.3) — aucune économie — et déplacerait la suppression
  de tache/les réglages locaux après recadrage, changeant quels pixels
  alimentent réduction de bruit et netteté. Un chantier de réordonnancement
  plus large et non nécessaire ici : une transformation de coordonnées
  suffit.
