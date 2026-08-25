# ADR 0063 — Appliquer les tables d'un profil DCP

**Statut :** Accepté — 2026-08

**Complète :** [ADR 0035](0035-camera-profile-dcp.md), [ADR 0037](0037-dcp-parsing-dependency.md), [ADR 0062](0062-dcp-illuminant-interpolation.md)

## Contexte

Un profil DCP porte quatre choses. Leyline en applique **une** : les matrices.
Les trois autres — `ProfileHueSatMapData`, `ProfileLookTableData`,
`ProfileToneCurve` — ne sont ni lues ni appliquées depuis ADR 0035, qui l'a
signalé comme un manque connu.

Ce sont pourtant elles qui portent le *rendu*. La matrice fait une conversion
colorimétriquement correcte ; c'est la look table qui fait qu'un fichier « a
l'air de sortir de Lightroom ». Tant qu'elles manquent, la comparaison à un
convertisseur de référence est perdue d'avance, quelle que soit la qualité du
reste du pipeline.

Trois profils réels ont été inventoriés le 2026-08-02, tags lus directement :

| | Canon 60D *(linéaire)* | Canon 5D IV *(linéaire)* | Canon 60D *(RawTherapee)* |
|---|---|---|---|
| `HueSatMapDims` | absent | 90×25×1 | 90×30×1 |
| `ProfileToneCurve` | 2 points | 2 points | 8192 points |
| `LookTableDims` | 36×8×16 | 36×8×16 | 90×30×30 |

Deux enseignements de cet inventaire, tous deux contraires à ce qu'on suppose :

**Un profil « linéaire » n'est pas un profil matriciel.** Il a une look table
complète ; ce qui est linéaire, c'est sa *courbe tonale*, réduite aux deux
points (0,0) et (1,1). L'identité s'y lit littéralement dans les quatre
flottants du tag.

**Les dimensions varient beaucoup d'un profil à l'autre** — de 4 608 à 81 000
entrées. Rien ne peut être codé en dur.

## Décision

**Les trois tables sont lues et appliquées, dans l'ordre et l'espace que la
spec DNG impose, par une nouvelle version `camera_profile::v3`.**

### 1. L'ordre, qui n'est pas celui qu'on devine

Vérifié dans le code de référence tel que le reprend RawTherapee
(`rtengine/dcp.cc`, `applyStep1`/`applyStep2`) :

```
RGB caméra
  → HueSatMap                (tôt, avant la matrice)
  → matrice avant → XYZ(D50)
  → ProPhoto RGB linéaire
  → LookTable                (en TSV)
  → ProfileToneCurve
  → espace de travail
```

**La look table passe avant la courbe tonale**, pas après. C'est l'inverse de
ce que suggèrent les noms, et de ce que j'aurais écrit sans vérifier.

**Tout se passe en ProPhoto RGB**, pas dans notre espace de travail Rec. 2020
(ADR 0044). Les tables sont définies contre ProPhoto ; les appliquer ailleurs
donnerait des couleurs fausses tout en ayant l'air de fonctionner. On convertit
donc, on applique, on revient.

### 2. Les hautes lumières, et le conflit avec ADR 0044

Les tables sont définies sur du TSV borné à `[0, 1]`. Notre tampon de travail
est **délibérément non borné au-dessus du blanc** (ADR 0044 §1) : c'est là que
vit la marge des hautes lumières d'un RAW, et la préserver était tout l'objet
de cette décision.

Appliquer une table bornée écraserait cette marge. La règle retenue est celle
du code de référence : **la table est calculée sur la valeur écrêtée, et n'est
écrite que si l'échantillon était déjà dans `[0, 1]`.** Un échantillon
au-dessus du blanc traverse inchangé.

C'est un compromis, et il faut le nommer : une haute lumière ne reçoit pas le
« look » du profil. L'alternative — écrêter pour appliquer la table — perdrait
une information que le pipeline entier est construit pour garder.

### 3. Interpolation

Une table est un cube TSV échantillonné : teinte × saturation × valeur, trois
deltas par entrée (décalage de teinte, facteur de saturation, facteur de
valeur). L'interpolation est **trilinéaire**, avec la teinte **cyclique** — le
360e degré est voisin du 0e, et traiter la teinte comme un axe ouvert produit
une couture visible sur les rouges.

Une table à `val = 1` n'a qu'un plan : l'interpolation dégénère sur cet axe, ce
qui est le cas des trois profils inventoriés et doit donc marcher.

### 4. Deux tables, deux illuminants

`HueSatMapData1` et `HueSatMapData2` correspondent aux deux illuminants de
calibration. Elles sont mélangées **par le même poids en mireds** qu'ADR 0062
calcule pour les matrices — un profil ne doit pas interpoler ses matrices sous
une lumière et ses tables sous une autre.

### 5. `camera_profile::v3`

Les pixels changent, donc nouvelle version d'étage ; `v1` (moyenne) et `v2`
(interpolation, sans tables) restent figés. Un profil sans aucune table rend en
`v3` exactement comme en `v2` — ce que les rendus de référence doivent montrer.

## Conséquences

* Le rendu d'une photo avec profil DCP change nettement, et se rapproche de ce
  qu'un convertisseur de référence produit. C'est le but.
* `DcpProfile` grossit : jusqu'à 81 000 flottants par table, deux tables
  possibles. Un profil est chargé une fois par rendu, pas par pixel.
* La conversion vers ProPhoto et retour entre dans `leyline-color`.
* La mention « expérimental » d'ADR 0035 pourra être levée **si** une
  comparaison à un rendu de référence le confirme — ce qui reste bloqué faute
  de convertisseur de référence disponible (`measured-findings.md` §A1).

## Alternatives écartées

* **N'appliquer que la courbe tonale**, la plus simple des trois : c'est celle
  qui est neutralisée dans les profils linéaires, donc la seule dont l'absence
  ne se voit pas. Faire l'inverse du bon choix.
* **Appliquer les tables dans notre espace Rec. 2020** sans passer par
  ProPhoto : économise deux matrices par pixel, et donne des couleurs fausses
  avec l'apparence du fonctionnement — le pire mode de défaillance.
* **Écrêter à `[0, 1]` pour appliquer les tables partout** : ferait de la
  cohérence du look la priorité sur la préservation des hautes lumières, à
  rebours d'ADR 0044.
* **Interpolation au plus proche voisin** au lieu de trilinéaire : visible en
  bandes sur un dégradé de ciel, pour une économie sans objet à cette échelle.
