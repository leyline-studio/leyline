# ADR 0048 — Masques par plage : un raffinement de luminance et de couleur, déterministe, au-dessus des masques géométriques

**Statut :** Accepté — 2026-07

## Contexte

[ADR 0029](0029-process-6-local-adjustments.md) a livré trois masques :
brosse, radial, gradué. Tous les trois sont de la **géométrie pure** —
`crates/leyline-engine/src/mask.rs` le dit dans son premier paragraphe, et sa
fonction de rastérisation ne reçoit d'ailleurs aucun pixel, seulement une
taille et un angle. Le photographe désigne donc *où*, jamais *quoi*.

C'est la moitié du geste. Assombrir un ciel demande de désigner le ciel, pas
le haut du cadre : un gradué mord sur la montagne, une brosse sur chaque
branche d'arbre qui dépasse. Les concurrents résolvent exactement ce cas, et
c'est leur argument de vente le plus visible :

* **DxO** avec ses masques U Point, présentés comme « masques IA » mais qui
  sont, sous le marketing, une sélection par **couleur et luminance** ;
* **Lightroom** avec son *Range Mask* (plage de luminance, plage de couleur),
  qui vient **raffiner** un masque local existant plutôt que le remplacer ;
* **Capture One** avec ses masques combinés.

La sélection de sujet ou de ciel par réseau de neurones (Capture One *People
Masking*, Luminar *Sky AI*) tombe, elle, sous l'exclusion d'IA de
`docs/specification.md` §4 : elle n'est pas visée ici et le présent ADR ne la
rouvre pas. Ce qui est visé est précisément la partie que le marketing
concurrent appelle IA sans qu'elle en soit : un seuillage sur des grandeurs
que le pixel porte déjà.

**Ce qui n'est pas en cause.** Le modèle de composition d'ADR 0029
(`output = lerp(buffer, opérateurs_locaux(...), couverture)`), le référentiel
de coordonnées d'[ADR 0026](0026-mask-spot-coordinate-referential.md), et le
jeu de réglages qu'un masque peut re-paramétrer. Seule la **provenance de la
couverture** change.

## Décision

### 1. Un raffinement, pas un quatrième masque

Une plage ne remplace pas un masque : elle le **multiplie**.

```
couverture = géométrie(x, y) × plage_luminance(pixel) × plage_couleur(pixel)
```

C'est le modèle de Lightroom, et il est plus expressif que celui d'un masque
autonome pour une raison concrète : le geste réel est « je brosse
grossièrement, puis je restreins au bleu du ciel ». Un masque de plage
autonome ne saurait pas exprimer cela ; un raffinement exprime les deux, le
cas autonome étant le raffinement d'une géométrie qui couvre tout.

Concrètement, `LocalAdjustment` gagne un champ optionnel :

```rust
pub struct LocalAdjustment {
    pub mask: Mask,
    pub range: Option<RangeMask>,   // nouveau, None = comportement d'ADR 0029
    pub opacity: f64,
    pub adjustments: LocalAdjustmentValues,
}
```

et `Mask` gagne une variante `Everything` — couverture pleine, deux lignes
dans le rastériseur — pour que la plage puisse se passer de géométrie sans
qu'on ait à détourner un radial géant ou un gradué dégénéré.

### 2. Deux termes, tous deux facultatifs et tous deux à bords doux

```rust
pub struct RangeMask {
    /// Bande de luminance sur l'axe d'affichage, `None` = pas de terme.
    pub luminance: Option<LuminanceRange>,
    /// Bande de teinte, `None` = pas de terme.
    pub color: Option<ColorRange>,
}
```

* **Luminance** : `min`, `max` dans `[0, 1]`, plus `softness`. Pleine
  couverture entre `min` et `max`, décroissance lissée
  (`smoothstep`) sur une largeur `softness` de part et d'autre. Un bord dur
  produirait un contour visible dès que le bruit fait osciller un pixel
  autour du seuil — c'est la raison d'être du paramètre, pas un ornement.
* **Couleur** : `center` (teinte en degrés), `width` (demi-largeur en degrés),
  `softness`. La teinte est circulaire, donc la distance est prise modulo 360.

Un pixel **sans chroma n'a pas de teinte** : un gris n'est ni rouge ni bleu, et
lui en attribuer une par convention ferait entrer tous les gris dans n'importe
quelle bande de couleur. Le terme de couleur pondère donc par la saturation du
pixel, de sorte qu'un gris reçoive une couverture nulle. C'est ce qui rend
« restreindre au bleu du ciel » utilisable sans sélectionner aussi les nuages.

### 3. L'axe : celui de l'affichage

Les deux termes sont évalués sur l'axe d'affichage (`kernel::v1::display`,
ADR 0044), pas en lumière linéaire. « Luminance 0,3 à 0,7 » doit désigner ce
que l'utilisateur voit sur son histogramme, et un intervalle linéaire
équivalent placerait sa borne basse dans le noir absolu. Même raisonnement que
pour les opérateurs de tonalité.

Le pixel évalué est celui **du tampon tel qu'il arrive à l'étage** — donc après
tous les opérateurs globaux, comme le reste d'ADR 0029. Une plage se règle en
regardant l'image telle qu'elle est à l'écran, ce qui est aussi la seule
définition qu'un utilisateur peut prédire.

### 4. Le code de la plage vit dans une version d'étage, pas dans `mask.rs`

