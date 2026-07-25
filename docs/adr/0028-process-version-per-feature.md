# ADR 0028 — Une process version par fonctionnalité pixel : maintien de la duplication par module

**Statut :** Remplacé par [ADR 0042](0042-versioned-stage-pipeline.md) — 2026-07

> Ce document décrivait la convention en vigueur jusqu'en juillet 2026 : une
> process version par fonctionnalité, chacune dans un `processN.rs` copie
> intégrale du précédent. Sa dernière conséquence prévoyait sa réouverture « avec
> des données réelles » ; ADR 0042 le fait, sur la base de 14 968 lignes
> cumulées et de 70–93 % de duplication entre modules consécutifs. Conservé tel
> quel pour la trace du raisonnement.

## Contexte

`docs/v2-scope.md` §9 relève un quatrième fil transversal, non bloquant mais
structurant : la **prolifération des process versions**. La maison duplique un
module `processN.rs` complet par version — `process2.rs` copie `process 1`,
`process3.rs` copie `process 2`, et ainsi de suite — en ne changeant que le
seul opérateur nouveau ou différent. C'est un choix délibéré, motivé dans les
sections « Alternatives écartées » d'ADR 0013 et d'ADR 0016 : le partage de
code est exactement ce qui risquerait qu'un correctif futur d'un opérateur
altère silencieusement le rendu gelé d'une version antérieure, censée figée
« mêmes pixels dans dix ans » (`docs/pipeline.md` §3.3).

Cinq versions existent aujourd'hui, toutes liées à la correction d'objectif
ou aux fonctions de transfert (ADR 0013, 0016–0018). La plupart des items du
périmètre V2 sont *pixel-affectants* et exigeraient, chacun, une nouvelle
process version sous cette convention (`docs/v2-scope.md` §2, §3, §4, §5, §6,
§8 : tous notés « Process +1 »). Empiler six items ou plus signifierait donc
autant de nouveaux modules `processN.rs` dupliqués s'ajoutant aux cinq
existants.

Le §9 pose la question ouverte : un process par fonctionnalité (statu quo,
plus de modules dupliqués) ou un bump groupé « process V2 consolidé » (moins
de modules, mais des fonctionnalités qui ne peuvent plus sortir
indépendamment) ? Plutôt que de laisser chaque futur ADR de fonctionnalité
V2 re-trancher cette stratégie, ce document la fige une fois.

Coût réel mesuré de la duplication à ce jour (`wc -l crates/leyline-engine/src/process*.rs`) :

| Module | Lignes |
|---|---|
| `process1.rs` | 451 |
| `process2.rs` | 560 |
| `process3.rs` | 726 |
| `process4.rs` | 773 |
| `process5.rs` | 923 |
| **Total** | **3433** |

## Décision

**La convention reste inchangée : une process version par fonctionnalité
pixel, chacune dans son propre module `processN.rs` gelé, créé en copiant le
module précédent entier et en ne changeant que l'opérateur nouveau ou
différent** — exactement ce qu'ADR 0013 et ADR 0016 ont établi. La V2
n'introduit **ni** bump groupé (« process V2 » multi-fonctionnalités), **ni**
bibliothèque d'opérateurs partagés que les versions composeraient au lieu de
dupliquer.

Ce document ne décide d'aucune fonctionnalité V2 : il fige la *stratégie de
versionnage* que chaque futur ADR de fonctionnalité (courbe tonale, TSL,
réglages locaux, suppression de tache, dehaze, DCP…) appliquera sans la
re-litiger. Chaque fonctionnalité pixel prête à sortir prend le prochain
numéro de process disponible et son propre module.

## Conséquences

* Chaque fonctionnalité V2 pixel-affectante peut sortir **indépendamment**,
  dès qu'elle est prête, sans être bloquée par une autre fonctionnalité du
  même lot théorique — le versionnage ne devient jamais un point de
  synchronisation entre chantiers non liés.
* Le champ `process` d'une révision garde sa **lisibilité sémantique** :
  `process: 3` signifie aujourd'hui exactement « correction de distorsion
  d'objectif active », un fait unique et lisible ; il continuera de désigner
  une fonctionnalité identifiable plutôt qu'un lot opaque de fonctionnalités
  sans rapport.
* Le nombre de modules `processN.rs` croît d'une unité par fonctionnalité
  pixel — coût borné et connu (3433 lignes cumulées pour cinq versions,
  ci-dessus). La base tolère déjà cinq versions sans friction ; la trajectoire
  reste linéaire et prévisible.
* La garantie de gel est préservée intégralement : aucun module de version
  antérieure n'est touché quand une nouvelle sort, puisqu'aucun code de rendu
  n'est partagé entre versions (le contrat « mêmes pixels dans dix ans » reste
  mécaniquement infalsifiable, `docs/pipeline.md` §3.3).
* Si le nombre de modules devenait un jour un vrai fardeau — un nombre bien
  supérieur aux cinq actuels, après que plusieurs fonctionnalités V2 ont
  réellement été livrées — un futur ADR pourra rouvrir la question **avec des
  données réelles**. Ce document ne préempte pas cette décision ; il refuse
  seulement de l'anticiper spéculativement aujourd'hui.

## Alternatives écartées

* **Bump groupé « process V2 consolidé »** (plusieurs fonctionnalités pixel
  réunies sous une seule nouvelle process version, donc un seul module
  supplémentaire) : n'économise pas la duplication future qu'il prétend
  éviter. La règle §3.3 impose qu'*un* correctif d'*une seule* des
  fonctionnalités groupées, une fois la version publiée, force malgré tout une
  process version entièrement nouvelle — donc un module complet de plus. Le
  groupage ne fait donc que **retarder** la croissance des modules (en
  bloquant la sortie indépendante des fonctionnalités déjà prêtes) sans
  jamais l'éviter. Il dégrade en prime la lisibilité du champ `process`, qui
  n'annoncerait plus un fait sémantique unique mais un paquet opaque de
  fonctionnalités hétérogènes. Écarté : tous les coûts, aucun des bénéfices
  supposés.
* **Bibliothèque d'opérateurs partagés** (extraire les opérateurs communs
  pour que les versions les *composent* au lieu de les *dupliquer*, réduisant
  le total de lignes) : contredit frontalement la raison d'être de la
  duplication intégrale, motivée dans ADR 0013 et ADR 0016. Du code partagé
  est précisément ce qui expose un rendu gelé au risque qu'un changement
  futur, sans rapport, l'altère silencieusement — la duplication par module
  n'est pas une négligence à optimiser, c'est le mécanisme même qui rend le
  gel infalsifiable. Écarté : réintroduirait exactement le danger que la
  convention existe pour supprimer.
* **Différer la décision et laisser chaque ADR de fonctionnalité V2 choisir**
  sa stratégie : c'était le statu quo implicite, mais `docs/v2-scope.md`
  montre que six items ou plus convergent vers le même choix de versionnage.
  Le trancher une fois maintenant évite que chaque futur ADR re-dérive — ou
  pire, tranche différemment — la même réponse, au risque d'une convention de
  versionnage incohérente d'un item à l'autre.
