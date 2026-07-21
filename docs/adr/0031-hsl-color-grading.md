# ADR 0031 — Mélangeur TSL et roues de color grading : HSL dérivé du RGB, zones tonales pondérées par luminance

**Statut :** Accepté — 2026-07

## Contexte

`docs/v2-scope.md` §4 (« Color grading / mélangeur TSL ») relève deux manques
liés, absents aujourd'hui alors que seules vibrance et saturation **globales**
existent (`crates/leyline-engine/src/process2.rs:298`) :

* le **mélangeur TSL par teinte** — 8 bandes teinte/saturation/luminance, à la
  manière du panneau TSL de Lightroom ou du mélangeur de Darktable ;
* les **roues de color grading** ombres/tons moyens/hautes lumières (couleur +
  luminance par zone), à la manière du *color balance rgb* de Darktable ou du
  panneau *color grading* de Lightroom.

Le §4 esquisse les champs `hsl` (8 bandes × `{hue, saturation, luminance}`) et
`color_grading.shadows/.midtones/.highlights` plus `.blending`/`.balance`, et
laisse deux questions ouvertes : le **modèle de teinte** à geler dans le
contrat de rendu (`docs/pipeline.md` §5) et le **color grading régional**
(sous masque). Le §9 confirme l'éligibilité d'un ADR propre (« nouveau
process, modèle de teinte gelé »).

Trois décisions transversales sont **consommées, non re-litigées, ici** :

* **ADR 0028** fige la stratégie de versionnage : une process version par
  fonctionnalité pixel, chacune dans son propre module `processN.rs` gelé,
  créé en copiant le module précédent entier.
* **ADR 0027** confirme que l'espace de travail interne du rendu reste sRGB
  gamma-encodé entre opérateurs (`process2.rs:30`) — l'espace où ces
  opérateurs couleur sont définis.
* **ADR 0029** a introduit le masquage spatial (couverture `[0,1]` par pixel,
  process 6) ; le présent document s'en distingue explicitement (voir ci-dessous)
  et laisse le color grading régional à un futur ADR adossé à cette
  infrastructure.

## Décision

Le mélangeur TSL et les roues de color grading sont des opérateurs pixel : ils
prennent une **nouvelle process version**, **le prochain numéro de process
disponible au moment de la sortie de cette fonctionnalité** (ADR 0028), dans
son propre module `processN.rs` copie intégrale du module précédent augmentée
des seuls opérateurs couleur nouveaux. Cet ADR **ne fige pas** un entier de
process précis : l'ordre de sortie des items 3/4/5/6/8 relève du plan
d'implémentation futur, pas de ce document.

Cet ADR **conçoit les deux ensemble** — TSL et color grading — parce que
`docs/v2-scope.md` §4 les scope comme un seul item et parce qu'ils partagent
la même mathématique de teinte/luminance dérivée du RGB de travail. Il ne les
**force pas** à sortir ensemble (voir *Alternatives écartées*) : chacun peut,
si l'implémenteur le préfère, prendre son propre numéro de process au moment
où il est prêt (ADR 0028) — cet ADR les conçoit conjointement, il ne couple pas
leur calendrier de livraison.

### Mélangeur TSL — HSL dérivé du RGB de travail

Le mélangeur opère en **HSL standard dérivé du tampon RGB de travail** (sRGB
gamma-encodé, `process2.rs:30`), **pas** dans un espace perceptuel/CIE. **8
bandes de teinte fixes** — rouge, orange, jaune, vert, aqua, bleu, violet,
magenta — chacune définie par un angle de teinte central, avec une **fonction
de falloff** entre bandes adjacentes de sorte qu'un pixel dont la teinte tombe
entre deux centres reçoit une contribution pondérée des deux. Ce modèle est
exactement la convention Lightroom/Darktable dont cette fonctionnalité vise la
parité.

Ce qui est **gelé ici**, c'est le **choix de modèle** : HSL dérivé du RGB,
8 bandes à centres fixes, falloff entre bandes adjacentes. Les **constantes
numériques exactes** — angles de centre de bande, forme précise de la courbe
de falloff — relèvent de la PR d'implémentation, au même niveau de précision
que la courbe tonale (ADR 0030) et la mathématique interne de Lensfun
(ADR 0016). Figer le modèle suffit à garantir la reproductibilité une fois la
process version publiée (`docs/pipeline.md` §5).

Chaque bande porte trois curseurs `{hue, saturation, luminance}` dans
`[-100, +100]`, réutilisant le vocabulaire d'unité des curseurs existants
(`process2.rs:236`) : `hue` décale la teinte des pixels de la bande, `saturation`
et `luminance` en modulent chroma et clarté — la même famille d'opération que
`saturate` existant (`process2.rs:298`), re-paramétrée par bande de teinte.

### Roues de color grading — zones tonales pondérées par luminance

