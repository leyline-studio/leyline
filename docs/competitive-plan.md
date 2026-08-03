# Plan d'action — justesse du rendu, performance, IA locale

**Document :** `docs/competitive-plan.md`
**Version :** 0.1
**Statut :** Recommandation (entrée de planification, pas une décision)

---

## État de ce document

> Ce document part d'une comparaison à froid, faite le **2026-08-02**, entre
> Leyline et les logiciels établis (Lightroom, Capture One, DxO PhotoLab,
> darktable, RawTherapee). Il cadre **trois axes** de travail et n'en tranche
> aucun : chaque item ci-dessous exige **son propre ADR avant toute ligne de
> code**, conformément à la règle du projet (« aucun code avant l'architecture »).
>
> Il ne remplace ni [`specification.md`](specification.md) — qui dit ce qui est
> livré et ce qui est exclu — ni [`roadmap.md`](roadmap.md), qui dit où en est
> le projet. Il alimente l'un et l'autre.

---

# 1. Objet

Le périmètre V1 et le cadrage V2 sont refermés : ce qui reste n'est plus de la
fonctionnalité manquante. La question devient donc **où Leyline perd face aux
logiciels établis**, ce qui n'est pas la même chose. Trois axes ressortent, et
un seul est un rattrapage de fonctionnalité.

| Axe | Nature du problème | Ce que l'utilisateur ressent |
|---|---|---|
| **A — Justesse du rendu** | Chaîne colorimétrique incomplète | L'image « sort » moins bien à l'ouverture, avant tout réglage |
| **B — Performance** | Travail structurellement redondant, aucun GPU | Chaque curseur traîne |
| **C — IA locale optionnelle** | Absence assumée, devenue un écart de marché | Hauts ISO et masquage restent manuels |

L'axe A décide du verdict au premier coup d'œil ; l'axe B se paie à chaque
seconde d'usage ; l'axe C est un horizon, pas un chantier courant.

---

# 2. Axe A — Justesse du rendu

C'est l'axe le plus rentable : trois manques précis, tous déjà identifiés dans
le dépôt, dont aucun ne demande d'invention.

## A1 — Valider la colorimétrie DCP, puis appliquer les tables

**État.** Le chemin DCP est livré mais **signalé expérimental**
([ADR 0035](adr/0035-camera-profile-dcp.md)) : la justesse n'a jamais été
confrontée à de vrais `.dcp` Adobe et à leurs rendus de référence, et les
tables `ProfileHueSatMapData`, `ProfileLookTableData` et `ProfileToneCurve` ne
sont pas appliquées.

**Pourquoi ça compte.** Ces tables *sont* le rendu Adobe. La matrice seule fait
une conversion correcte ; c'est la `LookTable` qui fait qu'un fichier « a l'air
de sortir de Lightroom ». Tant qu'elles manquent, la comparaison A/B est perdue
d'avance, quelle que soit la qualité du reste du pipeline.

**Découpage.** Deux temps distincts, à ne pas confondre :

1. **Valider l'existant** — protocole de comparaison contre des `.dcp` Adobe et
   des rendus de référence, mesure de l'écart, puis **lever ou confirmer** la
   mention « expérimental ». Ne change aucun pixel s'il n'y a pas de bug.
   **Fait les 2026-08-02/03** — voir le résultat plus bas.
2. **Appliquer les tables manquantes** — change le rendu, donc **nouvelle
   version d'étage** obligatoire (`pipeline.md` §5.1), jamais une modification
   de l'étage publié. **Livré le 2026-08-02** :
   [ADR 0062](adr/0062-dcp-illuminant-interpolation.md) pour l'interpolation des
   illuminants (`camera_profile::v2`) et
   [ADR 0063](adr/0063-dcp-tables.md) pour `HueSatMap`, `LookTable` et
   `ProfileToneCurve` (`camera_profile::v3`).

**Prérequis — et blocage constaté le 2026-08-02.** Il faut des `.dcp` Adobe et
des rendus de référence pour les boîtiers disponibles (les ~17 000 CR2 Canon 60D
réels sont l'échantillon naturel). **Aucun `.dcp` n'existe sur la machine de
développement**, et rien n'y est installé qui en fournisse — ni RawTherapee, qui
en livre habituellement une collection, ni darktable, ni le DNG Converter
d'Adobe. A1.1 est donc **bloquée sur un artefact à apporter**, pas sur du code.

Trois façons de la débloquer, par ordre de préférence :

1. **Installer RawTherapee** et reprendre les `.dcp` qu'il distribue — c'est la
   source la plus simple, et elle donne aussi un second rendu de référence.
2. **Le DNG Converter d'Adobe**, gratuit, qui installe la collection complète
   des profils par boîtier.
3. **Une charte ColorChecker photographiée** : la validation la plus honnête, et
   la seule qui ne dépende d'aucun autre logiciel — on compare les plages rendues
   aux valeurs sRGB de référence de la charte. Ne demande pas de `.dcp` du tout,
   mais demande une prise de vue.

**Piste apportée le 2026-08-02 :** des collections de **profils linéaires**
`.dcp` par marque circulent en téléchargement (par exemple
`olivier-rocq.com/lightroom/profil-lineaire/`), et **Adobe DNG Profile Editor**,
gratuit, en fabrique. Un profil *linéaire* est un cas de validation
particulièrement bien choisi : n'ayant ni `ProfileToneCurve` ni look table, il
n'exerce que le chemin matriciel — exactement ce que Leyline implémente, et rien
de ce qu'il n'implémente pas encore.

Ce que cette piste débloque, et ce qu'elle ne débloque pas :

* ✅ **Fait le 2026-08-02** — et le premier vrai fichier a trouvé un bug
  bloquant : Leyline ne lisait **aucun** `.dcp` authentique. Voir plus bas.
* ✅ **Fait** : un gris neutre du capteur ressort neutre à travers
  caméra→XYZ(D50)→sRGB, sur les deux profils réels. Test permanent, activé par
  `LEYLINE_TEST_DCP`.
* ❌ **La comparaison au rendu d'Adobe.** Elle exige toujours Lightroom ou ACR
  pour produire la référence. Un profil téléchargé ne la remplace pas.

**Le bug trouvé.** Un `.dcp` authentique est une *IFD nue* — un répertoire de
tags, sans aucune image — portant le numéro de version `0x4352` là où TIFF met
42. Le lecteur s'appuyait sur `tiff::Decoder`, qui exige les deux : le 42 et un
`ImageWidth`. Les seules fixtures existantes étant des images TIFF avec des tags
DCP greffés, elles passaient toutes pendant qu'aucun profil réel n'était
lisible. C'est le prix exact d'une suite de tests qui ne dialogue qu'avec
elle-même.

Attention aussi : ces profils sont l'œuvre de tiers, pas les profils d'usine
d'Adobe. Ils valident notre lecture et notre algèbre, pas notre fidélité au
rendu « Camera Standard ».

**Résultat du 2026-08-03.** La référence est arrivée, sous la forme d'un rendu
RawTherapee du même RAW avec le même profil et un profil de traitement neutre.
Une fois le niveau normalisé : **écart médian 0,0027 sur 1,0**, 90ᵉ centile
0,0060, rapports de canaux à 0,007 près. La colorimétrie concorde avec une
implémentation indépendante.

**Le protocole est désormais reproductible sans intervention manuelle** :
`rawtherapee-cli -s` sans sidecar rend avec des valeurs neutres, et un `.pp3`
minimal fixe le profil d'entrée et l'espace de sortie. La référence produite
ainsi reproduit un export fait à la main à 0,001 près.

### L'écart de niveau, le 2026-08-03 : un défaut trouvé, l'écart non refermé

Restait un **gain de ×1,185 en linéaire** : RawTherapee rend plus clair que
Leyline, uniformément. La piste retenue était le niveau de blanc du capteur.

**Elle a mené à un vrai défaut — mais pas à l'explication de l'écart.** Les
deux résultats sont distincts et il faut les lire séparément.

**Le défaut, corrigé ([ADR 0066](adr/0066-sensor-white-level.md)).** Leyline ne
divisait pas par 16 383 comme on le croyait : `adjust_maximum_thr`, un réglage
LibRaw laissé à son défaut de 0,75, abaisse le niveau de blanc jusqu'à
**l'échantillon le plus lumineux de l'image en cours**. Sur quatre fichiers
d'une même série, Canon 60D, ISO 100, même exposition, le niveau retenu était
13 794 pour l'un et 16 383 pour les trois autres — 19 % d'écart de luminosité
selon qu'un reflet est tombé ou non dans le cadre. C'est exactement ce que
`auto_brighten: false` était censé interdire. Le boîtier, lui, écrit la
réponse dans le fichier (`linear_max` : 12 279 à ISO 100, 15 094 à ISO 400,
11 222 ailleurs) — même découpage en groupes d'ISO que la table mesurée de
RawTherapee, et personne ne la lisait. `input::v4` la lit désormais — et un relevé sur 250 fichiers du corpus a montré
au passage que cette métadonnée suit aussi **l'ouverture**, à ~1 % du tableau
`aperture_scaling` que RawTherapee maintient à la main.

**L'écart avec RawTherapee, lui, n'est pas refermé.** Après correction il passe
de ×1,16 à ×1,03 sur le fichier ISO 100, mais **augmente** de ×1,08 à ×1,13 sur
un fichier ISO 400 — là où `v3` étirait le blanc jusqu'au pixel le plus clair
d'une image qui n'en avait pas de très clair. Les diviseurs effectifs des deux
moteurs sont maintenant connus des deux côtés, et **ils n'expliquent pas** le
facteur ~1,14 qui subsiste. Ce n'est donc pas le niveau de blanc, et la
question reste ouverte.

**Le protocole est désormais reproductible sans intervention manuelle** :
`rawtherapee-cli -s` sans sidecar rend avec des valeurs neutres, et un `.pp3`
minimal fixe le profil d'entrée et l'espace de sortie. La référence produite
ainsi reproduit un export fait à la main à 0,001 près.

### L'écart de niveau, expliqué le 2026-08-03

Restait un **gain de ×1,185 en linéaire** : RawTherapee rend plus clair que
Leyline, uniformément. Trois choses avaient été établies — indépendant du
profil caméra, donc pas de la couleur ; un gain et non une courbe ; et le
niveau de blanc du capteur comme piste, sans que les chiffres collent.

**Ils collent maintenant : la piste était la bonne, c'est la comparaison qui
mélangeait deux fichiers.** L'ancien calcul opposait le `linear_max = 11 222`
d'un fichier à la valeur `camconst` d'un autre groupe d'ISO.

Ce que les deux moteurs prennent pour « blanc », sur un Canon 60D :

| Source | ISO 100/125 | ISO 200…3200 | ISO 160/320/640/1250/2500 |
|---|---|---|---|
| LibRaw `maximum` — ce que Leyline divise par | 16 383 | 16 383 | 16 383 |
| LibRaw `linear_max` — métadonnée du boîtier, **ignorée** | 12 279 | 15 094 | 11 222 |
| `camconst.json` de RawTherapee | 13 480 | 15 200 | 12 550 |

Les deux dernières lignes **partagent le même découpage en trois groupes
d'ISO** : ce n'est pas une coïncidence, c'est le même comportement matériel vu
de deux côtés. Leyline, lui, normalise par le plafond théorique du 14 bits, le
même pour tous les fichiers.

**La prédiction, et sa vérification.** Si l'écart n'est que ce choix, il doit
suivre le groupe d'ISO du fichier — et non rester à 1,185 :

| Fichier | ISO | Rapport prédit (16 383 / blanc RT) | Rapport mesuré |
|---|---|---|---|
| IMG_9040 | 100 | 1,215 | **~1,19** |
| IMG_9046 | 400 | 1,078 | **1,085** |

Deux fichiers, deux prédictions différentes, deux mesures qui tombent à moins
de 2 %. **L'écart est expliqué.** Le désactivateur du roll-off des hautes
lumières ne change rien à ce rapport, ce qui écarte au passage notre propre
courbe de sortie comme explication.

**Ce que ça coûte, concrètement.** Un rendu neutre est 8 à 19 % trop sombre
selon la sensibilité, et surtout **un pixel saturé du capteur ne ressort pas
blanc** : à ISO 100 il arrive à 0,82. C'est exactement le « l'image sort moins
bien à l'ouverture » qui ouvre ce document.

**Rien n'est corrigé pour autant** : changer la normalisation déplace tous les
pixels de toutes les photos, et exige donc une **nouvelle version de l'étage
`input`** (`pipeline.md` §5.1) — les révisions existantes continuant de rendre
comme avant jusqu'à un reprocess. Le choix de la source de vérité est une
décision à part entière, avec au moins trois candidats — `linear_max` de la
métadonnée, une table par boîtier et par ISO à la `camconst` (RawTherapee est
GPL-3.0, donc réutilisable ici), ou l'ajustement par le contenu de l'image que
LibRaw propose (`adjust_maximum_thr`, à écarter : deux photos de la même scène
rendraient différemment). **Cela demande son ADR.**

**Darktable ne peut pas servir de troisième avis en l'état** : son rendu par
défaut applique un mappage tonal *scene-referred* (filmic), dont la signature
en S est nette — rapport à 1,43 dans les tons moyens, 0,96 au blanc. Le
comparer demanderait de désactiver ce module.

La mention « expérimental » reste : pour l'absence de comparaison à Adobe
lui-même, et pour ce facteur ~1,14 qui n'est toujours pas attribué. Ce qui est
acquis, c'est que **ce n'est pas la couleur** — la colorimétrie, elle,
concorde.

**Risque.** Faible sur le point 1, moyen sur le point 2 : l'interpolation des
tables `HueSatMap` est un travail de précision, où une erreur passe inaperçue
sur une image de test et saute aux yeux sur une peau.

## A2 — Exposer le choix de l'algorithme de dématriçage — **livré le 2026-08-02**

**État initial.** `params.user_qual` n'était ni exposé ni choisi : on prenait le
défaut de LibRaw. [ADR 0050](adr/0050-highlight-reconstruction.md) §143 laisse
explicitement la question ouverte.

**Pourquoi ça compte.** RawTherapee propose AMaZE, LMMSE, DCB ; le choix se voit
sur le détail fin et les motifs répétitifs (moiré, feuillage, tissu). C'est un
levier de qualité **déjà présent dans la dépendance**, qu'il suffit de piloter.

**Contrainte.** Le dématriçage est en amont de tout le pipeline : l'exposer
change le rendu, donc c'est une nouvelle version de l'étage `input`, et la
valeur retenue doit être écrite dans la révision. Un défaut qui changerait sans
version d'étage casserait `pipeline.md` §5.1.

**Risque.** Faible. Le travail est du câblage et de la validation, pas de
l'algorithmique.

**Livré** par [ADR 0061](adr/0061-demosaic-algorithm.md) : quatre valeurs
nommées (`ahd` par défaut, `vng`, `dcb`, `dht`), écrites dans la révision,
portées par `input::v3`. AMaZE et LMMSE sont absents faute d'être présents dans
la bibliothèque liée — les proposer aurait été proposer un choix qui retombe
silencieusement sur AHD.

## A3 — Profil de bruit mesuré par boîtier et par sensibilité

**État.** Déjà listé comme « décidé, non implémenté »
([ADR 0046](adr/0046-edge-preserving-denoise.md) §7), à trancher par son propre
ADR.

**Pourquoi ça compte.** Le débruitage travaille aujourd'hui sans rien savoir du
capteur. Un profil par boîtier/ISO est ce qui sépare un débruitage correct d'un
débruitage qui préserve le grain fin là où il faut.

**Risque.** Moyen, et surtout **coûteux en données** : il faut mesurer, ou
importer une base existante, avec la question de licence qui va avec.

**Dépendance.** Aucune sur A1/A2. Peut sortir dans n'importe quel ordre.

---

# 3. Axe B — Performance

## Le point de départ, mesuré

[ADR 0041](adr/0041-interactive-preview-rendering.md) chiffre le problème sur un
CR2 réel de **3888×2592** (10 Mpx), machine de référence i9-9900K, 16 threads :

* preview `Small`, neutre : **0,97 s**
* preview `Small`, tous curseurs actifs : **2,39 s**

Les boîtiers courants sont à 45–60 Mpx, soit **4 à 6×** ces temps.

## B1 — Cache d'étages d'ADR 0041 §3 — **livré le 2026-08-02**

**État initial.** ADR 0041 décide **trois** choses : le proxy à résolution
d'affichage (§1), la mise à l'échelle des rayons (§2), et un **cache d'états
intermédiaires** (§3). Les deux premières étaient livrées ; la troisième ne
l'était pas — seul `DecodeCache` existait, qui évite le re-décodage et non le
re-calcul, ce qu'ADR 0041 range lui-même parmi les alternatives insuffisantes.
La roadmap cochait pourtant la phase 7.

**Ce que ça valait.** Chaque rendu repartait du buffer décodé : bouger
`sharpening` (dernier étage, ~13 ms) rejoue dehaze, clarté, texture, TSL et les
réglages locaux à l'identique. C'est **la** différence structurelle avec
Lightroom, Capture One et darktable, qui ne rejouent que l'aval du nœud édité.

**Pourquoi en premier.** La conception était faite et acceptée — points
de contrôle avant les étages chers, `(index d'étage, empreinte des réglages
amont, buffer)`, ~30 Mo par session, chemin preview uniquement. Le cache est
**purement dérivé** : le jeter à tout instant ne change aucun pixel. Donc
**aucun risque de reproductibilité, aucune nouvelle version d'étage, aucune
dépendance nouvelle**. C'est le meilleur rapport gain/risque de tout ce
document.

**Livré.** Mesuré à **−78 %** sur un curseur de fin de pipeline (~60 ms → ~14 ms,
carte 1024×683, `--release`). Deux choses que l'implémentation a apprises et qui
sont consignées dans ADR 0041 : le cache vit sur la `Library`, pas sur la session
d'édition — la vue develop rend par `Library::preview` — et un seuil de point de
contrôle désigne une position, pas un rang exact, sans quoi trois des quatre
points ne sont jamais pris. Le prérequis réel était d'apprendre à chaque étage
quels réglages il lit (`Stage::reads`), ce qu'aucun ADR n'avait posé.

## B2 — Le chemin export et impression — **mesuré le 2026-08-03**

**État.** ADR 0041 exclut explicitement l'export et l'impression de ses
optimisations : pleine résolution, sans cache d'étages, **bit pour bit
identiques**. C'était le bon arbitrage pour un ADR centré sur l'interactif —
mais il laissait le chemin export sans un seul chiffre.

**Mesuré.** `crates/leyline-engine/benches/export.rs`, i9-9900K 16 threads
(la machine de référence d'ADR 0041), `--release`. Les trois coûts d'un export
sont pesés séparément, parce qu'ils ne se comportent pas pareil :

| Étape | 10 Mpx | 45 Mpx | Parallèle ? |
|---|---|---|---|
| Décodage LibRaw, pleine taille | **0,85 s** | 2,76 s à 30 Mpx (mesuré sur un 5D IV) | oui |
| Rendu, révision neutre | 0,045 s | **0,19 s** | oui (~8,7×) |
| Rendu, édition complète | 1,12 s | **4,33 s** | oui (~8,7×) |
| Encodage WebP | — | **0,61 s** | **non** |
| Encodage JPEG | — | **0,87 s** | **non** |
| Encodage TIFF | — | **1,62 s** | **non** |
| Encodage PNG | — | **2,55 s** | **non** |
| Encodage AVIF | 5,05 s | **21,8 s** | oui (~7,5×) |

Tout est **linéaire en pixels** : ×4,3 de surface donne ×3,9 sur le rendu, ×4,3
sur l'AVIF, ×3,3 sur le décodage. Rien ne s'effondre à la montée en taille, et
rien ne profite non plus d'un effet d'échelle.

**Ce que ça donne bout à bout.** Un fichier 30 Mpx, édition complète, JPEG :
2,8 s de décodage + 2,9 s de rendu + 0,6 s d'encodage ≈ **6,3 s**. Le lot de
500 fichiers évoqué plus haut prend donc **~52 minutes**. En AVIF, le même lot
passe à **~3 heures**, l'encodage devenant à lui seul les trois quarts du temps.

**Trois constats, dans l'ordre où ils comptent :**

1. **L'AVIF est hors norme** — 25× le coût du JPEG à taille égale. La vitesse
   d'encodage `ravif` est figée à `speed(6)` dans le code, sans que rien ne
   l'expose ni ne le documente. C'est le seul réglage du document qui pourrait
   diviser un temps par trois sans toucher à un pixel du rendu.
2. **Les encodeurs rapides sont mono-thread** (JPEG, PNG, TIFF, WebP : temps
   utilisateur ≈ temps réel), pendant que le lot traite **un fichier à la
   fois**. Sur 16 cœurs, chaque encodage laisse donc 15 cœurs inoccupés — de
   l'ordre de 10 à 15 % du temps d'un lot JPEG. Recouvrir l'encodage du fichier
   *n* avec le rendu du *n+1* est le gain structurel évident, et il ne change
   aucun pixel : c'est de l'ordonnancement, pas du calcul.
3. **Le rendu domine et il est déjà parallèle** (~8,7× sur 16 threads). Il n'y
   a pas de gaspillage à récupérer là sans changer les opérateurs eux-mêmes —
   et le cache d'étages de B1 est interdit ici par la promesse §5.1.

### Face à RawTherapee et darktable

Des chiffres absolus ne disent pas si l'export est lent — seulement combien il
prend. Même machine, mêmes fichiers, même sortie JPEG q90, RAW → fichier de
bout en bout (décodage compris), le 2026-08-03 :

| Fichier | Traitement | Leyline | RawTherapee 5.12 | darktable 5.6 |
|---|---|---|---|---|
| 30 Mpx (5D IV) | neutre / défaut | **2,88 s** | 3,53 s | 5,44 s |
| 10 Mpx (60D) | neutre / défaut | **0,90 s** | 1,10 s | 1,70 s |
| 30 Mpx | édition comparable | 5,79 s | **5,69 s** | — |
| 10 Mpx | édition comparable | **1,92 s** | 2,02 s | — |

L'« édition comparable » applique des deux côtés balance des blancs, exposition,
contraste, hautes lumières, ombres, noirs, vibrance, débruitage, accentuation,
rotation et recadrage ; darktable en est absent faute d'un XMP équivalent, son
rendu par défaut faisant déjà tourner sa chaîne *scene-referred* complète.

**Le verdict est bon, et il n'était pas acquis** : sur le chemin neutre Leyline
est le plus rapide des trois, et de loin le plus économe — 6,8 s de CPU là où
RawTherapee en consomme 17,7 pour le même fichier. Chargé de réglages, l'écart
avec RawTherapee tombe à 2 % (5,79 s contre 5,69), toujours avec ~18 % de CPU
en moins. **Il n'y a donc pas de retard de performance à rattraper sur
l'export.** Ce document supposait le contraire.

### L'exception AVIF, et ce qu'elle coûte vraiment

darktable exporte le même 30 Mpx en AVIF en **3,5 s**, là où Leyline met
**14,7 s** — mais son fichier pèse 8,4 Mo contre 0,98 Mo pour le nôtre. Ce
n'est donc pas le même travail, et l'écart brut ne prouve rien.

Ce qui prouve quelque chose, c'est de bouger notre propre curseur. Le même
export, `ravif` réglé sur trois vitesses :

| `with_speed` | Temps | Fichier |
|---|---|---|
| 6 (valeur figée aujourd'hui) | 14,7 s | 0,98 Mo |
| 9 | 11,1 s | 0,99 Mo |
| 10 | **6,6 s** | 1,12 Mo |

Passer de 6 à 9 rend **25 % du temps pour 1 % de poids** ; passer à 10 rend
**55 % du temps pour 14 %**. Une valeur figée dans le code décide donc seule de
cet arbitrage, sans que personne puisse le voir ni le changer. C'est le meilleur
rapport gain/effort qui reste dans tout ce document.

**Ce qui n'est pas mesuré :** l'impression (chemin PDF), et le coût mémoire
d'un pipeline 45 Mpx, qui décidera de la profondeur du pipelinage envisagé au
point 2.

**Suites possibles, chacune avec son ADR :** exposer la vitesse d'encodage AVIF
(et son défaut, que la mesure ci-dessus place plutôt vers 9 ou 10), et pipeliner
le lot d'export. Aucune des deux ne touche à la reproductibilité : la première
ne concerne que le codec, la seconde que l'ordre d'exécution.

## B3 — GPU : rouvrir la question, sur le chemin preview seul

**État.** [ADR 0012](adr/0012-rayon-data-parallelism.md) a écarté wgpu — « gains
supérieurs mais déterminisme inter-GPU non garanti » — en reportant à une
exploration ultérieure. ADR 0041 a refusé de rouvrir, en renvoyant à un ADR
propre **après mesure des optimisations CPU**.

**L'argument qui reste valable.** Le déterminisme inter-GPU est réel : c'est ce
que `pipeline.md` §5.1 refuse de laisser entrer dans le résultat.

**L'argument qui rend la question rouvrable.** La promesse §5.1 porte sur le
**rendu**, c'est-à-dire ce que produisent l'export et l'impression. La preview
est déjà un chemin séparé, déjà non bit-à-bit avec l'export (proxy réduit,
rayons mis à l'échelle), et déjà purement dérivé. **Un chemin GPU preview seul,
CPU faisant foi à l'export, ne toucherait donc pas à la promesse** — c'est
exactement la séparation qu'ADR 0041 a déjà instaurée pour d'autres raisons.

**Condition d'entrée.** Ne rouvrir qu'**après B1**, avec les mesures
d'après-cache en main, comme ADR 0041 le demande. Si B1 suffit à rendre
l'interaction fluide, le GPU ne se justifie plus au prix d'une dépendance et
d'un second chemin de rendu à maintenir.

**Réponse au 2026-08-03 : la condition n'est pas remplie, et le GPU ne se
justifie pas.** Trois mesures le disent :

* **B1 a rendu l'interaction fluide** — ~14 ms sur un curseur de fin de
  pipeline, soit sous le seuil où l'œil voit une latence. Il n'y a plus de
  gêne à supprimer sur le chemin où le GPU serait autorisé.
* **Sur l'export, le GPU est interdit** par la promesse `pipeline.md` §5.1, et
  c'est précisément le chemin qui prend des secondes. Un GPU qui ne peut pas
  toucher au seul endroit qui coûte cher ne règle rien.
* **Il n'y a pas de retard à rattraper** : à réglages comparables, Leyline
  exporte aussi vite que RawTherapee et plus vite que darktable, tous deux sur
  CPU eux aussi (voir B2). Le concurrent qu'on voudrait rattraper au GPU ne
  l'utilise pas non plus.

Et là où du temps est réellement gaspillé — 15 cœurs inoccupés pendant chaque
encodage — la réponse est l'ordonnancement, pas un second processeur. **À
reprendre si un jour l'interaction redevient le point douloureux**, pas avant.

---

# 4. Axe C — IA locale optionnelle

## Ce qui ne change pas

L'absence d'IA dans la V1 était **le contrat**, et ce document ne le renie pas.
[`specification.md`](specification.md) §4 classe l'IA en exclusion volontaire, en
laissant une porte : « une IA locale optionnelle reste envisageable à très long
terme » — porte que [`roadmap.md`](roadmap.md) §Long terme reprend. Le présent
axe **cadre cette porte**, il ne l'ouvre pas.

## Les conditions non négociables

Aucune de ces conditions n'est une préférence : chacune découle d'un principe
déjà écrit. Un projet d'IA qui en viole une seule est à refuser.

1. **Entièrement local.** Aucune inférence distante, aucun appel réseau, jamais
   — Local First ([`vision.md`](vision.md)).
2. **Optionnel et désinstallable.** Leyline doit rester complet et cohérent sans
   les modèles. Pas de fonctionnalité de base qui en dépende.
3. **Zéro télémétrie.** Rien ne sort de la machine, pas même un compteur d'usage.
4. **Déterminisme, ou aveu explicite.** C'est le point dur. Un modèle produit un
   résultat qui dépend du backend d'inférence, de la précision et du matériel —
   soit exactement ce que `pipeline.md` §5.1 refuse. Deux issues, à trancher
   dans l'ADR : soit **le poids du modèle et le backend sont épinglés dans la
   version d'étage** au même titre qu'une constante, soit le résultat IA est
   **matérialisé une fois** (un masque rasterisé, stocké dans la révision) et
   c'est ce résultat figé, non le modèle, que le pipeline rejoue.
   **La seconde issue est la seule qui tienne** la promesse telle qu'elle est
   écrite aujourd'hui.
5. **Licence compatible GPL-3.0**, poids du modèle compris. Beaucoup de modèles
   publiés ne le sont pas, et c'est un critère éliminatoire en amont du reste.
6. **Poids distribués séparément.** Un installateur ne peut pas gonfler de
   plusieurs centaines de mégaoctets pour une fonctionnalité optionnelle.

## C1 — Débruitage IA

**L'écart.** Lightroom Denoise et DxO DeepPRIME sont devenus *le* différenciateur
qualité sur les hauts ISO. Le débruitage par ondelettes
([ADR 0046](adr/0046-edge-preserving-denoise.md)) ne joue pas dans cette
catégorie, et aucun réglage ne l'y amènera.

**La difficulté propre.** Le débruitage agit **sur les pixels**, donc la
condition 4 mord de plein fouet : impossible de « matérialiser une fois » un
débruitage comme on matérialise un masque, sans stocker une image intermédiaire
— ce que le contrat de non-destructivité évite précisément. C'est le sujet le
plus difficile de tout ce document, et le seul dont je ne vois pas de solution
propre à ce stade.

**Recommandation.** Ne pas l'attaquer en premier. A3 (profil de bruit mesuré)
donne une partie du gain, sans aucune de ces questions.

## C2 — Masques automatiques (sujet, ciel, arrière-plan)

**L'écart.** C'est ce que les gens utilisent réellement pour les retouches
locales depuis 2021. Leyline a masques géométriques + masques par plage
([ADR 0048](adr/0048-range-masks.md)) : l'état de l'art d'avant cette bascule.

**Pourquoi c'est le bon premier candidat.** Un masque **est** matérialisable :
le modèle tourne une fois, produit un masque, ce masque est stocké dans la
révision, et le pipeline ne rejoue plus jamais le modèle. La condition 4 est
satisfaite par construction, sans compromis. L'infrastructure de masquage
existe déjà ([ADR 0029](adr/0029-process-6-local-adjustments.md),
[ADR 0049](adr/0049-local-adjustments-clients.md)) : l'IA ne serait qu'une
**source de masque de plus**, à côté de la brosse et du dégradé.

**Reste ouvert.** Le choix du modèle et du runtime, la licence des poids, et le
mode de distribution.

---

# 5. Séquencement recommandé

L'ordre suit le rapport **gain ressenti / risque**, pas la difficulté.

| # | Item | Pourquoi ici | Nouvelle version d'étage ? |
|---|---|---|---|
| ~~1~~ | ~~**B1** — cache d'étages~~ | **Livré le 2026-08-02, −78 %** | Non |
| ~~2~~ | ~~**A1.1** — valider le DCP~~ | **Fait le 2026-08-02/03** : bug du conteneur corrigé, algèbre validée contre RawTherapee (écart médian 0,0027) ; « expérimental » maintenu pour le gain ×1,185 inexpliqué | Non |
| ~~3~~ | ~~**A2** — choix du dématriçage~~ | **Livré le 2026-08-02** ([ADR 0061](adr/0061-demosaic-algorithm.md)) | Oui (`input::v3`) |
| ~~4~~ | ~~**A1.2** — appliquer les tables DCP~~ | **Livré le 2026-08-02** ([ADR 0062](adr/0062-dcp-illuminant-interpolation.md), [ADR 0063](adr/0063-dcp-tables.md)) | Oui (`camera_profile::v2`, `v3`) |
| ~~5~~ | ~~**B2** — mesurer l'export~~ | **Mesuré le 2026-08-03**, voir §3 : bench `export.rs`, deux suites possibles identifiées | Non |
| 6 | **A3** — profil de bruit | Coûteux en données ; donne une partie du gain visé par C1 | Oui |
| ~~7~~ | ~~**B3** — GPU preview~~ | **Écarté le 2026-08-03** : B1 a rendu l'interaction fluide, le GPU est interdit à l'export par §5.1, et la comparaison montre qu'il n'y a rien à rattraper (voir §3) | — |
| 8 | **C2** — masques IA | Horizon ; seul item IA compatible avec §5.1 sans compromis | Non (masque matérialisé) |
| 9 | **C1** — débruitage IA | Horizon lointain ; question de déterminisme non résolue | À trancher |

**Dépendances dures :** A1.2 après A1.1 ; B3 après B1 (exigé par ADR 0041).
Tout le reste peut sortir dans n'importe quel ordre.

**Reste ouvert au 2026-08-03 :** A3 (profil de bruit), B3 (GPU preview, à ne
rouvrir que si B1 ne suffit pas), C2 puis C1, et les deux suites de B2 —
vitesse d'encodage AVIF, et pipelinage du lot d'export.

---

# 6. Avant toute ligne de code

Chaque item de ce tableau exige un ADR qui lui est propre. Le document présent
n'en tient lieu pour aucun.

| Item | Ce que l'ADR doit trancher |
|---|---|
| A1.2 | Interpolation des tables, ordre d'application, nouvelle version d'étage |
| A2 | Algorithme par défaut, valeurs exposées, écriture dans la révision |
| A3 | Origine des mesures et leur licence, format de stockage |
| B1 | Rien — [ADR 0041](adr/0041-interactive-preview-rendering.md) §3 est déjà l'ADR. **Implémenté le 2026-08-02** |
| B2 (suites) | Vitesse d'encodage AVIF exposée et son défaut ; recouvrement encodage/rendu dans un lot, et ce que devient l'ordre du rapport |
| B3 | Périmètre preview-seul, backend, et ce que devient §5.1 dans le texte |
| C1, C2 | Les six conditions du §4 ci-dessus, modèle, runtime, distribution des poids |

---

## Documents liés

* [`specification.md`](specification.md) — ce qui est livré, ce qui est exclu.
* [`roadmap.md`](roadmap.md) — l'état réel, phase par phase.
* [`pipeline.md`](pipeline.md) §5 — la promesse de reproductibilité, que tout
  item de ce document doit respecter ou amender explicitement.
* [`v2-implementation-plan.md`](v2-implementation-plan.md) — le précédent
  document de séquencement, désormais archive.
