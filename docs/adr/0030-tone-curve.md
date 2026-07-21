# ADR 0030 — Courbe tonale : courbe par points, spline cubique monotone, appliquée en luminance via LUT

**Statut :** Accepté — 2026-07

## Contexte

`docs/v2-scope.md` §3 (« Courbe tonale ») relève qu'aucune courbe — ni
paramétrique ni par points — n'existe aujourd'hui : seuls les curseurs
grossiers du bloc tonal (exposition, contraste, hautes lumières/ombres,
blancs/noirs) sont réalisés (`crates/leyline-engine/src/process2.rs:236`,
`:257`, `:280`). Une courbe tonale est l'outil de mise au point fine que ces
curseurs ne couvrent pas : elle laisse repositionner librement n'importe quel
niveau d'entrée sur n'importe quel niveau de sortie.

Le §3 esquisse trois champs (`tone_curve.points`, `tone_curve.parametric`,
`tone_curve.channel`) et laisse deux questions ouvertes : l'interpolation à
geler dans le contrat de rendu (`docs/pipeline.md` §5 : deux moteurs, mêmes
points, mêmes pixels), et le choix « courbes par canal RGB dès la V2 ou
luminance seule d'abord ». Le §9 confirme l'éligibilité d'un ADR propre
(« nouveau process, interpolation gelée (ADR léger) »).

Trois décisions transversales sont déjà prises en amont et **consommées, non
re-litigées, ici** :

* **ADR 0028** fige la stratégie de versionnage : une process version par
  fonctionnalité pixel, chacune dans son propre module `processN.rs` gelé,
  créé en copiant le module précédent entier. Cette courbe est un opérateur
  pixel : elle prend donc une nouvelle process version, sans que cet ADR ait
  à re-choisir la convention.
* **ADR 0013** a établi la convention de fonction de transfert par table :
  une LUT de `LUT_SIZE` intervalles dont l'entrée `i` est la formule exacte
  évaluée en `i / LUT_SIZE`, les lookups interpolant linéairement entre
  entrées adjacentes (`process2.rs:54`–`:101`). Cet ADR **réemploie** cette
  convention pour la courbe, il ne la réinvente pas.
* **ADR 0027** confirme que l'espace de travail interne du rendu reste sRGB
  gamma-encodé entre opérateurs — l'espace où le bloc tonal existant est
  défini et où cette courbe s'insère.

## Décision

La courbe tonale est un nouvel opérateur pixel, donc une **nouvelle process
version** : elle prend **le prochain numéro de process disponible au moment de
la sortie de cette fonctionnalité** (ADR 0028), dans son propre module
`processN.rs` copie intégrale du module précédent augmentée du seul opérateur
courbe — exactement la convention de duplication par module réaffirmée par
ADR 0028. Cet ADR **ne fige pas** un entier de process précis : l'ordre de
sortie des items 3/4/5/6/8 relève du plan d'implémentation futur, pas de ce
document.

### La simplification centrale — courbe par points seulement, pas de moteur paramétrique

**La V2 ne livre que la courbe par points** (`tone_curve.points`, liste de
points de contrôle `{x, y}` normalisés `[0,1]`). Le champ
`tone_curve.parametric` esquissé au §3 (régions highlights/lights/darks/
shadows + points de bascule) est **retiré du contrat de rendu du moteur**.

Raisonnement, posé comme la décision centrale de cet ADR et non comme un
oubli : une courbe paramétrique n'est **pas une mathématique de rendu
distincte**, c'est une **UI différente pour générer une liste de points de
contrôle**. Les curseurs de régions d'une courbe paramétrique produisent, in
fine, une courbe — c'est-à-dire un jeu de points. Si Studio veut un jour offrir
un éditeur de style paramétrique, il calcule **côté client** la liste de points
équivalente et l'écrit dans `tone_curve.points` ; le moteur n'a alors **qu'un
seul chemin mathématique de courbe** à geler et à maintenir « mêmes pixels dans
dix ans » (`docs/pipeline.md` §3.3), au lieu de deux. Porter deux moteurs de
courbe séparés dans le rendu gelé serait deux contrats à figer, deux surfaces
de régression, pour une capacité que le premier subsume entièrement.

### Interpolation — figée dans le contrat de rendu

