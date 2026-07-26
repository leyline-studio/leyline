# ADR 0043 — Effondrement de l'historique de rendu avant publication, et la révision porte sa carte d'étages

**Statut :** Accepté — 2026-07
**Complète et amende :** [ADR 0042](0042-versioned-stage-pipeline.md) (§2 livré, §5 rendu sans objet)

## Contexte

ADR 0042 a remplacé les onze `processN.rs` dupliqués par des étages
versionnés indépendamment. Son §5 prévoyait que `process: N` reste lu et
compris pour toujours, via une **table d'expansion figée** de onze lignes :
`process: 4` signifiant `lens::v2` + `gains::v2` + `contrast::v1` + …

Cette table existe, elle est correcte, et les 77 rendus de référence prouvent
qu'elle rend les onze versions au bit près (commit `bf63df1`). La question que
le présent ADR tranche n'est pas *si elle marche* : c'est **à qui elle sert**.

**Réponse : personne.** Leyline n'a pas été publié. Il n'existe, nulle part,
aucune révision citant `process: 3` en dehors des catalogues de développement
de l'auteur et des fixtures de test. Les onze versions ne sont pas onze
promesses tenues envers onze générations d'utilisateurs — ce sont onze étapes
de construction, conservées par application d'une règle (« un moteur doit
savoir rendre toutes les process versions passées ») à une période où cette
règle n'avait encore rien à protéger.

Ce que cet historique coûte, une fois l'effondrement écarté :

* trois versions d'étage qui n'existent que pour lui — `gains::v1` (le `powf`
  exact d'avant ADR 0013), `lens::v1` (distorsion seule) et `lens::v2`
  (distorsion + vignettage, sans TCA) ;
* onze lignes de table d'expansion à maintenir exactes, dont ADR 0042 dit
  lui-même qu'une erreur « rendrait différemment une photo ancienne » — le
  point le plus critique du moteur, entretenu pour des photos qui n'existent
  pas ;
* 77 cas dorés à recalculer à chaque évolution du harnais ;
* et surtout un axe de versionnage **en double**. ADR 0042 §2 fait porter le
  versionnage par la carte `stages` de la révision ; `process: N` devait
  survivre comme abréviation historique. Deux mécanismes coexistants pour
  désigner un rendu, dont un seul a un avenir.

Le raisonnement est exactement celui qu'ADR 0042 s'applique déjà à lui-même à
propos du changement de forme de `settings_json` : *« ce n'est acceptable que
parce que le projet est pré-publication ; après ouverture au monde, cette
forme serait définitive. C'est la raison de faire ce changement maintenant et
pas plus tard. »* La même fenêtre, exactement, se referme sur l'historique de
rendu. Le jour de la publication, ces onze versions deviennent irréversiblement
des engagements ; aujourd'hui elles ne sont que du code.

## Décision

### 1. Une seule version publiée par opérateur, numérotée `v1`

L'historique de rendu antérieur à la publication est effondré. Chaque
opérateur conserve **une** version : celle qui rend aujourd'hui, c'est-à-dire
l'état de `process: 11`. `gains::v1`, `lens::v1` et `lens::v2` sont supprimés ;
`gains::v2` et `lens::v3` deviennent les `v1` de leur opérateur.

Elles sont numérotées `v1` et non `v0` : ce ne sont pas des brouillons. Ce sont
les premières versions **publiées** de chaque opérateur, gelées au sens plein
d'ADR 0042 §1 le jour de l'ouverture au monde. `v0` suggérerait un statut
provisoire qui cessera d'être vrai sans que rien dans le code ne change.

### 2. La révision enregistre sa carte d'étages ; `process` disparaît

ADR 0042 §2 est livré ici, dans le même mouvement, et pour une raison
mécanique : l'effondrement le rend trivial. Il n'y a plus qu'une expansion
possible, donc plus rien à expandre — la révision écrit directement la version
de chaque étage qu'elle utilise :

```json
"stages": { "gains": 1, "tone_curve": 1, "dehaze": 1 }
```

Le champ `process` est **retiré** de `settings_json`, sans remplacement et sans
abréviation historique : il n'a plus de valeur à désigner. Le champ `schema`,
lui, reste — il versionne la *forme* du document, pas le rendu, et les deux
axes restent distincts (`docs/pipeline.md` §3.4).

Les règles d'ADR 0042 §2 s'appliquent inchangées : un étage neutre ne s'exécute
pas, n'a donc aucun comportement à épingler, et **n'apparaît pas** dans la
carte.

### 3. Épingler une version se fait à l'écriture, jamais à la lecture

Quand une révision est écrite, chaque étage actif reçoit une entrée :

* l'étage **déjà présent** dans la carte garde sa version — corriger
  l'exposition d'une photo de 2026 en 2036 ne la fait pas changer de rendu ;
* l'étage **absent** (nouvellement sorti de sa valeur neutre) reçoit la version
  courante du moteur — activer la netteté en 2036 donne la meilleure netteté
  de 2036, pas celle de 2026 ;
* l'étage **redevenu neutre** perd son entrée, puisqu'il ne rend plus rien.

Rien n'est jamais déduit à la lecture : une carte lue est appliquée telle
quelle. C'est ce qui rend la révision auto-descriptive au sens d'ADR 0042 §2.

### 4. Une version d'étage inconnue est refusée, jamais approchée

`process > CURRENT_PROCESS` était le garde-fou d'ADR 0042 (`docs/pipeline.md`
§3.4) : une révision écrite par un moteur plus récent est refusée, jamais
devinée. Il devient : **toute carte citant un étage ou une version d'étage que
ce moteur ne connaît pas échoue** avec `NewerSettings`, et l'appelant retombe
sur le meilleur aperçu en cache. Le comportement observable est identique ; sa
granularité est meilleure, puisque le refus nomme l'étage fautif.

