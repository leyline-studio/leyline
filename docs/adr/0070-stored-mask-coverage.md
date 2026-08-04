# ADR 0070 — Un masque peut être une couverture calculée, pas seulement une géométrie

**Statut :** Accepté — 2026-08

## Contexte

`Mask` sait décrire quatre choses (ADR 0029, [ADR 0048](0048-range-masks.md)) :
une ellipse, un dégradé, un tracé de pinceau, et « tout ». Les quatre ont un
point commun — ce sont des **formules**. `rasterize_coverage` les évalue par
pixel, à la résolution du rendu en cours, et n'a donc besoin de rien d'autre
que quelques nombres rangés dans `settings_json`.

C'est exactement ce qui manque à un masque de sujet, de ciel ou d'arrière-plan
(C2 de `competitive-plan.md`). Sa couverture n'est pas dérivable de six
paramètres : c'est une image. Tant que `Mask` ne sait porter que des formules,
un tel masque **n'est pas exprimable**, et [ADR 0069](0069-closed-extension-boundary.md)
n'a rien à quoi s'attacher — sa règle « une extension produit des réglages,
jamais des pixels » suppose que le réglage produit puisse exister.

Cette ADR est donc la moitié **ouverte** et gratuite de ce dispositif : le
moteur libre apprend à *stocker et rendre* une couverture, quel que soit ce qui
l'a produite.

Ce n'est d'ailleurs pas propre à l'IA. Un masque peint dans un autre logiciel,
une sélection exportée en PNG, un masque de luminance calculé une fois et figé :
tous se heurtent au même mur aujourd'hui.

## Décision

**`Mask` gagne une variante `Coverage`, qui référence un fichier de couverture
au lieu de décrire une forme.**

```rust
Mask::Coverage {
    /// Chemin relatif à la bibliothèque (`catalog.md` §2.3).
    path: String,
    /// BLAKE3 des octets du fichier, « blake3:<hex> ».
    checksum: String,
}
```

### 1. La forme est celle qui existe déjà deux fois

Chemin relatif + checksum BLAKE3 : c'est exactement la forme de
[`CameraProfile`](0035-camera-profile-dcp.md) et de [`Lut`](0053-creative-lut.md),
et pour les mêmes raisons — une bibliothèque reste portable, et une
substitution de fichier est **détectée** au lieu d'être rendue en silence. Un
checksum qui ne correspond plus est une erreur, jamais un rendu différent sans
prévenir.

La résolution du fichier se fait **avant** le rendu, comme pour ces deux-là
(`camera_profile::resolve_from_settings`, `lut::resolve_from_settings`) : le
chemin des pixels ne lit jamais un fichier, et l'échec a un point unique et
nommé.

### 2. Le fichier : PNG gris 16 bits, à sa propre résolution

**Sans perte**, parce qu'un masque compressé avec perte ferait dériver le rendu
d'une révision sans que rien ne le signale.

**16 bits et non 8.** Une couverture multiplie un réglage : sur un dégradé
doux poussé de plusieurs EV, 256 niveaux se voient en bandes. Les masques
géométriques sont justement évalués en `f64` pour éviter cela ; stocker en
8 bits rendrait le chemin *stocké* moins bon que le chemin *calculé*
précisément dans le cas où l'écart se voit. Le PNG gris 16 bits coûte deux fois
plus d'octets avant compression, et un masque — de grandes zones uniformes
séparées par une transition fine — se comprime très bien.

**À sa propre résolution, sans plafond imposé.** Un modèle de segmentation
produit typiquement 512 à 1024 pixels de côté ; stocker cela ré-échantillonné à
la taille du capteur fabriquerait du détail qui n'existe pas et multiplierait
les octets par trente pour rien. Le fichier porte donc la résolution que son
producteur avait réellement, et le moteur ne l'invente pas.

Corollaire à assumer : **la finesse d'un masque stocké est celle de son
fichier.** Agrandi vers un export pleine résolution, un masque de 1 024 pixels
donne un bord doux, pas un bord net. C'est une limite du masque, pas du moteur,
et c'est au producteur de stocker à une résolution à la hauteur de la
complexité du bord qu'il décrit.

### 3. Les coordonnées sont celles des autres masques

Le fichier est échantillonné **bilinéairement sur le canevas normalisé
`[0,1]²`** — le repère post-rotation dans lequel `rasterize_coverage` évalue
déjà les quatre variantes existantes (`CanvasFrame`, ADR 0026).

Autrement dit, un masque stocké est **la même fonction de la position que les
masques géométriques, tabulée au lieu d'être calculée**. Il suit la rotation,
il se combine avec un masque par plage (ADR 0048) et une opacité comme
n'importe quel autre, et il est indépendant de la résolution du rendu : aperçu
et export l'échantillonnent pareil.

### 4. `local_adjustments::v3`, et le refus des versions gelées

Rendre une variante que `v1` et `v2` ne connaissent pas est un rendu nouveau,
donc une **nouvelle version d'étage** (`pipeline.md` §5.1).