L'interpolation entre points de contrôle est une **spline cubique monotone**
(Fritsch–Carlson ou équivalent préservant la monotonie). Ce choix est gelé
ici parce qu'il fait partie du contrat de reproductibilité (`docs/pipeline.md`
§5 : deux moteurs, mêmes points, doivent produire les mêmes pixels) — au même
titre que la fonction de transfert d'ADR 0013 ou la mathématique interne de
Lensfun d'ADR 0016. Une spline cubique **monotone** est choisie spécifiquement
pour éviter l'*overshoot*/le *ringing* qu'une spline cubique naïve introduit
entre des points de contrôle largement espacés : entre deux points, une
cubique naïve peut dépasser puis revenir, créant des inversions de tons
visibles (bandes, halos) là où l'utilisateur attend une transition monotone.
Un opérateur tonal doit rester monotone comme le sont déjà `contrast`,
`highlights_shadows` et `whites_blacks` (« *both blends are monotone* »,
`process2.rs:235` ; « *so the endpoints are fixed and the response is
monotone* », `process2.rs:256`).

Cet ADR gèle le **choix de modèle** (spline cubique monotone). Les détails
numériques exacts de l'implémentation (formulation précise des tangentes,
gestion des points colinéaires) relèvent de la PR d'implémentation, au même
niveau de précision qu'ADR 0016 pour la mathématique interne de Lensfun —
figer le modèle suffit à garantir la reproductibilité une fois la process
version publiée.

### Canal — luminance seule

