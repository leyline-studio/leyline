# ADR 0050 — Reconstruction des hautes lumières : un mode de décodage, épinglé par `input::v2`

**Statut :** Accepté — 2026-07
**Suite :** `input::v2`, que cet ADR crée, n'est plus la version courante.
[ADR 0061](0061-demosaic-algorithm.md) rend l'algorithme de dématriçage
choisissable (`v3`, identique bit pour bit à `v2` à réglage neutre), puis
[ADR 0066](0066-sensor-white-level.md) fait venir le niveau de blanc du capteur
plutôt que du contenu de la photo (`v4`, délibérément **pas** identique à son
prédécesseur). La reconstruction des hautes lumières décidée ici traverse les
deux sans changer.

## Contexte

Le décodeur n'a jamais reçu d'instruction sur les hautes lumières écrêtées.
`crates/leyline-raw/src/shim.c` ne touche pas `params.highlight`, dont la valeur
par défaut de LibRaw est **0 — écrêter au blanc**. Chaque canal qui a saturé au
capteur ressort donc à la valeur maximale, et l'information que les deux autres
canaux portent encore est jetée avant le premier curseur.

Ce que cela coûte est visible sur toute photo où un canal sature seul, ce qui
est le cas courant : un ciel bleu clair (le bleu sature d'abord), une peau en
plein soleil (le rouge), un nuage lumineux. La zone ressort en aplat blanc, et
aucun réglage en aval ne peut la reconstruire — `Settings::highlights`
redescend une luminosité, il ne réinvente pas un canal perdu.

Les concurrents traitent exactement ce cas, et depuis longtemps : dcraw
l'expose en `-H` depuis vingt ans, RawTherapee en fait un module à quatre
méthodes, darktable en a deux (dont sa *guided laplacian*). C'est le défaut de
rendu le plus visible de Leyline face à eux, et il ne vient pas d'un arbitrage :
personne n'avait posé la question.

**Ce qui n'est pas en cause.** L'espace de travail non borné d'
[ADR 0044](0044-linear-wide-gamut-working-space.md), qui est ce qui rend cette
décision utile — un tampon écrêté au blanc n'aurait nulle part où mettre ce
qu'on reconstruit. Ni l'épaule de sortie (`output_rendering`), qui décide ce que
*devient* la marge au-dessus du blanc et non ce qui la remplit.

## Décision

### 1. La reconstruction est une configuration du décodeur, pas un opérateur

Les modes de LibRaw opèrent sur les données **avant dématriçage**, là où le
voisinage d'un pixel saturé est encore une mosaïque de canaux distincts. C'est
la seule place où l'information nécessaire existe : après dématriçage, un pixel
écrêté est entouré de pixels déjà interpolés depuis des canaux écrêtés.

Écrire notre propre reconstruction demanderait donc d'abord d'exposer la
mosaïque brute à travers le shim, puis de réimplémenter — moins bien — un
algorithme que LibRaw livre déjà, testé sur des milliers de boîtiers. Le mode de
LibRaw est retenu ; §1 des alternatives écartées dit pourquoi la question se
reposera peut-être un jour, mais pas ici.

Conséquence directe : c'est un réglage que **`input` épingle**, puisque `input`
est précisément l'étage qui porte « la configuration demandée au décodeur »
(ADR 0044 §3, `docs/pipeline.md` §3.3). Il ne crée aucun étage nouveau et ne
déplace aucun rang.

### 2. Trois modes, pas neuf

`params.highlight` de LibRaw accepte 0 à 9. Leyline en expose trois :

| Réglage | LibRaw | Ce que ça fait |
| :--- | :---: | :--- |
| `clip` (défaut, neutre) | 0 | écrêter au blanc — le comportement d'avant cette décision |
| `blend` | 2 | mélanger les canaux écrêtés et non écrêtés : récupère de la texture sans dériver en couleur |
| `rebuild` | 5 | reconstruire le canal manquant depuis les autres : récupère le plus, au prix d'un risque de teinte dans les zones très saturées |

Le mode 1 (*unclip*) n'est pas exposé : il laisse les hautes lumières prendre la
teinte magenta caractéristique d'un canal laissé au-delà des autres, ce qui a
l'apparence d'un bug pour tout utilisateur qui n'a pas lu dcraw. Les niveaux 3
à 9 sont une même famille avec un curseur de force ; 5 est la valeur médiane et
celle que dcraw documente comme point de départ. Exposer un entier de 3 à 9
demanderait à l'utilisateur de deviner ce que le chiffre veut dire.

`clip` reste le défaut. Ce n'est pas une préférence esthétique : c'est la règle
du projet — la valeur neutre d'un réglage est celle qui ne change rien —, et
elle est ici doublement nécessaire, puisque changer le défaut modifierait le
rendu de toute photo déjà importée.

### 3. Rendre le gain que le décodeur retire

Mesuré sur un vrai CR2 : demander `blend` ou `rebuild` **assombrit toute la
photo** d'environ un tiers, hautes lumières comprises. Ce n'est pas un défaut
d'implémentation, c'est le fonctionnement de dcraw, repris par LibRaw : la
normalisation par les multiplicateurs de balance des blancs divise par le
**plus petit** d'entre eux quand on écrête — tous les canaux montent alors à 1
ou au-dessus, et le plus fort sature — et par le **plus grand** quand on
reconstruit, pour qu'aucun canal ne puisse dépasser le blanc. L'écart entre les
deux est un gain global, identique pour tous les pixels.

Le laisser tel quel serait inacceptable : « récupérer les hautes lumières »
donnerait l'apparence d'un curseur d'exposition, et l'utilisateur compenserait
à la main sans savoir pourquoi. `input::v2` le **rend** donc, en multipliant le
tampon par le rapport `max/min` des multiplicateurs as-shot du boîtier, que
`leyline-raw` expose pour cela (`RawMetadata::camera_multipliers`).

Le résultat est exactement ce que la fonction doit être : les tons moyens
reviennent là où l'écrêtage les mettait — mesuré à 0,1 % près sur le même
fichier — et ce qui a été reconstruit atterrit **au-dessus du blanc**, où le
tampon non borné d'ADR 0044 le garde jusqu'à ce que `output_rendering` décide de
son sort. C'est une opération sur les hautes lumières, pas sur l'exposition.

Un fichier sans balance des blancs enregistrée ne reçoit aucune compensation :
pas de multiplicateurs, donc pas de rapport à rendre — et pas de correction
inventée.

### 4. `input::v2`, et un `v1` qui ne bouge pas

Le mode de décodage fait partie du rendu, donc de la promesse de
`docs/pipeline.md` §5.1. Il lui faut une nouvelle version d'étage :

* `input::v1` continue de demander exactement ce qu'elle demandait et **ignore**
  le réglage ;
* `input::v2` lit le réglage et le passe au décodeur ; sa conversion vers
  l'espace de travail est une copie de celle de `v1` (ADR 0042 : la duplication
  est le prix du gel) ;
