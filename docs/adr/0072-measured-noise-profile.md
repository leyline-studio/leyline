# ADR 0072 — Le débruitage sait enfin de quel capteur il vient (`noise_luminance::v3`, `noise_color::v3`)

**Statut :** Accepté — 2026-08

## Contexte

[ADR 0046](0046-edge-preserving-denoise.md) a remplacé un flou par un vrai
opérateur — ondelettes à trous, seuillage doux — et a énoncé, dans son §7, ce
qu'il ne faisait pas :

> **Pas de profil de bruit par boîtier et par ISO.** Le seuil est un modèle de
> bruit blanc uniforme, pas la variance mesurée du capteur à cette sensibilité.

C'est l'item **A3** de [`measured-findings.md`](../measured-findings.md), le
dernier de l'axe « justesse du rendu » resté ouvert. Le présent ADR le tranche.

### Ce que « bruit blanc uniforme » coûte

Le seuil de `v2` vaut `k · BASE · σ_l` : trois constantes, les mêmes pour tous
les fichiers du monde. Or le bruit d'un capteur n'est ni uniforme ni constant,
et il varie sur **deux axes** que ce seuil ignore tous les deux.

**La sensibilité.** Entre ISO 100 et ISO 12800, l'écart-type du bruit d'un
Canon 60D à mi-gris passe de 0,0015 à 0,0131 — un facteur **neuf**. Un seuil
unique ne peut donc être juste qu'à une sensibilité : il détruit du détail pour
rien en dessous, et n'atteint pas le bruit au-dessus. Concrètement, « 30 » sur
le curseur ne veut pas dire la même chose d'une photo à l'autre, et
l'utilisateur passe son temps à le redécouvrir fichier par fichier.

**Le niveau du signal.** Le bruit de photons est poissonien : sa variance
**croît avec la lumière reçue**, et son écart-type *relatif* décroît. Un seuil
constant est donc trop faible dans les ombres — là où le bruit se voit — et
trop fort dans les hautes lumières, où il n'y a presque rien à retirer et où il
mange de la texture.

### Ce que le bruit est réellement

Le modèle standard, celui que mesurent darktable, DxO et la littérature
(Foi & al.), est **poissonien-gaussien** : pour une valeur brute `x` normalisée
sur `[0, 1]`,

```
var(x) = a · x + b
```

`a` porte le bruit de photons (proportionnel au signal, et proportionnel à la
sensibilité), `b` le bruit de lecture (constant, indépendant du signal). Deux
nombres par canal et par sensibilité suffisent à décrire un capteur — c'est
peu, et c'est mesurable.

## Décision

### 1. Les mesures viennent de la base de darktable, sous sa licence

Leyline embarque la table `data/noiseprofiles.json` du projet darktable :
**434 boîtiers**, 7 842 couples `(ISO, a, b)` par canal, mesurés un par un par
les contributeurs de ce projet depuis 2014.

**La licence permet exactement cela.** darktable est publié sous
**GPL-3.0-or-later**, Leyline sous [GPL-3.0-only](../../LICENSE) : un travail
sous « v3 ou ultérieure » entre sans difficulté dans un travail sous « v3 »,
c'est le sens même de la clause. L'attribution et la licence d'origine sont
consignées dans l'en-tête du fichier embarqué et dans
[`architecture.md`](../architecture.md) §Briques externes, au même titre que
Lensfun ou LittleCMS.

**Ce que nous ne pouvons pas faire nous-mêmes**, et c'est la raison de fond :
mesurer un profil demande une série de prises de vue contrôlées **par boîtier
et par sensibilité**. Le corpus de développement couvre deux boîtiers ; la
base en couvre 434. Un développeur RAW dont le débruitage ne serait profilé
que pour les deux appareils de son auteur ne serait pas un développeur RAW.
Mesurer reste possible — le format de la table est celui de l'amont, et un
boîtier absent s'y ajoute avec les mêmes deux nombres par canal et par
sensibilité — mais c'est un complément, pas la base.

**La table est réduite, pas transformée.** Les champs `name` et `comment` sont
retirés, les nombres arrondis à six chiffres significatifs (le tampon est en
`f32`, qui en porte sept), et les doublons d'ISO — plusieurs contributeurs
ayant mesuré le même boîtier — sont résolus par **la première entrée dans
l'ordre du fichier amont**, pour que la règle soit une règle et non un hasard
d'itération. Le fichier garde une ligne par boîtier, ce qui le rend
diffable contre l'amont, et son en-tête épingle le commit d'origine
(`e333310b`, 2026-08-03) : la provenance est vérifiable, pas déclarée.

