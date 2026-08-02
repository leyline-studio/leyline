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
2. **Appliquer les tables manquantes** — change le rendu, donc **nouvelle
   version d'étage** obligatoire (`pipeline.md` §5.1), jamais une modification
   de l'étage publié.

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

Tant que rien de tout cela n'est disponible, la mention « expérimental » reste,
et c'est le comportement correct : elle dit exactement ce qui n'a pas été
vérifié.

**Risque.** Faible sur le point 1, moyen sur le point 2 : l'interpolation des
tables `HueSatMap` est un travail de précision, où une erreur passe inaperçue
sur une image de test et saute aux yeux sur une peau.

## A2 — Exposer le choix de l'algorithme de dématriçage

**État.** `params.user_qual` n'est ni exposé ni choisi : on prend le défaut de
LibRaw. [ADR 0050](adr/0050-highlight-reconstruction.md) §143 laisse
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

## B2 — Le chemin export et impression

**État.** ADR 0041 exclut explicitement l'export et l'impression de ses
optimisations : pleine résolution, sans cache d'étages, **bit pour bit
identiques**. C'était le bon arbitrage pour un ADR centré sur l'interactif.

**Ce qui reste.** Aucune mesure n'existe sur un export pleine résolution de
fichier moderne. Un export par lots de 500 fichiers 45 Mpx est un usage réel et
non chiffré. **Premier pas : mesurer**, avant de décider quoi que ce soit.

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
| 2 | **A1.1** — valider le DCP | Borné, déjà listé comme ouvert, décide du verdict visuel | Non (si pas de bug) |
| 3 | **A2** — choix du dématriçage | Câblage d'une capacité déjà présente dans LibRaw | Oui (`input`) |
| 4 | **A1.2** — appliquer les tables DCP | Le vrai gain colorimétrique, mais demande A1.1 d'abord | Oui |
| 5 | **B2** — mesurer l'export | Aucune décision possible sans chiffres | Non |
| 6 | **A3** — profil de bruit | Coûteux en données ; donne une partie du gain visé par C1 | Oui |
| 7 | **B3** — GPU preview | À rouvrir seulement si B1 ne suffit pas | Non (chemin preview) |
| 8 | **C2** — masques IA | Horizon ; seul item IA compatible avec §5.1 sans compromis | Non (masque matérialisé) |
| 9 | **C1** — débruitage IA | Horizon lointain ; question de déterminisme non résolue | À trancher |

**Dépendances dures :** A1.2 après A1.1 ; B3 après B1 (exigé par ADR 0041).
Tout le reste peut sortir dans n'importe quel ordre.

---

# 6. Avant toute ligne de code

Chaque item de ce tableau exige un ADR qui lui est propre. Le document présent
n'en tient lieu pour aucun.

| Item | Ce que l'ADR doit trancher |
|---|---|
| A1.2 | Interpolation des tables, ordre d'application, nouvelle version d'étage |
| A2 | Algorithme par défaut, valeurs exposées, écriture dans la révision |
| A3 | Origine des mesures et leur licence, format de stockage |
| B1 | Rien — [ADR 0041](adr/0041-interactive-preview-rendering.md) §3 est déjà l'ADR. Reste à l'implémenter |
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
