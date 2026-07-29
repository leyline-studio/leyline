# ADR 0049 — Exposer les retouches locales aux clients : outils de tracé dans Studio, payload JSON dans la CLI

**Statut :** Accepté — 2026-07

## Contexte

[ADR 0029](0029-process-6-local-adjustments.md) a livré les masques (brosse,
radial, gradué) et [ADR 0048](0048-range-masks.md) leur raffinement par plage.
Les deux sont **entièrement implémentés dans le moteur** : `Mask`,
`LocalAdjustment`, `RangeMask`, la rastérisation de `mask.rs`, les étages
`local_adjustments::v1` et `v2`, et le pilotage par
`Param::LocalAdjustment(index)`.

Aucun client ne les expose. Ni Studio (aucune occurrence de
`local_adjustments` sous `crates/leyline-studio/`), ni la CLI (idem). ADR 0048
§6 l'écrivait noir sur blanc et renvoyait la question à « un travail à part
entière » : c'est celui-ci.

Le manque est le plus grave du projet. La retouche locale est la fonction
centrale de darktable et de Lightroom, et ici elle n'est atteignable qu'en
écrivant du JSON à la main à travers le SDK. Du code écrit, testé et gelé ne
sert à personne.

**Ce qui n'est pas en cause.** Le modèle de données, le référentiel de
coordonnées ([ADR 0026](0026-mask-spot-coordinate-referential.md)), la
sémantique des étages, la frontière SDK. Cette décision ne touche que les deux
clients : elle n'ajoute aucun réglage, aucun étage, aucune version.

## Décision

### 1. Une retouche locale est un *outil*, pas un curseur

Le panneau develop expose une liste de retouches, pas un jeu de curseurs :
chaque entrée de `Settings::local_adjustments` est une ligne, sélectionnable,
supprimable, et dont les valeurs se règlent en dessous. C'est la forme de
Lightroom et de darktable, et c'est celle que le modèle impose déjà — un
`Vec<LocalAdjustment>` piloté par index.

La géométrie se **trace sur l'image**, jamais au clavier : un outil actif dans
la barre au-dessus de la preview, comme *Crop* et *Spot Removal* le font déjà
(ADR 0032). Trois gestes, un par géométrie :

| Outil | Geste | Ce qu'il écrit |
| :--- | :--- | :--- |
| Radial | un glissement | l'ellipse inscrite dans le rectangle glissé, `angle: 0` |
| Gradué | un glissement | l'axe : appui = couverture pleine, relâché = couverture nulle |
| Brosse | des clics | un `BrushStroke` par clic, ajouté au tracé |

`Mask::Everything` (ADR 0048 §1) n'a pas de géométrie à tracer : il s'ajoute
depuis le panneau, par un bouton, ce qui est exactement l'emploi prévu — une
plage sans géométrie.

### 2. Le geste crée l'entrée ; il n'y a pas d'entrée vide

`Settings::validate()` refuse un `Mask::Brush` sans dab. Une brosse ne peut
donc pas être créée avant son premier coup, et l'interface ne fait pas
semblant du contraire : le premier dab de l'outil Brosse **crée** la retouche,
les suivants l'allongent. Radial et gradué, dont la géométrie est complète dès
le relâchement, suivent la même règle par cohérence : le glissement crée.

Une entrée sélectionnée est retracée au lieu d'être doublée — le même
glissement sur une retouche radiale déjà sélectionnée remplace sa géométrie.
Sans cela, corriger un radial mal placé demanderait de le supprimer d'abord.

### 3. Valeur neutre = *absente*, sauf la balance des blancs

`LocalAdjustmentValues` est un jeu d'`Option` : `None` signifie « pas de
changement ici », ce qui n'est pas la même chose que « 0 ». Pour les huit
champs dont le neutre *est* zéro (exposition, contraste, hautes lumières,
ombres, blancs, noirs, éclat, saturation), la distinction n'a aucune
conséquence de rendu, et un curseur ramené à zéro écrit donc `None` : le
`settings_json` reste propre, et la retouche n'énumère que ce qu'elle change.

`temperature` et `tint` n'ont pas cette propriété — `temperature: 0` est
refusé par `validate()`, et 0 n'est pas un neutre de teinte. La paire est donc
pilotée par un interrupteur explicite, qui l'amorce à la température globale
de la photo quand on l'active et la remet à `None` quand on l'éteint. Même
raisonnement pour les deux termes de `RangeMask`, dont `None` est la seule
manière de dire « pas de terme ».

### 4. La CLI prend le JSON stocké, pas une grammaire de son invention

```
leyline develop <lib> <ver> local-adjustment <json|@fichier>
leyline develop <lib> <ver> local-adjustment rm <index>
leyline develop <lib> <ver> local-adjustment reset
```