### 5. Les catalogues de développement existants ne sont pas migrés

Aucun code de migration n'est écrit. Une révision stockée citant `process: N`
n'a plus de forme valide et est refusée à la lecture, comme toute révision
illisible. Les bibliothèques de développement se réimportent.

Écrire une migration serait ici une **fausse rigueur** : elle ne pourrait pas
préserver les pixels des révisions en process < 11 (leur rendu était défini par
l'absence de vignettage, de TCA, de courbe tonale…, et l'effondrement fait
précisément disparaître cette absence). Elle ne préserverait que des réglages,
sur des données de test, au prix d'un code de conversion à maintenir et à
tester — dont la seule justification serait de faire *comme si* la promesse
s'appliquait déjà à des photos auxquelles elle ne s'applique pas encore.

### 6. Les rendus de référence sont re-bénis une fois, délibérément

Les 77 cas dorés d'ADR 0042 §7 deviennent les cas du pipeline unique. Le
manifeste est régénéré **une fois**, par cette décision-ci et sous sa trace.

C'est l'exact opposé du cas interdit. La règle d'ADR 0042 §7 — « ce sont les
fixtures, pas la relecture du diff, qui établissent l'égalité » — vise le
digest qui bouge **pendant un refactor censé ne rien changer** : là, le digest
qui bouge *est* le signal d'échec. Ici la décision précède la mesure et
l'assume : on a décidé que ces rendus ne sont plus dus à personne. Après cette
régénération, la règle reprend sa force pleine et sans exception.

### 7. Le mécanisme de versionnage garde un exemple vivant

Effet secondaire à traiter, et non à subir : avec une seule version partout, le
chemin « une ancienne version rend toujours ce qu'elle rendait » n'est plus
exercé par aucun test. C'est la garantie centrale du projet qui deviendrait,
pour la première fois, du code non couvert.

Le registre porte donc en permanence un **étage à deux versions réservé aux
tests**, compilé uniquement sous `cfg(test)` : la suite vérifie qu'une carte
citant `v1` rend `v1` alors même que `v2` existe, que le rang déclaré par
chaque version décide de la position, et qu'une version inconnue est refusée.
Le mécanisme reste ainsi démontré sans attendre la première vraie correction
pixel — laquelle, elle, sera bien un `v2` réel.

## Conséquences

* **Ce qui disparaît :** trois versions d'étage, la table d'expansion et ses
  onze lignes, le champ `process`, `CURRENT_PROCESS`, et la notion de
  « migrer une photo vers une process version récente » — remplacée par
  « remonter les étages épinglés d'une révision à leur version courante »,
  c'est-à-dire le retraitement (`reprocess`), qui garde son nom et son sens.
* **La promesse ne bouge pas d'un cran** — elle prend juste sa vraie date. Elle
  s'énonce toujours comme ADR 0042 §6 l'énonce, par version d'étage ; elle
  commence à courir à la publication, ce qui est exactement le moment où elle
  devient due. Prétendre qu'elle courait déjà était l'illusion que le présent
  ADR retire.
* **La fenêtre se referme ici.** Après la première version publique, aucun ADR
  ne pourra plus effondrer quoi que ce soit : les versions d'étage deviennent
  des engagements, et la seule évolution possible est l'ajout. Le présent ADR
  est donc, par construction, le dernier de son espèce.

## Alternatives écartées

* **Garder la table d'expansion « au cas où »** : elle ne protège aucune photo
  existante et coûte l'exactitude la plus critique du moteur. Un mécanisme de
  sécurité entretenu pour un risque nul n'est pas une sécurité, c'est une
  surface d'erreur — et celle-ci se trompe silencieusement, en pixels.
* **Effondrer maintenant, livrer la carte `stages` plus tard** : ferait changer
  la forme de `settings_json` deux fois de suite, donc casserait les catalogues
  de développement deux fois, pour un seul résultat final.
* **Numéroter `v0`** : voir §1. Le numéro survivrait au statut qu'il décrit.
* **Écrire une migration des catalogues de développement** : voir §5.
* **Renoncer au gel et adopter `legacy_params` (darktable)** : déjà examiné et
  écarté par ADR 0042, pour une raison que le présent ADR ne touche pas. On
  effondre un historique qui n'engage personne ; on ne renonce pas à geler
  celui qui engagera.