### 2. La table est gelée avec la version d'étage

C'est le point qui décide de tout le reste. Une table qui évoluerait sous une
version d'étage publiée changerait le rendu d'une révision existante — soit
exactement ce que [`pipeline.md`](../pipeline.md) §5.1 interdit.

Donc : **la table appartient à la version**. `kernel::v3` l'incorpore par
`include_str!("../../../data/noise_profiles_v1.json")`, au même titre qu'une
constante. Mettre les mesures à jour, ou ajouter des boîtiers, produit un
`noise_profiles_v2.json` **et** de nouvelles versions d'étages : jamais une
modification du fichier existant. Le coût est celui d'un fichier de 774 Ko de
plus par mise à jour, et il est assumé — c'est le prix exact de la promesse.

**Par contraste, ce que cette décision met en lumière.** La base Lensfun est
embarquée dans le binaire de la même façon (ADR 0016) et **n'est épinglée par
rien** : le jour où la version embarquée changera, une révision citant
`lens::v1` rendra autrement. C'est un trou réel dans §5.1, découvert en
écrivant ce document, hors de son périmètre, et consigné ici pour qu'il ne se
redécouvre pas une troisième fois.

### 3. Le seuil devient un seuil par pixel

L'opérateur d'ADR 0046 ne change pas — décomposition à trous, seuillage doux,
résidu jamais seuillé. Ce qui change est le seuil, qui cesse d'être un nombre
pour devenir une fonction du pixel :

```
t_l(i) = k · SIGMAS · σ_l · √( max(a · L_i + b, 0) )
k = strength / 100
```

`L_i` est le plan de luma, calculé **une fois** avant la décomposition : c'est
l'estimateur du signal, et il n'a pas besoin d'être meilleur que ça — une
erreur de ±σ sur `L` déplace `√(a·L + b)` de bien moins que le rapport de 9
entre deux sensibilités. `σ_l` reste le profil par échelle d'ADR 0046 §3
(`0.890, 0.201, 0.086, 0.041`), qui décrit comment la transformée répartit un
bruit blanc entre ses niveaux — le modèle mesuré dit *combien* de bruit il y
a, le profil par échelle dit *où* il va.

`b` peut être négatif dans la base (artefact d'ajustement sur certains
boîtiers, le 60D en donne à toutes ses sensibilités) : la variance est donc
bornée à zéro avant la racine, ce qui rend simplement le modèle purement
poissonien là où l'ajustement l'a voulu ainsi.

**Les deux constantes.** `SIGMAS_LUMA = 6` place le curseur à mi-course
(`strength = 50`) sur **3 σ**, la valeur de manuel pour un seuillage doux, et
lui laisse de quoi aller au double. `SIGMAS_CHROMA = 10` reste plus agressif,
pour la raison qui valait déjà dans `v2` — la chrominance d'une photo est
lisse presque partout — mais **pas** dans le rapport 2,5 que `v2` exprimait
par ses `BASE` (0,05 et 0,12) : une partie de ce rapport est désormais portée
par le σ mesuré lui-même, qui ressort environ 1,7 fois plus grand sur un plan
de chrominance que sur la luma (`chroma_terms` §5). Le compter deux fois
aplatirait de la vraie couleur. Ces deux constantes sont gelées au même titre
que la table.

**Sur une preview réduite** (ADR 0041), le bruit a déjà été moyenné par la
réduction : `n × n` pixels moyennés divisent son écart-type par `n`. Le σ
mesuré est donc multiplié par le facteur `scale` avant de servir de seuil, ce
qui est la seule façon pour l'aperçu et l'export de montrer le même
débruitage. Le nombre de niveaux continue de suivre `levels_at_scale`
(ADR 0046 §5) ; les deux corrections sont indépendantes et toutes deux
nécessaires.

### 4. Les étages remontent en tête du pipeline (rangs 5 et 6)

Un modèle mesuré sur les nombres du capteur ne veut plus rien dire une fois
que l'exposition, le contraste, la courbe tonale et la clarté sont passés.
Aux rangs 170 et 180, `v2` travaillait sur une image dont plus rien ne
reliait la valeur d'un pixel à la lumière reçue par la photosite. Un profil y
serait une décoration.

