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

**Ce qui l'est, en revanche : sa portée.** Le §5 d'origine promettait un
résultat identique « au pixel près » sans nommer la plateforme. Or le pipeline
appelle `powf`, `ln` et `exp`, qui sortent de la libm du système : leur dernier
bit change d'une plateforme, d'une version de libm ou de LLVM à l'autre. La
promesse était donc, telle qu'écrite, intenable — non par défaut de rigueur,
mais parce qu'aucun moteur ne la tient : Lightroom ne donne pas les mêmes
pixels sur ses chemins GPU et CPU, et darktable migre les paramètres des
anciens modules vers le code courant (`legacy_params`) au lieu de geler ce
code. §5 est donc rescindé en deux : ce qui est garanti (§5.1) et ce qui ne
l'est pas (§5.2).

Il faut souligner que ces deux relâchements sont **indépendants**, et qu'un
seul est retenu. Renoncer à l'exactitude inter-plateforme est *forcé* par la
virgule flottante. Renoncer au gel du code, à la manière de darktable, serait
un *choix* — et le présent ADR le rend inutile : ce qui rendait le gel coûteux
était la copie de 2 700 lignes par fonctionnalité, pas le gel lui-même. Une
fois les étages composés, `sharpen::v2` pèse quelques dizaines de lignes à côté
de `sharpen::v1`. On abandonne donc la garantie physiquement impossible, et on
conserve celle qui ne coûte plus grand-chose.

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

> **Sans objet depuis [ADR 0043](0043-collapse-prerelease-render-history.md).**
> Ce paragraphe a été appliqué tel quel (table d'expansion des onze versions,
> égalité bit à bit prouvée, commit `bf63df1`), puis retiré : Leyline n'ayant
> pas été publié, ces onze versions n'engageaient personne. L'historique de
> rendu est effondré sur une version par opérateur et `process` disparaît au
> profit de la carte `stages` du §2. Le reste du présent ADR est intact.

`process: N` reste lu et compris : chaque N possède une **expansion figée et
déterministe** vers un ensemble de versions d'étages, écrite une fois dans une
table de compatibilité. Le champ devient une abréviation historique et un
libellé d'affichage (« cette photo utilise un process ancien », comme le PV de
Lightroom), plus un axe qui croît.

### 6. La version d'étage, et non la version applicative, porte la garantie

Le champ `process` n'était pas seulement l'axe de versionnage du rendu : il
était aussi la seule échelle à laquelle la promesse savait s'énoncer. Elle
s'énonce désormais par étage :

> **Aucune version publiée — correctif, mineure ou majeure — ne modifie le
> rendu d'une version d'étage déjà publiée.** Si le rendu doit changer, c'est
> une nouvelle version d'étage ; les révisions existantes continuent de citer
> l'ancienne.

Un changement de rendu n'est donc **jamais** un incrément de version de
l'application : c'est un nouvel étage. La version applicative et l'identité du
rendu sont décorrélées — Leyline 1.0.3 et Leyline 7.2.0 rendent `sharpen::v1`
à l'identique, puisque c'est le même code gelé dans les deux binaires. C'est
aussi la bonne échelle côté utilisateur : sa révision nomme les versions
d'étages qu'elle utilise, alors qu'il ignore quel build a produit ses pixels.

Le profil de compilation ne fait pas non plus partie de l'équation : les
rendus de référence de §7 passent à l'identique en `debug` et en `release`
(Rust n'active ni *fast-math* ni la contraction FMA, et la vectorisation
automatique n'a pas le droit de réassocier une réduction flottante).

Reste une entrée que personne ne contrôle en écrivant du code : la chaîne de
compilation. `rust-toolchain.toml` est donc épinglé sur une **version exacte**
plutôt que sur `stable` — sinon un `rustup update` avant une publication de
correctif suffirait à déplacer des pixels. En changer impose de rejouer les
rendus de référence et de consigner la dérive.

### 7. Rien ne migre sans preuve : les rendus de référence d'abord

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
  ce que les fixtures de l'étape 7 vérifient, version par version.
* **Le nombre de versions d'étages peut croître**, lui — mais seulement pour
  les opérateurs réellement corrigés, pas pour les onze copies de ceux qui ne
  l'ont pas été. Sur l'historique réel, cela aurait produit une `v2` pour les
  quelques opérateurs touchés par ADR 0013, et **aucune** autre re-version.
* **`docs/pipeline.md` §3.3 est réécrit** : le tableau des process versions
  devient une table de compatibilité historique, et la section de versionnage
  décrit les étages.
* **`docs/pipeline.md` §5 est scindé** en ce qui est garanti (§5.1, avec la
  règle de publication ci-dessus) et ce qui ne l'est pas (§5.2, la dérive
  inter-plateforme). Le projet énonce désormais une promesse qu'il tient
  intégralement, au lieu d'une promesse plus large qu'il tenait en partie.
* **La chaîne de compilation devient une entrée versionnée du rendu** :
  `rust-toolchain.toml` est épinglé sur une version exacte, et en changer
  devient un acte qui impose de rejouer les rendus de référence. C'est la seule
  variable capable de déplacer des pixels sans qu'une ligne de code bouge.
* **La version applicative cesse de porter quoi que ce soit sur le rendu** :
  elle peut suivre le semver ordinaire (fonctionnalités, correctifs, interface)
  sans que la question « est-ce que cette publication change des pixels ? » se
  pose jamais. La réponse est structurellement non.
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
* **Adopter aussi le `legacy_params` de darktable** — convertir les paramètres
  des anciennes versions d'étages vers le code courant plutôt que de geler
  l'ancien code. C'est le modèle des deux références du domaine, et il ne coûte
  rien en lignes conservées ; il a été examiné sérieusement, puis écarté. La
  raison n'est pas doctrinale : c'est que le présent ADR lui retire son
  intérêt. Ce qui rendait le gel coûteux était la copie intégrale du pipeline,
  pas le gel ; une fois les étages composés, geler revient à laisser vivre
  quelques dizaines de lignes qui ne demanderont plus jamais d'attention. On
  échangerait la seule garantie qui distingue Leyline de Lightroom et de
  darktable contre quelques centaines de lignes par décennie. Si le calcul
  devait un jour s'inverser, l'échappatoire reste ouverte **et mesurable** :
  la structure d'ADR 0042 accueille les deux sémantiques, et les rendus de
  référence de §7 diraient exactement ce qu'un tel basculement coûterait, au
  pixel près.
* **Garder `docs/pipeline.md` §5 tel quel** (« identique au pixel près », sans
  mention de la plateforme) : intenable. Une promesse invérifiable sur une
  autre machine n'est pas une promesse plus forte, c'en est une plus fragile —
  la première dérive de libm constatée par un utilisateur la démolirait tout
  entière, y compris la partie qui, elle, tient.
* **Épingler la chaîne d'outils sur `stable`** : c'est ce qui était en place, et
  c'est précisément le trou. Sur un canal flottant, la promesse dépend de la
  date à laquelle chaque contributeur a lancé `rustup update`.
