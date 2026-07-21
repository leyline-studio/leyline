# ADR 0033 — Clarté, texture, dehaze : contraste local unifié à deux rayons et dehaze par dark channel prior en forme close

**Statut :** Accepté — 2026-07

## Contexte

`docs/v2-scope.md` §6 (« Dehaze / texture / clarté ») relève que seul « détail »
existe aujourd'hui (réduction de bruit + netteté, `process2.rs:324`, `:357`), sans
contrôles séparés de **clarté**, **texture** et **dehaze** — trois traitements de
contraste local/fréquentiel distincts qu'attend tout développeur RAW en parité
Lightroom/Darktable/Capture One.

Trois décisions transversales sont **consommées, non re-litigées, ici** :

* **ADR 0028** fige la stratégie de versionnage : une process version par
  fonctionnalité pixel, chacune dans son propre module `processN.rs` gelé, créé
  en copiant le module précédent entier. Ces trois curseurs sont des opérateurs
  pixel : ils prennent une nouvelle process version, sans que cet ADR ait à
  re-choisir la convention.
* **ADR 0027** confirme que l'espace de travail interne du rendu reste sRGB
  gamma-encodé entre opérateurs (`process2.rs:30`) — l'espace où le bloc
  présence/détail existant est défini et où ces trois étages s'insèrent.
* **ADR 0029** a introduit le masquage spatial (couverture `[0,1]` par pixel) ;
  le présent document livre les trois curseurs **en global** et défère leur
  version masquée à un futur ADR adossé à cette infrastructure (voir ci-dessous).

Le §6 laissait trois questions ouvertes propres à l'item : le déterminisme du
dehaze, l'ordre « global d'abord, masqué ensuite », et le coût CPU du contraste
local multi-échelle sur les grandes previews (`docs/engine-api.md` §11). Ce
document les tranche en même temps qu'il fixe la mathématique, le placement et le
stockage.

## Décision

Clarté, texture et dehaze sont des opérateurs pixel : ils prennent une **nouvelle
process version**, **le prochain numéro de process disponible au moment de la
sortie de cette fonctionnalité** (ADR 0028), dans son propre module `processN.rs`
copie intégrale du module précédent augmentée des seuls étages nouveaux. Cet ADR
**ne fige pas** un entier de process précis.

Le périmètre est **exactement trois curseurs indépendants** — `clarity`,
`texture`, `dehaze` — aucun curseur supplémentaire n'est inventé.

### Clarté et texture — un seul contraste local paramétré, deux rayons

Clarté et texture appartiennent à **une seule famille d'algorithme** : le
contraste local par masque flou (*unsharp mask*), c'est-à-dire l'amplification de
la différence entre un pixel et une version passe-bas (floutée) de lui-même —
exactement la forme du `sharpen` existant `L' = L + amount·(L − blur(L, σ))`
(`process2.rs:357`), mais à plus grand rayon et sans être bornée au détail fin.
Elles se distinguent **par le rayon du flou** :

* **clarté** — **grand rayon** : contraste local large, le « look clarté »
  classique qui donne de la présence aux tons moyens ;
* **texture** — **petit rayon** : contraste local fin, le micro-détail.

**Décision : les deux sont implémentées comme une seule fonction interne de
contraste local paramétrée**, appelée **deux fois** avec des constantes de
rayon/force différentes — **pas** deux algorithmes inventés indépendamment. C'est
de la **réutilisation de code *intra-version*, à l'intérieur d'un même module
process**, et c'est explicitement permis : **ADR 0028 n'interdit que le partage
de code *entre* modules de process versions gelés** (le risque qu'un correctif
altère silencieusement un rendu figé antérieur), **pas** la factorisation à
l'intérieur d'un seul module. Ce point est énoncé explicitement parce qu'il
pourrait sinon sembler contredire ADR 0028 : il ne le contredit pas — la fonction
partagée vit entièrement dans le nouveau module, gelée avec lui, sans lien avec
aucun module antérieur.

