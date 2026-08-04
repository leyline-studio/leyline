# ADR 0071 — Voir un masque : la couverture en surimpression

**Statut :** Accepté — 2026-08

## Contexte

[ADR 0049](0049-local-adjustments-clients.md) a livré les outils de masquage
dans les trois clients et a laissé trois choses de côté, dont la première :
**la surimpression de la couverture calculée par le moteur**. Elle est restée
ouverte depuis.

Elle n'est plus un confort. Trois raisons, dans l'ordre où elles pèsent :

1. **Un masque stocké n'a aucune poignée.** [ADR 0070](0070-stored-mask-coverage.md)
   vient d'ajouter `Mask::Coverage` : un radial dessine son ellipse, un dégradé
   son axe, une brosse ses dabs — une couverture importée ne dessine *rien*.
   On l'importe et on devine son effet au résultat. La surimpression est le
   seul moyen de la voir.
2. **Un masque par plage ne se devine pas du tout.** ADR 0048 multiplie la
   géométrie par des bandes de luminance et de teinte ; le résultat n'a plus
   de forme prévisible.
3. C'est ainsi qu'on travaille un masque partout ailleurs. Voir la zone
   couverte n'est pas une aide au débogage, c'est l'outil.

## Décision

**Studio peut afficher, par-dessus l'aperçu, la couverture du masque
sélectionné.**

### 1. C'est une *vue*, jamais un rendu

La surimpression ne produit aucun pixel de photo, n'entre dans aucune
révision, et n'a **pas de version d'étage**. `pipeline.md` §5.1 n'est pas
concernée : rien de ce qui est gelé ne change, et rien de ce qui est affiché
ici ne sera jamais exporté.

C'est la même nature que l'épreuvage écran d'[ADR 0051](0051-watermark-rasterization-and-soft-proof-surface.md) :
une transformation d'affichage, décidée par l'interface, que le moteur calcule
mais n'enregistre pas.

### 2. Elle doit tomber au bon endroit, donc elle passe par la géométrie

C'est tout le problème, et la raison pour laquelle la surimpression n'est pas
un simple dessin côté interface.

Une couverture est rasterisée dans le repère du **tampon de travail**, non
tourné (`mask::rasterize_coverage`, ADR 0026). L'aperçu affiché, lui, sort de
trois étages de géométrie qui s'exécutent *après* les réglages locaux :
`rotate` (rang 200), `perspective` (205) et `crop` (210). Une couverture
dessinée sans eux serait décalée, inclinée, et déborderait du cadre — d'autant
plus visiblement que le recadrage est serré.

La couverture est donc **poussée à travers ces trois étages-là**, aux versions
que la révision épingle, exactement comme les pixels de la photo. Rien n'est
réimplémenté : ce sont les mêmes étages gelés, appliqués à un autre tampon.

Les étages entre 160 et 200 — LUT, bruit, accentuation — sont au contraire
**sautés** : ce sont des opérateurs de pixels, ils déformeraient une image de
masque sans rien lui apporter.

### 3. Elle montre la couverture *effective*, plages comprises

Montrer la seule géométrie serait montrer ce qu'on sait déjà, et taire ce
qu'on ne devine pas (§Contexte, point 2). La surimpression est donc calculée
sur le tampon tel qu'il est au rang 160 — après les opérateurs de couleur, là
où les termes de plage lisent leurs valeurs — puis multipliée par l'opacité de
l'entrée.

Ce que l'utilisateur voit est ce que le moteur applique.

**Le prix, assumé :** le terme de plage vit dans les modules gelés de
`local_adjustments`, où il est déjà recopié de version en version. La
surimpression le recopie une fois de plus, dans un module qui **aiguille sur
la version épinglée**. C'est la contrepartie du gel, et elle est bornée par la
même propriété : un module gelé ne reçoit jamais de correctif, seulement un
successeur, donc deux copies ne peuvent pas diverger.

### 4. Un seul masque à la fois : celui qui est sélectionné

Superposer plusieurs couvertures donnerait une bouillie où l'on ne sait plus
quelle entrée fait quoi. Le panneau a déjà une notion de ligne sélectionnée
(ADR 0049) ; la surimpression suit cette sélection, et disparaît quand rien
n'est sélectionné.

### 5. Rouge à 50 %, et un interrupteur

La convention de tous les logiciels du domaine, et elle est bonne : une teinte
franche qu'aucune photo ne contient uniformément, assez transparente pour
qu'on juge encore l'image dessous.

L'affichage est un **interrupteur** de l'interface, pas un réglage de la
photo : il ne s'enregistre pas dans la révision, et repart à l'état affiché à
chaque sélection de masque — c'est ce qu'on veut voir en travaillant un
masque, et jamais ce qu'on veut retrouver figé sur une photo trois mois plus
tard.

### 6. Ce que cette ADR ne fait pas

Les deux autres reliquats d'ADR 0049 restent ouverts, et chacun est un travail
distinct : la **pipette de plage**, et les **poignées de déplacement** d'une
géométrie déjà tracée.

## Conséquences

* Le moteur gagne une surface : rendre la couverture d'un masque à la taille
  d'un aperçu, `Library::mask_coverage_preview`. Elle rend une image en niveaux
  de gris, jamais une couleur — la teinte est une décision d'interface.
* Un masque stocké devient **utilisable** : jusqu'ici on l'importait à
  l'aveugle (ADR 0070 §7).
* Un coût de calcul supplémentaire, du même ordre qu'un aperçu, et payé
  seulement quand la surimpression est allumée.
* Le terme de plage existe désormais en deux exemplaires de plus, aiguillés par
  version. C'est écrit ici pour que ce ne soit pas découvert plus tard comme
  une négligence.

## Alternatives écartées

* **Dessiner la géométrie côté interface**, en Slint, sans passer par le
  moteur. Ne sait rien des plages ni des couvertures stockées — donc muet
  précisément là où on a besoin de voir — et devrait réimplémenter rotation,
  perspective et recadrage pour tomber juste.
* **Ne montrer que la géométrie**, sans les plages. Moins cher, et il tait ce
  qu'on ne devine pas.
* **Superposer tous les masques** avec une couleur par entrée. Illisible dès
  trois entrées, et ne dit plus laquelle on est en train de régler.
* **Un rendu complet avec le masque substitué à l'image**, en laissant tous les
  étages s'exécuter. La LUT, le bruit et l'accentuation déformeraient l'image
  de masque : on verrait un masque accentué, pas le masque.
* **Enregistrer l'état de la surimpression dans la révision.** Ce n'est pas une
  propriété de la photo ; la retrouver allumée trois mois plus tard serait une
  surprise, pas un service.