La V2 applique la courbe **en luminance uniquement** : une seule courbe
partagée, appliquée au même tampon RGB de travail gamma-encodé sRGB
(`process2.rs:30`), et **non** trois courbes indépendantes par canal R/G/B.
Le champ `tone_curve.channel` esquissé au §3 est donc réduit à sa seule valeur
neutre implicite (luminance) ; les courbes par canal sont **retranchées de la
V2** comme une fonctionnalité séparée et plus lourde (UI plus grande, trois
fois l'état de courbe, question de l'ordre d'application des trois courbes) —
à concevoir plus tard si elle est voulue, dans son propre ADR. C'est une coupe
délibérée, dans le même esprit qu'ADR 0016 retranchant vignettage/TCA du
process 3 pour livrer d'abord le cœur.

### Précalcul — LUT, pas d'évaluation par pixel

La courbe est précalculée en **LUT** puis appliquée par lookup interpolé, en
réutilisant exactement la convention d'ADR 0013 (`process2.rs:74`–`:90`) :
table + interpolation linéaire entre entrées, plutôt que d'évaluer la spline
en chaque pixel. La spline est évaluée une fois par entrée de table à la
construction de la révision ; le rendu par pixel n'est qu'un lookup. La
**résolution** exacte de la LUT est une constante de la PR d'implémentation,
pas décidée ici (comme `LUT_SIZE` est une constante gelée du module process,
`process2.rs:54`, et non un choix d'ADR) ; seule la *méthode* — précalcul en
LUT — est figée.

### Place dans le pipeline

L'étage courbe s'insère dans l'ordre fixe de `docs/pipeline.md` §3.1 **après
Blancs/Noirs et avant Vibrance/Saturation** — ce que la ligne « Pipeline
(§3.1) » du tableau `docs/v2-scope.md` §3 fixe déjà. Cet ADR **confirme et
consomme** ce placement, il ne le re-dérive pas. C'est la dernière étape du
bloc tonal avant le bloc couleur : la courbe opère sur des tons déjà réglés
par les curseurs grossiers, avant que la saturation ne s'applique.

### Stockage — schéma additif

`tone_curve.points` est un champ **additif** de `settings_json`. **Absent ou
liste vide = courbe identité** (chaque niveau se mappe sur lui-même), rendu
**bit-pour-bit identique** à la process version précédente. Cela préserve
l'invariant « *a parameter at its neutral value skips its operator entirely,
so the neutral rendering is bit-for-bit the decoded image* » documenté en tête
de `process3.rs:25`. Aucun bump de schéma requis, cohérent avec le schéma
additif « process +1, schema inchangé » de la plupart des items V2
(`docs/v2-scope.md` §1) — les champs inconnus d'un moteur ancien sont déjà
préservés verbatim (`Settings::extra`, `crates/leyline-core/src/settings.rs`).

### Esquisse JSON

Le style suit `docs/pipeline.md` §3.2. Une courbe en S doux (relève les ombres,
abaisse les hautes lumières) et le cas neutre :

```json
{
    "schema": 1,
    "process": 7,

    "exposure": 0.2,
    "tone_curve": {
        "points": [
            { "x": 0.0,  "y": 0.0 },
            { "x": 0.25, "y": 0.30 },
            { "x": 0.75, "y": 0.70 },
            { "x": 1.0,  "y": 1.0 }
        ]
    }
}
```

Cas neutre — champ absent (ou `"points": []`), rendu bit-pour-bit identique à
la process version précédente :

```json
{
    "schema": 1,
    "process": 7,
    "exposure": 0.2
}
```

> *Le `process: 7` ci-dessus est purement illustratif : le numéro réel est le
> prochain disponible au moment de la sortie (ADR 0028), pas fixé par cet ADR.*

### Masquage — hors décision ici

Cet ADR ne décide **pas** si la courbe tonale devient un réglage masquable
sous ADR 0029 (`LocalAdjustment`) : ce serait une petite extension future du
jeu de champs ajustables de ce struct, pas conçue ici.

> **Note d'implémentation (pas une édition de spec ici).** Cet ADR ne modifie
> **pas** le diagramme de `docs/pipeline.md` §3.1 ni le tableau des process
> versions §3.3. Comme pour ADR 0029, la spec est mise à jour dans le même
> changement que l'implémentation réelle, conformément à CLAUDE.md. Le présent
> document fixe seulement **où** l'étage atterrit et **quelle** mathématique il
> gèle ; le diagramme §3.1 et le tableau §3.3 seront amendés par la PR qui
> livre le module process de la courbe.

## Conséquences

* **Le champ `process` garde sa lisibilité sémantique** (ADR 0028) : le
  nouveau numéro signifiera exactement « courbe tonale par points active », un
  fait unique et lisible, comme `process: 3` signifie « correction de
  distorsion active ».
* **Sortie neutre gelée** : sans points, l'étage est bit-pour-bit la process
  version précédente. L'invariant « valeur neutre → opérateur entièrement
  sauté » (`process3.rs:25`) reste vrai pour l'étage entier.
* **Un seul chemin de courbe à maintenir** : la coupe du mode paramétrique
  garde le rendu gelé minimal ; un futur éditeur paramétrique de Studio
  n'ajoute aucun code de rendu, il écrit des points.
* **Le contrat de reproductibilité** (`docs/pipeline.md` §5) est respecté :
  l'interpolation monotone est figée par la process version, la LUT est pure
  et déterministe (ADR 0012), tout se sérialise dans `settings_json` — « même
  révision → mêmes pixels ».
* **Deux extensions restent ouvertes pour de futurs ADR** sans bloquer le
  cœur : courbes par canal R/G/B, et courbe masquée sous ADR 0029. Aucune
  n'est requise pour livrer la courbe par points en luminance.
* **Un module `processN.rs` de plus** (ADR 0028) : coût borné et connu ; aucun
  module de version antérieure n'est touché, le gel « mêmes pixels dans dix
  ans » reste mécaniquement infalsifiable (`docs/pipeline.md` §3.3).
* **La spec `docs/pipeline.md` (§3.1, §3.3) n'est pas éditée par cet ADR** :
  elle le sera par la PR d'implémentation, conformément à CLAUDE.md.

## Alternatives écartées

* **Livrer les courbes paramétriques par régions comme fonctionnalité moteur
  distincte de premier ordre**, à côté de la courbe par points. Écarté :
  c'est la simplification centrale de cet ADR. Une courbe paramétrique ne
  produit qu'une liste de points ; en faire un second moteur de rendu obligerait
  à geler et maintenir **deux** chemins mathématiques de courbe « mêmes pixels
  dans dix ans » (§3.3), deux surfaces de régression, alors que la courbe par
  points les subsume toutes deux. Studio calcule la liste de points équivalente
  côté client si un éditeur paramétrique est un jour voulu — aucun code de
  rendu supplémentaire, aucun contrat gelé supplémentaire.
* **Courbes par canal R/G/B dès la V2.** Écarté comme fonctionnalité séparée
  et plus lourde : trois fois l'état de courbe, une UI par canal, et la
  question de l'ordre d'application des trois courbes entre elles. La luminance
  seule couvre l'usage tonal principal ; les courbes par canal (virage
  colorimétrique par courbe) sont un chantier à part, à concevoir plus tard
  dans son propre ADR si voulu — même esprit de coupe qu'ADR 0016.
* **Spline cubique naïve (non monotone).** Écarté : entre points de contrôle
  largement espacés, une cubique naïve *overshoote* puis revient, créant des
  inversions de tons visibles (bandes, halos) là où l'utilisateur attend une
  transition monotone. Tous les opérateurs tonals existants sont monotones à
  dessein (`process2.rs:235`, `:256`) ; une courbe qui romprait cette propriété
  serait un régression de qualité perceptible. La spline cubique monotone
  (Fritsch–Carlson) donne des transitions douces **sans** dépassement.
* **Évaluer la spline par pixel plutôt que via une LUT.** Écarté : la spline
  est coûteuse à évaluer et ne dépend que de la valeur d'entrée `[0,1]` — le
  cas d'usage exact d'une table (ADR 0013). Précalculer une LUT une fois par
  révision puis faire un lookup interpolé par pixel réutilise la convention
  déjà gelée du moteur (`process2.rs:54`–`:90`), pour un coût par pixel
  constant au lieu d'une évaluation de spline complète à chaque échantillon.