`noise_luminance::v3` prend donc le **rang 5** et `noise_color::v3` le
**rang 6** — entre `input` (rang 0) et `camera_profile` (rang 10), le seul
endroit du pipeline où le tampon est encore une transformation **linéaire**
des comptes du capteur. Le rang est une propriété de la version, pas de
l'opérateur ([ADR 0042](0042-versioned-stage-pipeline.md) §3) : c'est
précisément ce mécanisme qui rend ce déplacement possible sans toucher à
`v1` ni `v2`, qui restent aux rangs 170 et 180 pour les révisions qui les
citent.

Trois conséquences, dans l'ordre où elles comptent :

* **Le débruitage précède la géométrie** (`lens` au rang 20 rééchantillonne,
  `rotate` et `perspective` aussi). C'est la bonne place : après un
  rééchantillonnage, le bruit n'est plus indépendant d'un pixel à l'autre et
  aucun modèle par pixel ne le décrit plus.
* **L'axe d'affichage est abandonné.** `v1` et `v2` travaillaient sous
  `in_display` (ADR 0046 §4) parce qu'un seuil constant en lumière linéaire
  serait énorme dans les ombres et négligeable dans les hautes lumières. Le
  seuil n'est plus constant : il suit le signal, ce que la courbe d'affichage
  ne faisait qu'approximer. Deux non-linéarités de moins par rendu, et le
  modèle appliqué là où il a été mesuré.
* **Le cache d'étages change de main.** Le curseur de bruit était le
  quatrième avant la fin ; il devient le second après le début. Bouger *ce*
  curseur-là rejoue désormais tout le pipeline, pendant que **tous les autres
  curseurs** trouvent le débruitage — l'étage le plus cher du pipeline — déjà
  fait dans le cache. Aucun point de contrôle n'est à ajouter pour cela : le
  premier de ceux d'ADR 0041 §3 est pris *avant* le rang 40, donc après les
  rangs 5 et 6, et il capture donc déjà le tampon débruité.

### 5. Transporter le modèle jusqu'à l'espace du tampon

Les coefficients de la base sont mesurés sur les valeurs brutes du capteur,
canal par canal, **avant balance des blancs**. Le tampon du rang 5 n'est pas
cet espace-là : il en est une transformation linéaire, connue, en deux temps.

**La balance des blancs du boîtier.** LibRaw multiplie le canal `j` par
`g_j = m_j / min(m)`, où `m` sont les multiplicateurs de la prise de vue
(`SourceColor::Camera::multipliers`). Multiplier un échantillon par `g` en
multiplie la variance par `g²`, donc :

```
a_j ← g_j · a_j        b_j ← g_j² · b_j
```

