# ADR 0042 — Pipeline composé d'étages versionnés : le gel porte sur l'opérateur, plus sur la version entière

**Statut :** Accepté — 2026-07
**Remplace :** ADR 0028 (une process version par fonctionnalité, duplication par module)

## Contexte

ADR 0028 a figé la convention actuelle : une process version par fonctionnalité
pixel, chacune dans son propre module `processN.rs` **copie intégrale** du
précédent. Sa dernière conséquence prévoyait explicitement sa propre
réouverture :

> Si le nombre de modules devenait un jour un vrai fardeau — un nombre bien
> supérieur aux cinq actuels, après que plusieurs fonctionnalités V2 ont
> réellement été livrées — un futur ADR pourra rouvrir la question **avec des
> données réelles**.

Ces données existent maintenant.

**Volume.** ADR 0028 mesurait 3 433 lignes sur cinq modules. Aujourd'hui :

| Module | Lignes | | Module | Lignes |
|---|---|---|---|---|
| `process1.rs` | 471 | | `process7.rs` | 1 384 |
| `process2.rs` | 580 | | `process8.rs` | 1 633 |
| `process3.rs` | 750 | | `process9.rs` | 2 048 |
| `process4.rs` | 804 | | `process10.rs` | 2 556 |
| `process5.rs` | 954 | | `process11.rs` | 2 671 |
| `process6.rs` | 1 117 | | **Total** | **14 968** |

**Taux de duplication.** Chaque module est identique à son prédécesseur à
**70–93 %** (lignes strictement identiques) — `process11.rs` l'est à 93 % de
`process10.rs` : 2 482 lignes sur 2 671.

**Nature réelle des versions livrées.** Le tableau de `docs/pipeline.md` §3.3
est sans ambiguïté : **dix des onze versions** sont définies par la formule
« Identique à N−1, **plus** X ». Une seule — process 2, les fonctions de
transfert par table (ADR 0013) — a modifié le rendu de réglages *déjà
existants*. Autrement dit : **dix copies complètes du pipeline ont été payées
pour des changements qui ne pouvaient affecter aucune photo existante.** Une
photo sans valeur `dehaze` rend rigoureusement pareil que `dehaze` existe ou
non dans le moteur.

**Coût de maintenance, constaté.** ADR 0041 (rendu proxy) a dû faire descendre
un unique facteur d'échelle jusqu'aux rayons exprimés en pixels. Le même
changement de trois lignes a dû être appliqué **onze fois**, plus 33 appels de
test inter-modules et onze blocs de documentation. Le code de netteté de
`process3.rs` est octet pour octet celui de `process9.rs` : le modifier onze
fois n'apporte aucune garantie que le modifier une fois n'apporterait pas.

**Ce qui n'est pas en cause.** La promesse elle-même — *ce RAW, ces réglages,
ces pixels, dans dix ans* (`docs/pipeline.md` §3.3, §5) — n'est ni affaiblie ni
renégociée ici. Elle est **comportementale**. La duplication intégrale n'en
était qu'une *implémentation possible*, jamais son énoncé.

## Décision

Le pipeline cesse d'être une suite de modules-versions dupliqués. Il devient la
**composition d'étages versionnés indépendamment**.

### 1. L'unité de gel est l'opérateur, pas le pipeline

Chaque opérateur vit dans son propre module versionné — `sharpen/v1.rs`,
`dehaze/v1.rs`, `tone_curve/v1.rs` — **gelé le jour où il sort**, exactement
comme un `processN.rs` l'est aujourd'hui.

C'est le point qui répond à l'objection décisive d'ADR 0028 (« du code partagé
est précisément ce qui expose un rendu gelé au risque qu'un changement futur,
sans rapport, l'altère silencieusement »). Cette objection vise le **partage
d'une implémentation mutable** entre plusieurs versions. Ce n'est pas ce que
décrit le présent ADR : `sharpen::v1` n'est pas une implémentation partagée et
modifiable, c'est un module figé au même titre que `process3.rs`. Corriger la
netteté produit `sharpen::v2` ; `v1` n'est **jamais** touché. La garantie reste
mécaniquement infalsifiable, à l'identique — seule disparaît la re-congélation
de onze copies d'un opérateur qui n'a pas changé.

### 2. Une révision enregistre la version des étages qu'elle utilise

Le champ `process` cesse d'être l'axe de versionnage. Une révision enregistre
la version de chaque étage **effectivement actif** :

```json
"stages": { "exposure": 1, "tone_curve": 1, "dehaze": 1 }
```

Un étage **neutre ne s'exécute pas** — c'est déjà le comportement du moteur,
chaque étage étant sauté quand son réglage vaut sa valeur neutre. Il n'a donc
aucun comportement à épingler et **n'apparaît pas** dans la carte. Celle-ci est
par construction proportionnelle à l'édition réelle : trois entrées pour une
photo peu retouchée, une quinzaine pour une photo très travaillée.

Ce choix rend la révision **auto-descriptive** : rien n'est déduit d'une table
de correspondance côté moteur, donc aucun compteur global ne se réintroduit par
la porte de derrière.

### 3. La position dans le pipeline est une propriété de la version d'étage