`mask.rs` est partagé, non versionné, et le justifie explicitement : il
« n'encode aucune formule de transformation de pixel ». Une plage **est** une
formule sur les pixels. L'y mettre ferait dépendre le rendu gelé de
`local_adjustments::v1` d'un code modifiable, ce qu'ADR 0042 §1 interdit.

Donc :

* `mask.rs` garde la géométrie seule, y compris `Mask::Everything` (qui est de
  la géométrie) ;
* le calcul de la plage et sa composition avec la géométrie vivent dans
  **`local_adjustments::v2`**, gelé comme n'importe quelle version d'étage ;
* `local_adjustments::v1` n'est pas touchée et continue de rendre ce qu'elle
  rendait.

### 5. Une plage sur une révision épinglée en `v1` est **refusée**, pas ignorée

Le piège de cette forme : une révision de 2026 épingle
`local_adjustments: 1` ; un utilisateur y ajoute une plage en 2027 ; la règle
d'épinglage d'ADR 0042 §2 dit que l'étage **garde** sa version, donc `v1`
rendrait — et `v1` ne connaît pas les plages. Le réglage disparaîtrait sans un
mot.

C'est inacceptable, et la réponse n'est pas de relâcher l'épinglage :
`Settings::validate()` **refuse** un `range` non nul quand la carte `stages`
épingle `local_adjustments` en version 1. Le message nomme le remède, qui est
celui du projet : retraiter la photo vers les versions courantes
(`docs/pipeline.md` §4.5), ce qui crée une nouvelle révision épinglée en `v2`.

Une erreur explicite là où le silence était possible : c'est la même règle que
`MixedWorkingSpaces` applique déjà à un plan incohérent (ADR 0044 §4), et le
premier cas où une *capacité* — non un rendu — se révèle liée à une version
d'étage. La règle générale qui s'en dégage, et qui vaudra pour toute
fonctionnalité future ajoutée à un étage existant : **un réglage qu'une version
épinglée ne sait pas exprimer est un refus de validation, jamais une valeur
perdue.**

### 6. Hors périmètre

* **La détection de sujet, de ciel ou de visage.** Exclusion d'IA
  (`docs/specification.md` §4), inchangée.
* **La combinaison de plusieurs masques géométriques** (union, intersection,
  soustraction — les *Combined Masks* de Capture One). Utile, indépendant, et
  qui demanderait de changer la forme de `Mask` plutôt que de l'étendre : son
  propre ADR.
* **La plage de profondeur.** Il n'y a pas de carte de profondeur à lire.
* **L'exposition dans Studio et la CLI.** Les réglages locaux d'ADR 0029
  eux-mêmes n'y sont pas encore exposés — ni brosse, ni radial, ni gradué —
  donc les plages arrivent au même niveau que ce qu'elles raffinent : moteur,
  SDK et sessions d'édition (`Param::LocalAdjustment`). Exposer les masques
  aux clients est un travail à part entière, et le manque est antérieur à
  cette décision.

## Conséquences

* **Le geste « assombrir ce ciel » devient faisable** sans mordre sur la
  montagne, avec le même vocabulaire que les concurrents (plage de luminance,
  plage de couleur) et sans emprunter à leur marketing d'IA.
* **`settings_json` gagne un champ optionnel à valeur neutre absente**, ce qui
  est explicitement compatible au sens de `docs/pipeline.md` §3.4 : `schema`
  n'est pas incrémenté.
* **Deuxième version d'étage réelle du projet**, après ADR 0046. Le mécanisme
  d'ADR 0042 sert maintenant deux fois, et sert ici pour une raison différente
  — non pas corriger un rendu, mais **étendre** un opérateur — ce qui exerce
  la règle d'épinglage sous un angle que rien n'avait encore éprouvé (§5).
* **Le coût est proportionnel au raffinement demandé** : sans `range`, rien ne
  change ni en pixels ni en temps ; avec, un passage supplémentaire sur les
  pixels couverts.
* **`mask.rs` conserve son invariant** — géométrie pure, partageable par toutes
  les versions — ce qui était la raison de ne pas y toucher.

## Alternatives écartées

* **Une quatrième variante de `Mask`, autonome.** Moins expressive (pas de
  raffinement d'une brosse), et elle placerait quand même une formule de
  pixels dans le rastériseur partagé, donc dans `mask.rs` — le problème du §4
  sans le bénéfice du §1.
* **Étendre `mask.rs` avec les plages.** Écarté au §4 : le rendu gelé de `v1`
  dépendrait d'un code que rien n'empêche de modifier.
* **Ignorer silencieusement un `range` sur une révision en `v1`.** Écarté au
  §5. C'est le comportement qu'on aurait obtenu sans y penser, et le plus
  mauvais : l'utilisateur voit son curseur ne rien faire.
* **Rendre `local_adjustments::v1` capable de lire les plages** (donc modifier
  un module gelé). Interdit par ADR 0042 §1, et sans nécessité : `v2` coûte
  quelques dizaines de lignes.
* **Une sélection par proximité de couleur au point cliqué**, à la manière des
  points de contrôle DxO. C'est une interface au-dessus de la même mécanique,
  pas une mécanique différente : elle calcule un `center`/`width` à partir du
  pixel désigné. À faire quand les masques auront une interface, sans nouvelle
  décision de moteur.
