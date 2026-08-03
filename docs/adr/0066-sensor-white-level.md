# ADR 0066 — Le niveau de blanc vient du capteur, pas de la photo

**Statut :** Accepté — 2026-08

## Contexte

Les nombres d'un fichier RAW ne veulent rien dire tant que quelque chose n'a
pas désigné **la valeur qui compte pour « blanc »**. Tout le rendu en découle :
diviser par un niveau trop haut assombrit l'image entière et fait ressortir
gris un pixel pourtant saturé au capteur.

Leyline n'avait jamais fait ce choix. Il prenait ce que LibRaw laissait dans
`maximum`, sans le nommer nulle part — et ce que LibRaw y laisse **dépend de la
photo**.

### Ce que LibRaw fait par défaut

`params.adjust_maximum_thr` vaut 0,75. Avant la normalisation, LibRaw abaisse
alors `maximum` jusqu'à l'échantillon le plus lumineux **de cette image-là**,
dès qu'il dépasse 0,75 du plafond du format. Mesuré sur quatre fichiers d'une
même série, Canon 60D, ISO 100, même exposition :

| Fichier | Échantillon le plus lumineux | Niveau de blanc retenu |
|---|---|---|
| IMG_9040 | 13 794 | **13 794** — son propre pixel le plus clair |
| IMG_9041 | 10 828 | **16 383** — le plafond du 14 bits |
| IMG_9042 | 2 807 | 16 383 |
| IMG_9044 | 1 641 | 16 383 |

Deux photos de la même scène, prises à la suite, se retrouvent donc normalisées
par des niveaux distants de 19 % — selon qu'un reflet spéculaire est tombé ou
non dans le cadre. La luminosité du rendu neutre dépendait du contenu de
l'image.

**C'est exactement ce que `auto_brighten: false` interdit.** Ce champ porte,
depuis la V1, le commentaire « le rendu neutre ne doit pas dépendre du contenu
de l'image ». L'intention était juste ; elle était contournée un étage plus
bas, par un réglage que personne n'avait regardé.

### Ce que le boîtier, lui, sait

Le fichier porte la réponse. LibRaw lit dans les métadonnées Canon une **marge
de linéarité** — le niveau au-delà duquel le capteur cesse de répondre
proportionnellement — et la range dans `linear_max`. Sur ce boîtier elle ne
dépend que de la sensibilité :

| Groupe d'ISO | `linear_max` |
|---|---|
| 100, 125 | 12 279 |
| 200 … 3200 | 15 094 |
| 160, 320, 640, 1250, 2500 | 11 222 |

Le découpage en trois groupes est **le même** que celui de la table mesurée que
RawTherapee maintient de son côté (`camconst.json`) : deux observateurs
indépendants du même comportement matériel. `identify` s'en sert d'ailleurs
pour renseigner `maximum` — et `unpack` l'écrase ensuite par le plafond du
format.

## Décision

**Le niveau de blanc est celui que le boîtier a écrit, et jamais celui que la
photo contient.**

### 1. La règle

1. La marge de linéarité du fichier (`linear_max`), quand le boîtier en écrit
   une ;
2. sinon, le plafond du format (`maximum`), comme avant.

Et dans les deux cas, **l'ajustement par le contenu est coupé**
(`adjust_maximum_thr = 0`) : sans cela, le second cas resterait dépendant de
l'image pour les boîtiers sans métadonnée, c'est-à-dire précisément là où on ne
peut rien vérifier.

Le niveau ne dépend donc plus que du boîtier et de sa sensibilité. Deux photos
d'une même série se rendent enfin pareil.

### 2. Une nouvelle version de l'étage `input`

`input::v4`. Le rendu change, donc la version d'étage change
(`pipeline.md` §5.1) — une révision existante continue de rendre par `v3`,
inchangée, jusqu'à ce que quelqu'un la reprocesse.

Contrairement à `v2` et `v3`, **`v4` n'est pas identique à son prédécesseur à
réglages neutres**, et ne peut pas l'être : redéfinir le blanc est tout ce
qu'elle fait. Une photo reprocessée en `v4` devient plus claire de 12 à 33 %
selon ce que `v3` avait retenu pour elle.

### 3. Ce que les rendus de référence ne peuvent pas geler

Les goldens rendent un tampon synthétique et ne passent jamais par LibRaw :
`v3` et `v4` y produisent les mêmes pixels, au bit près. Ce que `v4` change
n'est pas gelable là. C'est donc un test de configuration du décodeur qui le
fixe (`the_white_level_reaches_the_decoder_only_from_input_v4`), plus un test
sur fichier réel derrière `LEYLINE_TEST_RAW`. Même trou, même remède que le
mode de hautes lumières d'[ADR 0050](0050-highlight-reconstruction.md).

### 4. Ce que cette décision **ne** règle **pas**

RawTherapee rend toujours plus clair que Leyline : ×1,16 avant, ×1,03 après sur
un fichier ISO 100, et l'écart *augmente* sur un fichier ISO 400 (×1,08 avant,
×1,13 après) parce que `v3` y étirait le blanc jusqu'au pixel le plus clair
d'une image qui n'en avait pas de très clair.

Il reste donc, après cette correction, **un facteur d'environ 1,14 constant
entre les deux moteurs, qui n'est pas le niveau de blanc** : les diviseurs
effectifs des deux côtés sont connus et n'expliquent pas cet écart. Ce n'est
pas une raison de ne pas corriger ce qui est corrigé ici — la dépendance au
contenu est un défaut en soi, indépendamment de tout comparatif. C'est une
raison de ne pas prétendre que le sujet est clos.

## Conséquences

* Un pixel saturé au capteur ressort **blanc**, ce qui n'était pas le cas.
* La luminosité d'un rendu neutre ne bouge plus d'une photo à l'autre d'une
  même série.
* `leyline-raw` expose `WhiteLevel`, deux valeurs nommées ; le défaut du type
  reste `FormatCeiling`, ce que les versions `input` gelées demandent
  explicitement depuis cette ADR.
* Le shim gagne deux fonctions, `linear_max` (lecture, **avant `unpack`**, qui
  écrase la valeur) et `user_sat` (écriture).
* Les bibliothèques existantes ne changent pas d'aspect tant qu'un reprocess ne
  le demande pas.

## Alternatives écartées

* **Reprendre la table `camconst.json` de RawTherapee** (GPL-3.0, donc
  juridiquement possible ici avec attribution) : plus précise, et c'est une
  base de données tierce à maintenir, à étendre boîtier par boîtier, et à
  reprendre à chaque nouvelle mesure de leur côté. La métadonnée du fichier
  répond pour tous les boîtiers sans rien maintenir. À rouvrir si un jour la
  différence entre les deux sources se montre visible.
* **Garder l'ajustement par le contenu** (le défaut de LibRaw) : c'est le
  défaut corrigé ici.
* **Régler `adjust_maximum_thr` plus haut** plutôt que de le couper : déplace
  le seuil sans supprimer la dépendance — la même photo avec et sans reflet
  rendrait encore différemment, un peu moins souvent.
* **Corriger sans nouvelle version d'étage** : romprait `pipeline.md` §5.1, qui
  est la promesse la plus chère du projet.