L'ordre des étages est un état observable : presque toutes nos fonctionnalités
se sont insérées **au milieu** du pipeline. Chaque version d'étage déclare donc
son propre rang (`sharpen::v1` au rang 90). Déplacer un étage n'est pas une
modification d'une version existante mais une **nouvelle version** qui déclare
un autre rang — les révisions référençant `v1` conservent le rang 90.

L'ordre redevient ainsi reproductible **sans** version globale de disposition.

### 4. Ajouter une fonctionnalité ne touche rien d'existant

Un nouvel étage = un module + une entrée au registre. Aucune copie. Les photos
existantes ne mentionnent pas ce nom dans leur carte `stages`, donc l'étage
n'existe pas dans leur pipeline : leur rendu est inchangé **par construction**,
et non parce qu'on a pris soin de ne pas toucher leur module.

### 5. Les révisions existantes continuent d'être rendues à l'identique

`process: N` reste lu et compris : chaque N possède une **expansion figée et
déterministe** vers un ensemble de versions d'étages, écrite une fois dans une
table de compatibilité. Le champ devient une abréviation historique et un
libellé d'affichage (« cette photo utilise un process ancien », comme le PV de
Lightroom), plus un axe qui croît.

### 6. Rien ne migre sans preuve : les rendus de référence d'abord

**Aucune ligne n'est refactorisée avant que des rendus de référence n'existent.**
La migration procède dans cet ordre, strictement :

1. Capturer, depuis le code **actuel**, un rendu de référence par process
   version (1 à 11) sur des images de test déterministes couvrant chaque
   opérateur, et les committer comme fixtures.
2. Refactoriser vers les étages versionnés.
3. Prouver l'égalité **bit à bit** contre ces fixtures, pour les onze versions.

Ce sont les fixtures — pas la relecture du diff — qui établissent qu'une
révision de 2026 rend en 2036 ce qu'elle rendait en 2026. Elles restent dans la
suite de tests après la migration, comme garde permanent.

## Conséquences

* **La duplication disparaît sans que la promesse bouge.** ~15 000 lignes de
  pipeline se ramènent aux opérateurs réellement distincts, plus onze
  déclarations. Un correctif d'opérateur s'écrit une fois, au lieu d'être
  appliqué onze fois comme sous ADR 0041.
* **Le coût d'une nouvelle fonctionnalité pixel devient constant** au lieu de
  croître avec le nombre de versions déjà livrées. La prochaine (les tables
  DCP, ADR 0037) sera un étage, pas une douzième copie de 2 700 lignes.
* **`settings_json` change de forme** : c'est le contrat de reproductibilité
  lui-même qui est amendé. Ce n'est acceptable **que** parce que le projet est
  pré-publication ; après ouverture au monde, cette forme JSON serait
  définitive. C'est la raison de faire ce changement maintenant et pas plus
  tard.
* **La correction de l'expansion `process: N` devient critique** : une
  expansion fausse rendrait différemment une photo ancienne. C'est exactement
  ce que les fixtures de l'étape 6 vérifient, version par version.
* **Le nombre de versions d'étages peut croître**, lui — mais seulement pour
  les opérateurs réellement corrigés, pas pour les onze copies de ceux qui ne
  l'ont pas été. Sur l'historique réel, cela aurait produit une `v2` pour les
  quelques opérateurs touchés par ADR 0013, et **aucune** autre re-version.
* **`docs/pipeline.md` §3.3 est réécrit** : le tableau des process versions
  devient une table de compatibilité historique, et la section de versionnage
  décrit les étages.
* **ADR 0028 est remplacé, non annulé rétroactivement** : son raisonnement
  était correct pour les données dont il disposait (cinq modules, 3 433
  lignes), et il avait lui-même prévu sa réouverture sur données réelles.

## Alternatives écartées

* **Conserver ADR 0028 tel quel** : la trajectoire n'est pas « linéaire et
  prévisible » comme il l'espérait — elle est linéaire en *nombre de modules*
  mais quadratique en *lignes cumulées*, chaque module étant plus gros que le
  précédent. De 3 433 à 14 968 lignes pour six fonctionnalités livrées.
* **Bibliothèque d'opérateurs partagés mutables** (ce qu'ADR 0028 écartait
  vraiment) : toujours écarté, et pour sa raison d'origine. Un opérateur unique
  et modifiable utilisé par toutes les versions rendrait un rendu gelé
  falsifiable. Les étages **versionnés et figés** ne sont pas cela.
* **Garder un compteur global de disposition** en plus des versions d'étages :
  redondant. Le rang porté par la version d'étage suffit, et un compteur global
  recommencerait à croître à chaque insertion — le problème même qu'on retire.
* **Écrire la carte `stages` complète sur chaque révision**, étages neutres
  compris : verbeux sans rien garantir de plus. Un étage neutre ne s'exécute
  pas ; épingler la version d'un code qui ne tourne pas ne pin rien.
* **Déduire les versions d'étages d'une « ligne de base moteur » enregistrée
  par révision** : c'est un compteur global déguisé, avec l'inconvénient
  supplémentaire de rendre la révision non auto-descriptive.
* **Migrer sans rendus de référence, en relisant le diff** : la seule partie du
  système où « ça devrait aller » n'est pas un critère acceptable.