`v3` rend les quatre variantes existantes **exactement** comme `v2` : elle
n'ajoute qu'un cas exprimable de plus. Les rendus de référence existants ne
bougent donc pas ; le manifeste gagne des entrées `v3` aux empreintes
identiques.

Et la règle de capacité s'applique telle quelle : **une révision épinglée sur
`v1` ou `v2` qui porte un `Mask::Coverage` est refusée par `validate()`**, avec
l'erreur qui le dit. Elle n'est pas rendue en ignorant le masque — un masque
ignoré, c'est un réglage local appliqué à toute l'image.

### 5. Écrire une couverture : la surface que l'extension utilise

```rust
impl Library {
    /// Range une couverture dans la bibliothèque et rend le masque prêt à
    /// être posé dans une révision.
    pub fn store_mask_coverage(&self, width: u32, height: u32,
                               coverage: &[u16]) -> Result<Mask>;
}
```

C'est le seul point d'entrée, et c'est celui qu'un crate fermé d'ADR 0069
appelle — par le SDK, comme n'importe quel client. Il écrit le fichier, calcule
le checksum, et rend le `Mask::Coverage` correspondant. L'appelant n'a jamais à
connaître ni le chemin, ni le format, ni l'emplacement.

**Les fichiers sont adressés par leur contenu** : `Masks/<blake3-hex>.png`,
sous la racine de la bibliothèque, à côté de `Profiles/Camera/` et
`Profiles/LUT/` (`catalog.md` §3). Deux masques identiques deviennent un seul
fichier, et réécrire le même masque ne fait rien. Le champ `path` reste
néanmoins explicite dans le réglage, comme chez ses deux prédécesseurs :
l'uniformité vaut mieux qu'un champ économisé, et elle laisse la porte ouverte
à une autre disposition plus tard.

### 6. Ce que cette ADR ne fait pas

* **Aucun masque n'est produit ici.** Le moteur libre sait *stocker et rendre*
  une couverture ; ce qui en *propose* une — modèle, runtime, poids — est hors
  périmètre, et ADR 0069 explique pourquoi cette séparation est le cœur du
  dispositif plutôt que sa réserve.
* **Aucune interface Studio.** Poser un `Mask::Coverage` depuis Studio
  supposerait un outil qui en fabrique un ; il n'y en a pas encore.
* **Aucun ramassage des fichiers orphelins.** Un masque référencé par une
  révision d'historique doit survivre à un `undo`, sinon un `redo` casse. Les
  masques ne sont donc **jamais** supprimés implicitement. Un ramassage des
  fichiers que plus aucune révision ne cite est un travail à part, avec sa
  propre ADR — il touche à l'historique, donc à ce qu'on promet de ne pas
  perdre.

## Conséquences

* `Mask` cesse d'être fermée sur la géométrie : un masque venu d'ailleurs — un
  autre logiciel, une sélection exportée, un modèle — devient exprimable, et
  tout ce qui existe déjà (plages, opacité, rotation, empilement) s'y applique
  sans rien de neuf.
* **La version libre rend les masques de tout le monde**, ce qui est la
  propriété qu'ADR 0069 §1 avait promise et que cette ADR livre.
* Une bibliothèque gagne un répertoire `Masks/` et pèse un peu plus lourd.
  `catalog.md` §3 le documente.
* Les couvertures résolues **voyagent comme le profil DCP et la LUT** : de
  `resolve_from_settings` au bord du moteur, à travers `render`,
  `render_scaled`, `develop_scaled` et le `Context` des étages, jusqu'à
  `rasterize_coverage`. Ce n'est pas un seul changement de signature mais la
  même chaîne que les deux références qui existaient déjà — et les versions
  d'étage gelées, elles, reçoivent une carte **vide**, ce qui rend leur
  immunité structurelle au lieu de dépendre du refus de `validate()`.
* Un fichier de masque manquant ou modifié est une **erreur de rendu**
  explicite, exactement comme un `.dcp` ou un `.cube` disparu.

## Alternatives écartées

* **Stocker la couverture dans `settings_json`**, en base64. Une couverture
  30 Mpx pèse 60 Mo en 16 bits, ~80 Mo encodée — dans une colonne TEXT, pour
  *chaque* révision de l'historique. La forme chemin + checksum existe déjà
  deux fois dans le projet précisément pour ce genre de donnée.
* **Un format avec perte** (JPEG, WebP lossy) pour économiser. Le rendu d'une
  révision dériverait avec le ré-encodage, ce que §5.1 interdit.
* **Imposer la résolution du capteur.** Fabrique du détail que le producteur
  n'avait pas, pour trente fois les octets.
* **Vectoriser la couverture** en contours pour rester dans une « formule ».
  Un masque de cheveux ou de feuillage n'est pas vectorisable sans le trahir,
  et l'approximation serait invisible dans le réglage tout en changeant le
  rendu.
* **Ranger le fichier à côté de la photo**, comme un sidecar. Contredit le
  contrat de non-destructivité (`pipeline.md` §6) : rien n'est écrit à côté des
  originaux.
* **Ne rien changer et rendre un masque IA depuis un greffon appelé au rendu.**
  C'est l'alternative qu'ADR 0069 a écartée, et cette ADR est ce qui la rend
  inutile.