### Dehaze — dark channel prior en procédure close et déterministe

Le dehaze est une **suppression de voile atmosphérique de type *dark channel
prior*** (le canal sombre d'un pixel étant le minimum sur ses canaux RGB dans un
voisinage local, voile atmosphérique et transmission s'en estimant).

**L'estimation de la lumière atmosphérique et de la transmission doit être une
procédure entièrement spécifiée, déterministe et en forme close** — **aucune
optimisation itérative**, rien dont le résultat dépende d'une initialisation ou
d'une tolérance de convergence. **Décision : la lumière atmosphérique est estimée
à partir d'un percentile fixe des pixels les plus brillants du canal sombre de
l'image** (une **sélection en forme close**, pas un solveur itératif), et la
règle exacte de sélection est **gelée dans le contrat de rendu** une fois
implémentée.

Ce qui est **gelé ici**, c'est le **choix d'algorithme et de famille** — dark
channel prior, lumière atmosphérique par percentile supérieur du canal sombre,
transmission dérivée en forme close. Les **constantes numériques exactes** —
valeur du percentile, taille du voisinage du canal sombre, facteur de garde de la
transmission — relèvent de la PR d'implémentation, au **même niveau de précision**
qu'ADR 0016 (mathématique interne de Lensfun), ADR 0030 (spline tonale) et
ADR 0031 (modèle de teinte). Figer le modèle suffit à garantir la reproductibilité
une fois la process version publiée (`docs/pipeline.md` §5).

### Coût CPU — flou approché par downsampling, pas de grand noyau plein résolution

Le flou à grand rayon de la clarté est **coûteux** en plein résolution sur les
grandes previews/exports (`docs/engine-api.md` §11, signalé par la question
ouverte #3 du §6). Le `gaussian_blur` séparable existant (`process2.rs:396`) a un
noyau de rayon `⌈3σ⌉` : à grand σ il devient prohibitif.

**Décision : la famille d'algorithme du flou est une approximation par
sous-échantillonnage / filtre boîte (type pyramide gaussienne)** du flou à grand
rayon — **pas** un noyau gaussien littéral à grand rayon en pleine résolution —
afin de borner le coût. Le **facteur de sous-échantillonnage exact** et la
**taille de noyau** sont des constantes de la PR d'implémentation, pas décidées
ici — **même niveau de précision** que partout ailleurs dans cette série d'ADR ;
seule la **famille** — flou approché borné — est figée.

### Global uniquement en V2 — le masqué est déféré

Les trois curseurs sont livrés **en global**. Le **dehaze/clarté/texture masqué
ou régional** (les combiner avec l'infrastructure de masque spatial d'ADR 0029)
est **explicitement hors périmètre de la V2** — même coupe d'une ligne qu'ADR 0031
pour le color grading régional : c'est une extension naturelle une fois que cette
fonctionnalité et le masquage existent tous deux, déférée à un futur ADR adossé à
ADR 0029, pas conçue ici. Des curseurs globaux sont livrables **sans attendre**
l'item 2 (`docs/v2-scope.md` §6, question ouverte #2).

### Place dans le pipeline — clarté → texture → dehaze, avant Vibrance/Saturation

Les trois étages s'insèrent dans l'ordre fixe de `docs/pipeline.md` §3.1 dans le
**bloc tonal/présence, avant Vibrance/Saturation** : clarté et texture (contraste
local) près des curseurs de présence, dehaze après les curseurs tonals grossiers.
Ce que la ligne « Pipeline (§3.1) » du tableau `docs/v2-scope.md` §6 fixe déjà
(« clarté/texture… dans le bloc tonal/présence ; dehaze après le bloc tonal ») —
cet ADR **confirme et consomme** ce placement, il ne le re-dérive pas.

**Ordre précis des trois entre eux et vis-à-vis de Vibrance/Saturation :
clarté → texture → dehaze → Vibrance/Saturation.** Les trois atterrissent dans le
bloc tonal **avant** Vibrance/Saturation. Conséquence voulue : la position de
l'étage « Réglages locaux » d'ADR 0029 (immédiatement **après**
Vibrance/Saturation) reste **inchangée**, que cette fonctionnalité-ci ou l'item 2
sorte en premier — les deux insèrent leurs étages à des positions fixes distinctes
de l'ordre, sans se chevaucher et sans réconciliation (ADR 0028).

### Stockage — schéma additif

`clarity`, `texture` et `dehaze` sont trois curseurs **additifs** de
`settings_json`, dans `[-100, +100]`, **neutre = 0, absent = 0** — la même
convention d'unité et de plage que les curseurs sans dimension physique existants
(`contrast`, `vibrance`… `process2.rs:236`, `docs/pipeline.md` §3.2). À 0, chaque
étage est **entièrement sauté**, rendu **bit-pour-bit identique** à la process
version précédente — l'invariant « *a parameter at its neutral value skips its
operator entirely, so the neutral rendering is bit-for-bit the decoded image* »
(`process3.rs:25`). Aucun bump de schéma requis, cohérent avec « process +1,
schema inchangé » (`docs/v2-scope.md` §1) ; les champs inconnus d'un moteur ancien
sont préservés verbatim (`Settings::extra`,
`crates/leyline-core/src/settings.rs`).

> **Note de plage.** Les trois curseurs sont bidirectionnels `[-100, +100]` pour
> une uniformité de vocabulaire avec les curseurs existants et la parité
> Lightroom (dehaze négatif ré-ajoute un voile atmosphérique plutôt que de le
> retirer, clarté/texture négatives adoucissent le contraste local). C'est un
> détail d'UI mineur, **pas** un gel du contrat de rendu : la PR d'implémentation
> pourrait le restreindre à `[0, 100]` pour le dehaze si l'ergonomie l'exige, sans
> rouvrir cet ADR.

### Esquisse JSON

Le style suit `docs/pipeline.md` §3.2. Un exemple non neutre puis le cas neutre :

```json
{
    "schema": 1,
    "process": 9,

    "exposure": 0.2,
    "clarity": 25,
    "texture": 15,
    "dehaze": 30
}
```

Cas neutre — champs absents (ou à 0), rendu bit-pour-bit identique à la process
version précédente :

```json
{
    "schema": 1,
    "process": 9,
    "exposure": 0.2
}
```

> *Le `process: 9` ci-dessus est purement illustratif : le numéro réel est le
> prochain disponible au moment de la sortie (ADR 0028), pas fixé par cet ADR.*

> **Note d'implémentation (pas une édition de spec ici).** Cet ADR ne modifie
> **pas** le diagramme de `docs/pipeline.md` §3.1 ni le tableau des process
> versions §3.3. Comme pour ADR 0029/0030/0031/0032, la spec est mise à jour dans
> le même changement que l'implémentation réelle, conformément à CLAUDE.md. Le
> présent document fixe **où** les étages atterrissent et **quels** modèles ils
> gèlent ; le diagramme §3.1 et le tableau §3.3 seront amendés par la PR qui livre
> le module process.

## Conséquences

* **Le champ `process` garde sa lisibilité sémantique** (ADR 0028) : le nouveau
  numéro signifiera exactement « clarté/texture/dehaze actifs », un fait lisible
  comme `process: 3` signifie « correction de distorsion active ».
* **Sortie neutre gelée** : à 0, les trois étages sont bit-pour-bit la process
  version précédente (`process3.rs:25`).
* **Un seul chemin de contraste local à maintenir** : clarté et texture
  partagent une fonction paramétrée appelée à deux rayons — moins de code gelé,
  une seule surface de régression, et la distinction avec ADR 0028 (partage
  interdit *entre* modules, permis *dans* un module) est explicite.
* **Le dehaze est reproductible par construction** : estimation atmosphérique en
  forme close (percentile du canal sombre), aucune itération, aucune dépendance à
  une initialisation — gelée avec la process version (`docs/pipeline.md` §5).
* **Coût borné sur les grandes previews** : le flou à grand rayon est approché par
  sous-échantillonnage, jamais un noyau plein résolution — la préoccupation de
  `docs/engine-api.md` §11 est adressée par le choix de famille, les constantes
  restant à la PR.
* **Le dehaze/clarté/texture régional reste ouvert** pour un futur ADR adossé à
  ADR 0029, sans bloquer la version globale livrée ici (`docs/v2-scope.md` §6).
* **Trois étages avant Vibrance/Saturation** : la position de l'étage d'ADR 0029
  (après Vibrance/Saturation) reste intacte quel que soit l'ordre de sortie
  (ADR 0028).
* **Le contrat de reproductibilité** (`docs/pipeline.md` §5) est respecté :
  modèle de contraste local et règle de sélection atmosphérique figés par la
  process version, opérateurs purs et déterministes (ADR 0012), tout dans
  `settings_json`.
* **Un module `processN.rs` de plus** (ADR 0028) : aucun module antérieur touché,
  le gel « mêmes pixels dans dix ans » reste infalsifiable (§3.3).
* **La spec `docs/pipeline.md` (§3.1, §3.3) n'est pas éditée par cet ADR** : elle
  le sera par la PR d'implémentation, conformément à CLAUDE.md.

## Alternatives écartées

* **Clarté et texture comme deux algorithmes pleinement indépendants** au lieu
  d'une fonction paramétrée à deux rayons. Écarté : les deux **sont** le même
  algorithme — contraste local par masque flou — à un seul paramètre près, le
  rayon. Les écrire séparément dupliquerait la même mathématique dans le même
  module pour aucun bénéfice, et multiplierait les points où une régression
  pourrait diverger entre deux traitements censés être la même famille. La
  factorisation *intra-module* est permise (ADR 0028 n'interdit que le partage
  *entre* modules gelés) : une fonction, deux jeux de constantes.
* **Une estimation de voile itérative / par optimisation** au lieu d'une règle en
  forme close par percentile. Écarté : une optimisation itérative rendrait le
  résultat dépendant de l'initialisation et de la tolérance de convergence — un
  poison direct pour la reproductibilité « mêmes pixels » (`docs/pipeline.md`
  §5). Un percentile du canal sombre est une **sélection déterministe close**,
  gelable telle quelle avec la process version, à la précision d'ADR 0016.
* **Un flou gaussien littéral à grand rayon en pleine résolution** au lieu d'une
  approximation par sous-échantillonnage. Écarté : le noyau `⌈3σ⌉` de
  `gaussian_blur` (`process2.rs:396`) devient prohibitif au grand σ qu'exige la
  clarté, sur les grandes previews/exports (`docs/engine-api.md` §11). Un flou
  approché borné (downsampling / filtre boîte / pyramide) donne le même contraste
  large pour un coût maîtrisé ; l'approximation fait partie du rendu gelé, donc
  reproductible.
* **Supporter le dehaze/clarté/texture régional (sous masque) en V2.** Écarté :
  déféré à un futur ADR adossé à l'infrastructure de masque d'ADR 0029 — même
  coupe qu'ADR 0031 pour le color grading régional. La version globale est
  autonome et livrable sans le masquage ; la version régionale s'y adossera le
  moment venu, dans le référentiel commun d'ADR 0026, sans être recodée.
* **Insérer les trois étages après l'étage « Réglages locaux » d'ADR 0029** au
  lieu d'avant Vibrance/Saturation. Écarté : clarté/texture appartiennent au bloc
  présence et le dehaze suit les curseurs tonals grossiers, tous avant le bloc
  couleur (`docs/v2-scope.md` §6). Les placer après ADR 0029 déplacerait la
  position de son étage et changerait quels pixels alimentent le masquage. Les
  deux fonctionnalités insèrent à des positions fixes distinctes, sans
  réconciliation (ADR 0028) : celle qui sort la première n'impose rien à l'autre.