* les nouvelles révisions épinglent `input: 2`, les anciennes gardent `input: 1`.

La table `INPUT_DECODE` qui associe une version d'`input` à sa configuration de
décodeur voit sa signature passer de `fn(bool)` à `fn(&Settings, bool)`. Ce
n'est pas une édition de version publiée au sens d'ADR 0042 §1 : ce que le gel
protège est **ce que `v1` demande au décodeur**, et `v1` demande la même chose
qu'avant en ignorant son nouvel argument.

### 5. Un mode non neutre sur une révision épinglée en `input: 1` est **refusé**

C'est le cas de figure d'[ADR 0048](0048-range-masks.md) §5, mot pour mot :
une révision de 2026 épingle `input: 1`, l'utilisateur y demande `rebuild` en
2027, la règle d'épinglage garde `v1`, et `v1` ne connaît pas le réglage. Le
mode disparaîtrait en silence.

`Settings::validate()` refuse donc la combinaison, et le message nomme le
remède : retraiter la photo (`docs/pipeline.md` §4.5), ce qui crée une révision
épinglée en `input: 2`. C'est la deuxième application de la règle générale
qu'ADR 0048 §5 a dégagée — **un réglage qu'une version épinglée ne sait pas
exprimer est un refus de validation, jamais une valeur perdue** — et la
première qui ne concerne pas un opérateur de pixels mais le décodeur.

