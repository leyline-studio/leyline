# ADR 0052 — Correction de perspective : une homographie à deux curseurs, entre la rotation et le recadrage

**Statut :** Accepté — 2026-07

## Contexte

Le pipeline sait redresser un horizon (`rotation`) et recadrer (`crop`). Il ne
sait pas redresser des **verticales fuyantes** : photographier un bâtiment en
levant l'appareil fait converger ses arêtes, et aucun réglage de Leyline ne
touche à cela. C'est le trou fonctionnel le plus franc du pipeline
géométrique — il n'a même pas d'ADR qui l'écarte, contrairement au *heal*
([ADR 0032](0032-spot-removal-clone.md)) ou au GPU
([ADR 0012](0012-rayon-data-parallelism.md)).

Tous les concurrents l'ont : darktable (*rotate and perspective*), RawTherapee
(*transform*), Lightroom (*Transform*), digiKam. C'est aussi le premier réglage
que réclame quiconque photographie de l'architecture ou reproduit des documents.

**Ce qui n'est pas en cause.** Le référentiel de coordonnées d'
[ADR 0026](0026-mask-spot-coordinate-referential.md) (les masques et les taches
se placent *avant* la géométrie), l'ordre du pipeline, et la mise à l'échelle
des préversions ([ADR 0041](0041-interactive-preview-rendering.md)).

## Décision

### 1. Deux curseurs, et pas six

`Settings` gagne un champ optionnel :

```rust
pub struct Perspective {
    /// Correction verticale, curseur dans [-100, +100].
    pub vertical: i32,
    /// Correction horizontale, curseur dans [-100, +100].
    pub horizontal: i32,
}
```

Absent = neutre. Deux termes, parce que ce sont les deux que le geste
photographique produit : lever l'appareil (verticales fuyantes) et le tourner
(horizontales fuyantes).

Ce que Lightroom appelle *Aspect*, *Scale*, *X/Y Offset* n'entre pas. `Aspect`
est un étirement, pas une perspective ; `Scale` et les décalages sont un
recadrage, que `crop` fait déjà — les ajouter ici donnerait deux manières
d'exprimer la même chose, avec deux ordres d'application possibles et un
`settings_json` qui ne dirait plus laquelle a eu lieu.

**La correction automatique n'entre pas non plus** : détecter les lignes de
fuite demande une détection de contours et un vote de Hough, c'est-à-dire un
algorithme d'analyse d'image dont le résultat dépend du contenu. Ce serait une
décision à part entière (et un candidat sérieux : la mécanique de rendu, elle,
est celle-ci).

### 2. Une homographie, pas deux cisaillements

La correction est une **transformation projective** (homographie 3×3) dont les
coefficients viennent des deux curseurs : chaque curseur rapproche les deux
coins d'un bord et écarte ceux du bord opposé, dans le référentiel normalisé du
cadre. La matrice est ensuite **inversée** et le rendu échantillonne la source
en arrière (comme `rotate::v1`), en bilinéaire.

Un cisaillement affine — plus simple à écrire — ne corrige *pas* une
perspective : il incline les verticales sans changer leur convergence. Ce qui
distingue une correction de perspective d'un simple redressement est justement
la division par la troisième coordonnée.

### 3. Rang 205 : après la rotation, avant le recadrage

L'ordre est contraint des deux côtés :

* **après `rotate`** (rang 200), parce qu'un horizon droit est le repère par
  rapport auquel une verticale est verticale ; corriger la perspective d'une
  image penchée demanderait à l'utilisateur de composer mentalement les deux ;
* **avant `crop`** (rang 210), parce que la correction élargit le cadre (les
  bords deviennent des trapèzes) et qu'on recadre ce que l'on voit, pas
  l'inverse.

Le rang 205 était libre, ce qui est exactement à quoi servent les dizaines
d'ADR 0042.

### 4. Le cadre grandit, il ne se remplit pas

Comme `rotate::v1`, l'étage rend la **boîte englobante** du quadrilatère
transformé, et les pixels qui n'ont pas de source restent noirs. Aucun
remplissage, aucun recadrage automatique dans le contenu utile.

C'est cohérent avec la rotation, qui fait déjà exactement cela, et c'est ce que
`crop` sert à corriger — l'utilisateur voit ce que la correction a produit et
décide lui-même de ce qu'il garde. Un recadrage automatique déciderait à sa
place, et perdrait des pixels qu'il aurait peut-être voulu garder.

### 5. Une valeur normalisée, donc indépendante de la taille

Les deux curseurs n'expriment aucune longueur en pixels : ils déplacent des
coins en fractions du cadre. Une préversion réduite (ADR 0041) et un export
pleine résolution subissent donc **la même** transformation, sans facteur
d'échelle à propager — contrairement aux rayons de flou de la clarté ou du
débruitage.

### 6. Hors périmètre

* **La correction automatique** (§1).
* **La correction de l'objectif** — distorsion, TCA, vignettage — qui est un
  autre problème, déjà traité par Lensfun ([ADR 0016](0016-process-3-lens-correction.md)–[0018](0018-process-5-tca.md)) et à un autre rang.
* **Le recadrage automatique dans le contenu utile** (§4).
* **`Aspect`, `Scale`, décalages** (§1).

## Conséquences

* **Le dernier trou du pipeline géométrique se ferme**, avec un étage neutre par
  défaut : aucune révision existante ne change de rendu.
* **Un étage de plus dans le pipeline géométrique**, donc un rééchantillonnage
  de plus quand il est actif. C'est le prix d'un ordre lisible : composer
  rotation et perspective en une seule matrice serait plus propre en pixels,
  mais ferait dépendre le rendu de `rotate` d'un réglage qui n'est pas le sien,
  et casserait le gel de `rotate::v1`.
* **`settings_json` gagne un champ optionnel à valeur neutre absente**, donc
  `schema` n'est pas incrémenté (`docs/pipeline.md` §3.4).
* **Les trois clients l'exposent** : deux curseurs dans le groupe *Geometry* de
  Studio, `develop … perspective <vertical> <horizontal>` dans la CLI.

## Alternatives écartées

* **Un cisaillement affine.** Ne corrige pas une perspective (§2).
* **Composer la perspective dans `rotate`**, pour n'échantillonner qu'une fois.
  Interdit par le gel : `rotate::v1` rend ce qu'elle rend, et une `rotate::v2`
  qui lirait un nouveau réglage obligerait toute révision voulant la
  perspective à changer aussi de version de rotation, donc de rendu de
  rotation. Deux étages indépendants sont la forme qu'ADR 0042 rend possible.
* **Huit paramètres (les quatre coins).** Plus expressif, et inutilisable au
  clavier ; l'interface qui les rendrait utiles est un tracé de quadrilatère sur
  l'image, à faire par-dessus la même mécanique le jour où le besoin s'en fait
  sentir — comme les poignées de masque d'[ADR 0049](0049-local-adjustments-clients.md) §6.
* **Recadrer automatiquement après correction.** §4.