(La forme de `a` suit du changement de variable : `var(g·x) = g²(a·x + b)` et
`x = y/g` donnent `g·a·y + g²·b`. C'est le calcul que darktable fait en
divisant le pixel par `wb` avant d'évaluer son modèle.) Le mode de
reconstruction des hautes lumières ne change rien à ce gain : LibRaw y divise
par `max(m)` au lieu de `min(m)`, et `input::v2` rend exactement ce rapport
([ADR 0050](0050-highlight-reconstruction.md) §3).

**La matrice colorimétrique.** Sans profil DCP, `input` a déjà appliqué
`M = camera_to_rec2020` ; avec un profil, le tampon est encore camera-natif
et `camera_profile` (rang 10) fera la conversion plus tard. Dans le premier
cas, une combinaison linéaire de variables indépendantes donne
`var(y_k) = Σ_j M_kj² · var(x_j)`, d'où, sous l'hypothèse de gris (`x_j ≈ y_k`,
que la normalisation de `M` rend cohérente sur un neutre) :

```
a'_k = Σ_j M_kj² · a_j      b'_k = Σ_j M_kj² · b_j
```

Dans le second, la transformation est l'identité et le modèle s'applique tel
quel. L'étage sait dans lequel des deux cas il est — c'est ce que
`ctx.camera_profile.is_some()` dit, et `input::v2` prend déjà sa décision sur
ce même booléen.

**De là aux deux plans traités.** La luma est `Σ w_k y_k` avec les poids
Rec. 2020 de `pixels::luma`, donc `a_L = Σ w_k² a'_k`. La chrominance du canal
`k` est `y_k − L`, donc `a_C,k = (1 − w_k)² a'_k + Σ_{j≠k} w_j² a'_j`. Les
mêmes formules sur `b`. Rien n'y est ajusté à la main : chaque coefficient
descend de la définition du plan qu'il décrit. Ces poids sont ceux de
`pixels::luma`, donc ceux du Rec. 2020 : dans le cas « profil DCP », où le
tampon est encore camera-natif, ils sont appliqués à des canaux qui ne sont
pas les leurs. C'est l'opérateur lui-même qui fait déjà ce choix — `v1` et
`v2` extraient la même luma — et l'erreur qui en résulte sur un **seuil** est
sans commune mesure avec ce que le profil apporte.

**Ce qui n'est pas transporté, et pourquoi.** Le niveau de blanc. Nos valeurs
sont normalisées par la marge de linéarité du boîtier
([ADR 0066](0066-sensor-white-level.md)), celles de la base par la constante
de blanc de darktable — un rapport de l'ordre de 1,1 sur un 60D, donc ~10 %
sur σ. C'est dérisoire devant le facteur 100 que couvre l'échelle des
sensibilités, et devant l'incertitude de l'ajustement lui-même. C'est dit ici
pour que personne n'ait à le déduire d'un silence.

### 6. La correspondance boîtier, et l'interpolation en sensibilité

La recherche prend la marque, le modèle et la sensibilité EXIF de la prise de
vue. Le catalogue **retire déjà la marque du modèle** (`exif::without_brand`,
et LibRaw le fait de son côté) : `Canon` / `EOS 60D`, ce qui est exactement la
forme de la base. La comparaison est faite à la casse et aux espaces près, et
rien de plus — une correspondance approximative sur un nom de boîtier
donnerait le profil d'un autre capteur, ce qui est pire que pas de profil.

**Entre deux sensibilités mesurées, les coefficients sont interpolés
linéairement** en ISO, et bornés aux extrémités de l'échelle. La base est
dense (le 5D Mark IV y a 29 entrées de 50 à 102 400) : l'interpolation ne fait
que combler des tiers de tiers de diaphragme, jamais un trou.

**Les sources RAW seulement.** Un JPEG a déjà traversé le débruitage du
boîtier et sa courbe : le modèle n'y décrit plus rien. `SourceColor::Srgb`
tombe donc dans le repli du §7, comme un boîtier inconnu.

**Vérifié sur de vrais fichiers, et pas seulement sur des chaînes écrites à la
main.** Une correspondance qui échoue ne casse rien : elle bascule sur le
repli, en silence — exactement le défaut qu'ADR 0035 avait laissé passer en ne
lisant que ses propres fixtures. Un test ignoré par défaut
(`LEYLINE_TEST_RAW`) part donc d'un CR2 du corpus, en tire les métadonnées par
le chemin d'import réel et **exige** que la table réponde. Les deux boîtiers
disponibles y passent : Canon EOS 60D et Canon EOS 5D Mark IV.

### 7. Sans correspondance, un modèle par défaut — pas un étage muet

L'étage objectif, faute de profil, ne corrige rien (ADR 0016 §2). Ce n'est pas
transposable ici : un objectif inconnu veut dire « aucune correction n'a été
demandée », alors qu'un boîtier inconnu ferait perdre **le débruitage
lui-même** à qui possède un appareil absent de la base — et la version
épinglée pour toute nouvelle révision est `v3`.

Le repli est donc un modèle, explicite : `a = 0`, `b = σ₀²` avec
**σ₀ = 0,003** — un bruit indépendant du signal, de l'ordre de ce que la base
donne pour un reflex APS-C autour d'ISO 800. C'est exactement l'hypothèse de
`v2`, replacée en lumière linéaire : le boîtier inconnu retrouve le
comportement d'avant cet ADR, ni plus ni moins. La constante est définie dans
les unités du tampon et n'est **pas** transportée par le §5 : transporter un
nombre inventé ne le rendrait pas plus vrai.

### 8. Ce que cette décision ne change pas

* **La surface de réglage.** Deux curseurs `0..100`
  (`noise_reduction.luminance`, `noise_reduction.color`), `settings_json`
  inchangé, `schema` non incrémenté. Aucun client — Studio, CLI, SDK — n'a un
  champ à ajouter. Comme pour ADR 0046, ce sont les mêmes valeurs, mieux
  dépensées.
* **Les révisions existantes.** `v1` et `v2` ne sont pas touchées et restent
  enregistrées à leurs rangs. Une révision qui les cite rend à l'identique,
  pour toujours ; l'utilisateur qui veut le débruitage profilé retraite sa
  photo, ce qui crée une nouvelle révision.
* **La révision n'apprend rien du capteur.** Marque, modèle et ISO sont des
  propriétés **du fichier**, pas de l'intention de l'utilisateur : ils entrent
  par le même chemin que `SourceColor` et que le `LensShot`, c'est-à-dire un
  argument de `render`, et non par `settings_json`. Un préréglage reste donc
  applicable d'un boîtier à l'autre — et c'est la raison décisive de ne pas
  matérialiser les coefficients dans la révision.

## Conséquences

* **Le curseur veut enfin dire quelque chose.** À réglage égal, une photo
  ISO 100 est à peine touchée et une photo ISO 6400 est franchement
  débruitée : c'est le capteur qui porte l'écart, plus l'utilisateur.
* **Le fichier de mesures pèse 774 Ko dans le binaire**, chargé et analysé une
  seule fois par processus, à la première utilisation de l'étage
  (`OnceLock`) — jamais à l'ouverture de l'application.
* **Le seuil mesuré ne coûte rien.** Le banc `denoise/` compare les trois
  versions épinglées, à réglages identiques (luminance 40, chroma 30) sur la
  trame synthétique de 3 Mpx : **98 ms pour `v1`, 231 ms pour `v2`, 219 ms
  pour `v3`** — un rendu complet, pas l'opérateur seul. `v3` est donc
  légèrement *plus rapide* que `v2` : une racine carrée par pixel et un plan
  de σ de plus, contre les deux passes d'aller-retour vers l'axe d'affichage
  que le §4 supprime, `display()` et `linear()` appelant `powf` sur chaque
  échantillon. L'échange est favorable, ce qui n'était pas prévu.
* **Le manifeste golden gagne des entrées, aucune ne bouge.** Les variantes
  `noise_*::v3` s'y ajoutent, dont une exerçant la correspondance réelle
  (Canon EOS 60D à ISO 3200) et une le repli. Le blessing reste additif.
* **`render` prend un argument de plus** (`sensor: Option<&SensorShot>`), et
  [`engine-api.md`](../engine-api.md) le documente. C'est le huitième argument
  d'une fonction qui en portait sept : le regroupement de ces entrées résolues
  dans une structure unique est une simplification à faire, et elle n'a pas sa
  place dans le même changement que celui-ci.
* **`docs/pipeline.md` §3.3 gagne deux lignes**, et l'ordre du tableau des
  étages cesse d'être l'ordre de leur numérotation historique : deux étages y
  figurent maintenant deux fois, à deux rangs différents. C'est la première
  fois qu'une version change de rang, et le mécanisme d'ADR 0042 §3 sert donc
  pour ce qu'il a été écrit.
* **A3 est fermé** dans [`measured-findings.md`](../measured-findings.md), et
  l'argument de C1 (« A3 donne une partie du gain visé par le débruitage IA,
  sans aucune de ses questions ») devient vérifiable.

