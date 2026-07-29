# ADR 0046 — Débruitage préservant les contours : ondelettes à trous et seuillage doux (`noise_luminance::v2`, `noise_color::v2`)

**Statut :** Accepté — 2026-07

## Contexte

Le débruitage livré est, littéralement, un flou.

`stages/noise_luminance/v1.rs` mélange le plan de luma vers son flou gaussien
(`add_luma_delta(px, |i| k * (blurred[i] - plane[i]))`, σ = `k · 2 · scale`), et
`stages/noise_color/v1.rs` fait de même sur l'écart de chaque canal à la luma.
Aucun terme ne dépend du contenu local : un contour et une zone plate reçoivent
exactement le même traitement. Monter le curseur retire du bruit **et** du
détail, dans la même proportion, ce qui plafonne l'outil très bas — au-delà
d'une quinzaine, il ne débruite plus, il adoucit.

C'est l'écart le plus sérieux du moteur face aux logiciels du marché, et il
tombe sur le profil d'usage que ces logiciels revendiquent en premier
(animalier, sport, astrophoto, mariage sans flash — tous à 6400 ISO et
au-delà). Un développeur RAW dont le débruitage est un flou gaussien n'est pas
utilisable sur ces images.

**Ce qui n'est pas en cause.**

* **L'exclusion de l'IA** (`docs/specification.md` §4). Elle n'est ni contournée
  ni renégociée : ce qui suit est un opérateur déterministe et fermé, sans
  modèle appris, sans poids à embarquer, sans inférence. Les débruiteurs
  appris (DeepPRIME, Topaz) restent hors périmètre et le présent document ne
  prétend pas les égaler (§7).
* **Le contrat de reproductibilité** (`docs/pipeline.md` §5.1). Le rendu de
  `noise_luminance::v1` et `noise_color::v1` ne bouge pas d'un bit : la
  correction est une **nouvelle version d'étage**, ce que
  [ADR 0042](0042-versioned-stage-pipeline.md) §1 impose et rend bon marché —
  quelques dizaines de lignes à côté de `v1`, et non une douzième copie du
  pipeline.
* **La surface de réglage.** Deux curseurs `0..100` (`noise_reduction.luminance`,
  `noise_reduction.color`), `settings_json` inchangé, `schema` non incrémenté.
  Aucun client (Studio, CLI, SDK) n'a de champ à ajouter : ce sont les mêmes
  valeurs, mieux dépensées.

## Décision

### 1. Deux nouvelles versions d'étages, à rangs et espace inchangés

`noise_luminance::v2` au rang 170, `noise_color::v2` au rang 180, tous deux en
`LinearRec2020`, mêmes prédicats `active` que leurs `v1`. Les `v1` ne sont pas
touchées et restent enregistrées : une révision qui les cite rend à
l'identique, pour toujours. Une révision nouvelle épingle `v2` par le mécanisme
ordinaire (`Stage::current`), sans table de correspondance ni cas particulier.

### 2. L'opérateur : ondelettes à trous, seuillage doux par échelle

Décomposition **non décimée** (*à trous*) du plan traité par le noyau B3-spline
séparable `[1, 4, 6, 4, 1]/16`, avec un espacement de trous de `2^l` au niveau
`l`, bords répliqués :

```
a₀ = plan
a_{l+1} = B3(a_l, espacement 2^l)      d_l = a_l − a_{l+1}
plan débruité = a_L + Σ seuil_l(d_l)
```

Le seuillage est **doux** : `sign(d) · max(|d| − t_l, 0)`.

Deux propriétés le rendent supérieur au flou de `v1`, et ce sont les deux
raisons de le choisir :

* **Le bruit n'a pas une échelle, il en a plusieurs.** Un flou à σ unique ne
  peut en attaquer qu'une : réglé sur le grain fin, il laisse les taches
  chromatiques ; réglé sur les taches, il détruit le détail. La décomposition
  sépare explicitement les échelles et applique à chacune son propre seuil.
* **La préservation des contours ne demande aucun détecteur de contours.** Un
  contour produit des coefficients grands devant le seuil, qui traversent donc
  l'opérateur (diminués de `t_l`, pas écrasés) ; le bruit produit des
  coefficients petits, qui tombent à zéro. Aucune heuristique, aucun paramètre
  de sensibilité, rien à régler pour l'utilisateur.

Le résidu `a_L` — la structure plus grossière que la dernière échelle analysée —
n'est **jamais** seuillé : le débruitage ne touche pas à la tonalité de l'image.

### 3. Seuils : le profil de bruit du noyau, mis à l'échelle par le curseur

Pour un bruit blanc gaussien, la décomposition ci-dessus concentre l'énergie
dans les premiers niveaux, avec des écarts-types connus par niveau
(`0.890, 0.201, 0.086, 0.041`). Le seuil suit ce profil :

```
t_l = k · BASE · σ_l          k = strength / 100
BASE_LUMA = 0.05              BASE_CHROMA = 0.12
```

`BASE_CHROMA` est plus du double parce que le bruit chromatique est à la fois
plus visible et moins porteur d'information : la chrominance d'une photo est
spatialement lisse presque partout, et un seuil agressif y coûte beaucoup moins
qu'en luminance. C'est la même dissymétrie que `v1` exprimait par ses σ (2 et 3),
mais placée là où elle a un sens.

Ces trois constantes, le noyau et le nombre de niveaux sont **gelés** au titre
du §1 : les changer serait une `v3`.

### 4. L'axe d'affichage est conservé