### 6. Ce que la reproductibilité couvre ici

La reconstruction est déterministe : mêmes octets d'entrée, mêmes paramètres,
mêmes pixels. Elle dépend en revanche de la **version de LibRaw**, comme tout le
décodage depuis le premier jour — ce que `docs/pipeline.md` §5.2 range déjà sous
« changer de plateforme ». Cette décision n'élargit pas la zone non garantie :
elle y ajoute un paramètre dont l'effet est visible, là où le décodage y était
déjà entièrement.

### 7. Hors périmètre

* **Le choix de l'algorithme de dématriçage** (`params.user_qual`), aujourd'hui
  laissé au défaut de LibRaw. Même famille de question — un paramètre de
  décodeur qu'`input` épinglerait —, mais des arbitrages entièrement
  différents ; son propre ADR.
* **Une reconstruction maison à partir de la mosaïque** (§1 des alternatives).
* **Un curseur de force pour `rebuild`** (les niveaux 3 à 9). Ajoutable plus
  tard sans nouvelle décision : ce serait un mode de plus dans la même énumération.

## Conséquences

* **Le défaut de rendu le plus visible face à darktable et RawTherapee
  disparaît**, pour un réglage à trois valeurs et une version d'étage.
* **Le tampon non borné d'ADR 0044 sert enfin à ce qu'il promettait** : ce que
  `rebuild` remonte au-dessus du blanc traverse tout le pipeline et c'est
  l'épaule de `output_rendering` qui le ramène — les deux décisions composent
  exactement comme prévu, sans que l'une ait à connaître l'autre.
* **`leyline-raw` expose une donnée de plus, et une seule** : les
  multiplicateurs as-shot, pour le gain du §3. Aucun autre étage ne les lit.
* **Un cas de rendu de référence de plus**, qui gèle la compensation du §3 —
  la seule partie de cette décision qu'un tampon synthétique puisse exercer,
  le mode lui-même vivant dans le décodeur.
* **Le cache de décodage reste correct sans y toucher** : il est indexé par
  `(asset, DecodeParams)`, donc deux modes sont deux entrées.
* **Troisième version d'étage réelle du projet** (après 0046 et 0048), et la
  première sur un étage d'encadrement — ce qui exerce le fait qu'`input` épingle
  autre chose que des pixels calculés par nous.
* **Un réglage de plus au-dessus du blanc, mais aucun schéma changé** :
  champ optionnel à valeur neutre absente (`docs/pipeline.md` §3.4).

## Alternatives écartées

* **Reconstruire nous-mêmes depuis la mosaïque brute.** Il faudrait exposer les
  données Bayer à travers le shim, gérer les motifs non Bayer (X-Trans), et
  réimplémenter un algorithme éprouvé. Le jour où le dématriçage deviendra un
  choix du projet (hors périmètre §6), la question se reposera dans un cadre où
  elle a du sens ; aujourd'hui elle ajouterait du risque sans rien gagner.
* **Reconstruire après dématriçage, dans un étage à nous.** Séduisant parce que
  cela resterait dans du code gelé par nous plutôt que dans LibRaw — mais
  l'information nécessaire n'existe plus à cet endroit (§1). On obtiendrait un
  lissage de zones blanches, pas une reconstruction.
* **Activer `blend` par défaut.** Meilleur rendu pour presque toute photo, et
  inacceptable : cela changerait le rendu de l'existant, ce que la règle de
  publication interdit (§5.1).
* **Exposer les neuf modes de LibRaw.** Une énumération dont l'utilisateur ne
  peut pas prédire les éléments n'est pas un réglage, c'est un formulaire.
* **Passer le mode par une option de rendu plutôt que par les réglages de la
  révision.** Il changerait les pixels sans être inscrit dans la révision —
  exactement ce qu'ADR 0044 §3 a corrigé pour `camera_native`.