Les trois zones — ombres / tons moyens / hautes lumières — sont séparées par
une **fonction de pondération basée sur la luminance** par pixel (la luminance
Rec. 709 déjà utilisée dans le moteur, `crates/leyline-engine/src/pixels.rs:110`) :
des masques de zone de type *smoothstep* (la maison emploie déjà `x²(3 − 2x)`,
`process2.rs:242`) qui, pour chaque pixel, répartissent son appartenance entre
les trois zones selon sa valeur de luminance. Deux contrôles règlent cette
répartition :

* **`balance`** — point de bascule qui déplace la frontière ombres↔hautes
  lumières, décidant quelle plage de luminances compte comme « tons moyens » ;
* **`blending`** — largeur de recouvrement des zones, contrôlant la douceur des
  transitions entre zones adjacentes.

Chaque zone porte `{hue, saturation, luminance}` : la couleur choisie (teinte +
saturation) est mélangée dans les pixels de la zone au prorata de leur poids de
zone, et `luminance` en règle la clarté.

**C'est un fondu par zone tonale, pas un masquage spatial.** Ce point est
souligné pour qu'il ne soit **pas confondu** avec le mécanisme de masque
spatial d'ADR 0029 : le color grading pondère par **valeur de luminance** du
pixel (un pixel sombre est « une ombre » où qu'il soit dans le cadre), tandis
qu'un masque d'ADR 0029 pondère par **position spatiale** du pixel (couverture
`[0,1]` remontée au tampon pré-rotation par ADR 0026). Ce sont **deux mécanismes
orthogonaux opérant sur des axes différents** — valeur tonale contre position
spatiale — et le color grading n'emprunte **rien** à l'infrastructure de
masquage.

### Place dans le pipeline

Les deux étages s'insèrent dans le **bloc couleur, après Vibrance/Saturation**
— ce que la ligne « Pipeline (§3.1) » du tableau `docs/v2-scope.md` §4 fixe
déjà. Cet ADR **confirme et consomme** ce placement, il ne le re-dérive pas :
TSL et color grading affinent la couleur une fois la saturation globale
appliquée.

### Color grading régional — hors périmètre V2

Appliquer le TSL ou le color grading **sous masque** (les combiner avec
l'infrastructure spatiale d'ADR 0029) est **explicitement hors périmètre de la
V2** : c'est une extension naturelle une fois que cette fonctionnalité et le
masquage existent tous deux, déférée à un futur ADR — pas conçue ici.

### Stockage — schéma additif

`hsl` (8 bandes × `{hue, saturation, luminance}`) et
`color_grading.shadows/.midtones/.highlights` (`{hue, saturation, luminance}`
par zone) plus `color_grading.blending`/`.balance` sont des champs **additifs**
de `settings_json`. **Absents = neutres** (0 partout : aucun décalage de
teinte, aucune coloration de zone), rendu **bit-pour-bit identique** à la
process version précédente — l'invariant « *a parameter at its neutral value
skips its operator entirely, so the neutral rendering is bit-for-bit the
decoded image* » (`process3.rs:25`). Aucun bump de schéma requis, cohérent avec
« process +1, schema inchangé » (`docs/v2-scope.md` §1) ; les champs inconnus
d'un moteur ancien sont préservés verbatim (`Settings::extra`,
`crates/leyline-core/src/settings.rs`).

### Esquisse JSON

Le style suit `docs/pipeline.md` §3.2. Un exemple non neutre puis le cas neutre :

```json
{
    "schema": 1,
    "process": 8,

    "vibrance": 12,
    "hsl": [
        { "hue": 0,   "saturation": -20, "luminance": 0 },
        { "hue": 5,   "saturation": 0,   "luminance": 0 },
        { "hue": 0,   "saturation": 0,   "luminance": 0 },
        { "hue": -10, "saturation": 15,  "luminance": 8 },
        { "hue": 0,   "saturation": 0,   "luminance": 0 },
        { "hue": 8,   "saturation": 25,  "luminance": 0 },
        { "hue": 0,   "saturation": 0,   "luminance": 0 },
        { "hue": 0,   "saturation": 0,   "luminance": 0 }
    ],
    "color_grading": {
        "shadows":    { "hue": 220, "saturation": 15, "luminance": 0 },
        "midtones":   { "hue": 0,   "saturation": 0,  "luminance": 0 },
        "highlights": { "hue": 45,  "saturation": 10, "luminance": 0 },
        "balance": 0,
        "blending": 50
    }
}
```

Cas neutre — champs absents, rendu bit-pour-bit identique à la process version
précédente :

```json
{
    "schema": 1,
    "process": 8,
    "vibrance": 12
}
```