Comme `v1`, les deux étages travaillent sous `in_display` (ADR 0044). Ce n'est
pas un réflexe de copie : la visibilité du bruit est perceptuelle, un seuil
constant en lumière linéaire serait énorme dans les ombres et négligeable dans
les hautes lumières, et les deux curseurs ont été calibrés sur cet axe. La
marge au-dessus du blanc traverse l'opérateur sans être écrêtée, `in_display`
s'en chargeant déjà.

### 5. Rendu proxy : c'est le nombre de niveaux qui porte l'échelle

Le niveau `l` analyse des structures de l'ordre de `2^l` pixels. Sur une preview
réduite d'un facteur `scale` (ADR 0041), analyser les mêmes structures
*physiques* demande donc de retirer des niveaux, pas de rétrécir un rayon :

```
niveaux = clamp(1, 4, 4 + ⌊log₂(scale)⌋)
```

Une preview réduite 4× analyse 2 niveaux, ce qui couvre les mêmes détails de
l'image que 4 niveaux sur le rendu complet. C'est la traduction exacte, pour un
opérateur multi-échelle, de ce que les étages à rayon font en multipliant par
`scale`.

### 6. Le corps partagé va dans `kernel::v2`

La transformée et le seuillage servent aux deux étages, donc ne peuvent vivre
dans aucun des deux : ils vont dans `kernel::v2`, gelé au même titre que
`kernel::v1` (ADR 0042 §1, point 2). `kernel::v1` est réutilisé tel quel pour
`in_display`, `luma_plane` et `add_luma_delta` — appeler un module gelé depuis
un module neuf est sûr par construction, puisque le gelé ne changera jamais.

### 7. Ce que cette décision ne prétend pas

Énoncé ici pour que personne n'ait à le déduire d'un silence :

* **Pas de parité avec les débruiteurs appris.** DeepPRIME XD3 et Topaz Wonder
  reconstruisent du détail plausible ; un seuillage n'en reconstruit aucun, il
  se contente de ne pas détruire celui qui est là. L'écart se réduit
  franchement, il ne se ferme pas.
* **Pas de profil de bruit par boîtier et par ISO.** Le seuil est un modèle de
  bruit blanc uniforme, pas la variance mesurée du capteur à cette sensibilité
  (ce que fait darktable via ses profils). C'est la suite naturelle de cet
  opérateur, elle demande une base de mesures comme Lensfun en a une, et donc
  son propre ADR.
* **Pas de débruitage masqué.** Comme la clarté et le dehaze en ADR 0033, les
  deux curseurs restent globaux.

## Conséquences

* **Rien ne migre, rien ne casse.** Les révisions existantes citent `v1` et
  rendent `v1`. L'utilisateur qui veut le nouveau débruitage retraite sa photo
  (`docs/pipeline.md` §4.5), ce qui crée une nouvelle révision — l'ancienne
  reste rendable.
* **Les rendus de référence gagnent des entrées, aucune ne bouge.** Le manifeste
  golden voit apparaître les variantes `noise_*::v2` ; les entrées citant `v1`
  doivent rester bit-identiques, et c'est précisément ce qui prouve le §1. Le
  blessing est additif par construction.
* **Le coût CPU monte, d'un facteur mesuré.** Quatre niveaux = huit passes
  séparables de 5 taps par plan, contre une paire de passes gaussiennes ; la
  chrominance en traite trois. Le banc `denoise/` compare les deux versions
  épinglées, à réglages identiques (luminance 40, chroma 30) sur la trame
  synthétique de 3 Mpx : **103 ms pour `v1`, 223 ms pour `v2`** — un rendu
  complet, pas l'opérateur seul. Le facteur ~2,2 est payé sur un étage qui
  n'était bon marché que parce qu'il ne faisait pas le travail. L'opérateur
  reste O(N) par niveau et parallèle par lignes, et la preview en paie moins :
  moins de niveaux à l'échelle réduite (§5).
* **`docs/pipeline.md` §3.3 cesse d'être uniforme.** Le tableau des étages
  portait « tous en version 1 » depuis ADR 0043 : `noise_luminance` et
  `noise_color` y prennent une seconde ligne. C'est le premier `v2` réel du
  projet, donc la première fois que le mécanisme d'ADR 0042 sert hors du stage
  fixture de test.
* **L'écart annoncé dans la revue concurrentielle se réduit là où il était le
  plus grand**, sans toucher à une exclusion de périmètre.

## Alternatives écartées

* **Filtre bilatéral.** Préserve les contours, mais reste **mono-échelle** —
  c'est la limite de `v1` qu'on cherche à lever, et le bruit chromatique en
  taches large lui échappe. Coût O(r²) par pixel de surcroît, là où la
  transformée est O(N) par niveau.
* **Filtre guidé.** O(N) et préservant les contours, mais mono-échelle lui aussi,
  et il faut choisir une image guide — un paramètre de plus pour un résultat qui
  ne domine pas le seuillage multi-échelle sur le cas qui motive l'ADR.
* **Non-local means.** Meilleure qualité potentielle, déterministe, mais la
  recherche de patchs le rend hors de portée sur CPU à 24+ Mpx pour un rendu
  interactif (ADR 0012 ayant écarté le GPU).
* **Débruiteur appris.** Exclu par périmètre, et supposerait embarquer des poids
  — ce que le projet ne fait pas.
* **Corriger `v1` sur place.** Interdit par ADR 0042 §1 : le rendu d'une version
  publiée ne bouge pas. C'est aussi ce qui rend le présent ADR peu coûteux.
* **Attendre les profils de bruit mesurés** pour ne livrer qu'une fois. Le
  seuillage uniforme est déjà très au-dessus d'un flou, et rien dans les profils
  n'invaliderait la transformée : ils changeraient les seuils, donc une `v3`
  plus tard, non un retour en arrière.