Le payload est **exactement** la forme sérialisée d'un `LocalAdjustment`, celle
que `settings_json` contient et que `docs/pipeline.md` §3.2 documente. Une
grammaire positionnelle à la manière de `spot-removal` demanderait sept à
douze champs dans un ordre à retenir, plus une syntaxe imbriquée pour `range` —
soit un second dialecte à documenter, à valider et à faire vieillir en
parallèle du premier.

Le JSON est validé par `Settings::validate()` comme n'importe quel réglage :
un payload mal formé ou hors bornes est une erreur nommée, pas un silence.
`@fichier` lit le payload sur disque, parce qu'un masque de brosse à trente dabs n'entre
pas dans une ligne de commande.

### 5. Ce que l'interface montre de la couverture

Studio dessine le **contour** de la géométrie sélectionnée sur la preview —
l'ellipse d'un radial, l'axe d'un gradué, les dabs d'une brosse — calculé côté
UI à partir de la géométrie stockée, sans rien demander au moteur.

Le contour ne reflète pas la rotation d'une ellipse (`angle`) : les outils
n'en écrivent jamais — un glissement à deux coins n'a pas de rotation à
rapporter (§1) — et une ellipse inclinée écrite par la CLI ou le SDK montre
donc son contour non tourné.

Il ne dessine **pas** la couverture réelle en surimpression (le « masque rouge »
de Lightroom). Cette couverture inclut le terme de plage, donc dépend des
pixels : la produire demanderait au moteur un rendu de masque, c'est-à-dire une
nouvelle sortie de rendu à spécifier, à mettre en cache et à mettre à l'échelle
comme la preview (ADR 0041). C'est une décision de moteur, séparable, et son
absence ne bloque pas le geste : le contour suffit à savoir *où* on a tracé.

### 6. Hors périmètre

* **La surimpression de couverture calculée par le moteur** (§5), y compris
  pour le terme de plage.
* **La sélection de plage par pipette** sur un pixel désigné (les points de
  contrôle de DxO), déjà nommée comme faisable sans décision moteur par
  ADR 0048 §6 — elle demande une pipette, donc sa propre tranche.
* **Le déplacement d'une géométrie déjà tracée par poignées.** Retracer
  remplace (§2) ; des poignées sont de l'ergonomie pure, ajoutable après.
* **La copie de retouches locales entre photos.** `SettingsGroup`
  (`docs/presets.md` §3.1) ne comporte pas de groupe pour elles, et lui en
  ajouter un touche les presets, pas les clients.

## Conséquences

* **Le moteur cesse d'avoir des fonctions inaccessibles.** Les deux ADR les
  plus coûteuses en pixels (0029, 0048) deviennent utilisables par un
  photographe, ce qui était leur objet.
* **La barre d'outils de develop passe de trois outils à six**, sur le même
  mécanisme d'`active-tool` : aucun nouveau chemin d'interaction, aucune
  nouvelle convention de coordonnées — la boîte à lettres de `letterbox_unit`
  sert les trois nouveaux gestes comme elle sert le crop et le spot.
* **Les règles de décodage restent pures et testées** dans `crate::masks`
  (Studio), libres de tout type Slint, comme `crate::develop` l'est déjà.
* **La CLI gagne un payload JSON, ce qu'aucune autre commande n'avait.** C'est
  assumé et borné à ce cas : la justification (§4) est la profondeur de la
  structure, pas la commodité.
* **`ui/state/mask.slint` est le huitième global de domaine** (ADR 0045 §1).
  Il porte la liste des retouches, les trois champs de la brosse **et la ligne
  sélectionnée** : celle-ci traverse la frontière parce qu'un geste qui *crée*
  une retouche doit la sélectionner depuis Rust (§2). L'outil actif et les
  replis, que Rust ne lit jamais, restent privés au panneau.

## Alternatives écartées

* **Une grammaire positionnelle dans la CLI**, par symétrie avec
  `spot-removal`. Écartée au §4 : un second dialecte pour la même structure.
* **Des champs numériques dans Studio pour la géométrie** (cx, cy, rx, ry…).
  Écrit vite, inutilisable : personne ne place un radial en tapant des
  pourcentages. Le tracé est le geste, ce qui est précisément pourquoi les
  concurrents n'offrent que lui.
* **Un curseur « désactivé » par champ** plutôt que la règle « neutre =
  absent » du §3. Deux fois plus de contrôles pour distinguer deux états dont
  le rendu est identique.
* **Attendre la surimpression de couverture pour livrer l'interface.** Cela
  garderait la fonction centrale du logiciel inaccessible en attendant une
  décision de moteur indépendante. Le contour (§5) est ce qui rend le tracé
  utilisable ; le reste est un raffinement.
* **Exposer les masques uniquement dans la CLI**, en attendant une refonte de
  l'interface. La CLI ne sert pas le geste : tracer une brosse en JSON n'est
  pas retoucher.