> *Le `process: 8` ci-dessus est purement illustratif : le numéro réel est le
> prochain disponible au moment de la sortie (ADR 0028), pas fixé par cet ADR.
> Les 8 entrées `hsl` suivent l'ordre fixe des bandes
> rouge/orange/jaune/vert/aqua/bleu/violet/magenta.*

> **Note d'implémentation (pas une édition de spec ici).** Cet ADR ne modifie
> **pas** le diagramme de `docs/pipeline.md` §3.1 ni le tableau des process
> versions §3.3. Comme pour ADR 0029 et ADR 0030, la spec est mise à jour dans
> le même changement que l'implémentation réelle, conformément à CLAUDE.md. Le
> présent document fixe **où** les étages atterrissent et **quels** modèles ils
> gèlent ; le diagramme §3.1 et le tableau §3.3 seront amendés par la PR qui
> livre le module process correspondant.

## Conséquences

* **Le champ `process` garde sa lisibilité sémantique** (ADR 0028) : le
  nouveau numéro signifiera « mélangeur TSL + color grading actifs », un fait
  lisible comme `process: 3` signifie « correction de distorsion active ».
* **Sortie neutre gelée** : sans réglage, les deux étages sont bit-pour-bit la
  process version précédente (`process3.rs:25`).
* **Aucun couplage à l'infrastructure de masquage** : le color grading pondère
  par luminance, orthogonalement au masque spatial d'ADR 0029 — les deux
  peuvent évoluer indépendamment, et le color grading livre sans attendre le
  masquage.
* **Le color grading régional reste ouvert** pour un futur ADR adossé à
  ADR 0029, sans bloquer la version globale livrée ici.
* **TSL et color grading peuvent, si voulu, sortir séparément** (ADR 0028,
  chacun son numéro de process) bien qu'ils soient conçus ensemble ici : cet
  ADR décrit la mathématique, il ne couple pas le calendrier de livraison.
* **Le contrat de reproductibilité** (`docs/pipeline.md` §5) est respecté :
  modèle de teinte et pondération de zone figés par la process version,
  opérateurs purs et déterministes (ADR 0012), tout dans `settings_json`.
* **Un module `processN.rs` de plus** (ADR 0028) : aucun module antérieur
  touché, le gel « mêmes pixels dans dix ans » reste infalsifiable (§3.3).
* **La spec `docs/pipeline.md` (§3.1, §3.3) n'est pas éditée par cet ADR** :
  elle le sera par la PR d'implémentation, conformément à CLAUDE.md.

## Alternatives écartées

* **Un espace de teinte perceptuel/CIE (LCh, OKLCh…) plutôt que le HSL
  dérivé du RGB.** Écarté : un espace perceptuel donnerait des transitions de
  teinte plus régulières, mais il romprait la parité avec la convention
  Lightroom/Darktable que cette fonctionnalité vise (les bandes et les résultats
  attendus des utilisateurs sont définis en HSL RGB), imposerait une conversion
  aller-retour hors de l'espace de travail sRGB gelé (ADR 0027), et alourdirait
  le contrat gelé pour un bénéfice que le manque réel — parité mélangeur TSL —
  ne réclame pas. Le HSL dérivé du RGB de travail est le modèle de parité et le
  plus proche de l'espace existant.
* **Le masquage spatial (ADR 0029) comme mécanisme de sélection de zone** pour
  le color grading, au lieu d'une pondération par luminance. Écarté : ce serait
  confondre deux axes orthogonaux. Une « ombre » en color grading est une
  **valeur de luminance basse**, où qu'elle soit dans le cadre — pas une région
  spatiale. Utiliser un masque spatial obligerait l'utilisateur à peindre les
  zones tonales à la main, ce qui n'est ni l'ergonomie visée ni la sémantique
  de l'outil. La pondération par luminance (smoothstep sur la luma Rec. 709,
  `pixels.rs:110`, `process2.rs:242`) sélectionne les zones automatiquement par
  valeur, sans géométrie.
* **Livrer TSL et color grading comme deux ADR/process versions séparés** au
  lieu d'un design combiné. Écarté **comme choix de conception**, pas comme
  contrainte de livraison : `docs/v2-scope.md` §4 les scope comme un seul item
  et ils partagent la mathématique teinte/luminance dérivée du RGB — les
  concevoir ensemble évite deux ADR qui re-dérivent le même modèle de teinte.
  Cet ADR laisse néanmoins l'implémenteur libre de leur donner deux numéros de
  process distincts au moment de la livraison (ADR 0028) : le design est
  commun, le calendrier ne l'est pas.
* **Supporter le color grading régional (sous masque) en V2.** Écarté :
  déféré à un futur ADR adossé à l'infrastructure de masquage d'ADR 0029. La
  version globale (par zone tonale) est autonome et livrable sans le masquage ;
  la version régionale s'y adossera naturellement le moment venu, dans le
  référentiel commun d'ADR 0026, sans être recodée.