## Alternatives écartées

* **Mesurer nous-mêmes la base.** Deux boîtiers contre 434, pour des mois de
  prises de vue contrôlées et un protocole à valider. Le protocole reste
  utile — il est le moyen d'ajouter un boîtier absent — mais il ne peut pas
  *être* la base.
* **La transformée stabilisatrice de variance (Anscombe généralisé)**, ce que
  darktable applique. Plus juste en théorie : elle rend le bruit uniforme, ce
  qui autorise un seuil constant et une analyse propre. En pratique elle
  ajoute deux transformations non linéaires par rendu et un biais à corriger
  à l'inverse, pour un écart au seuil par pixel qui reste petit devant
  l'incertitude de §5 sur le niveau de blanc. Le seuil par pixel garde en
  outre l'opérateur d'ADR 0046 littéralement intact, ce qui rend cette version
  lisible à côté de la précédente.
* **Matérialiser les coefficients dans la révision.** Rendrait le rendu pur à
  partir de `settings_json` — et rendrait un préréglage dépendant du boîtier
  sur lequel il a été créé, ce qui casserait
  [`presets.md`](../presets.md) pour un gain nul : le fichier est de toute
  façon nécessaire au rendu, et il porte son propre EXIF.
* **Garder les rangs 170 et 180.** Aurait évité tout déplacement — et appliqué
  un modèle mesuré sur des comptes de capteur à une image passée par
  l'exposition, la courbe tonale et la clarté. Un profil correct au mauvais
  endroit est un profil faux.
* **Une table maison à la `camconst`**, mesurée boîtier par boîtier comme
  RawTherapee le fait pour les niveaux de blanc. Même impasse que la première
  alternative, avec en plus une base à maintenir seul.
* **Corriger `v2` sur place.** Interdit par ADR 0042 §1, et c'est aussi ce qui
  rend le présent ADR peu coûteux.
