# ADR 0013 — Process version 2 : fonctions de transfert sRGB par table

**Statut :** Accepté — 2026-07

## Contexte

Dans `process 1`, la balance des blancs et l'exposition convertissent chaque
échantillon vers la lumière linéaire et retour (`srgb_to_linear`,
`linear_to_srgb`), soit deux `powf` par échantillon — le coût dominant des
opérateurs par-pixel. Une table précalculée avec interpolation linéaire est
beaucoup plus rapide, mais ses résultats ne sont pas bit-à-bit ceux des
formules exactes : d'après `docs/pipeline.md` §3.3 et l'ADR 0012, un tel
changement exige une nouvelle version de process.

## Décision

Le moteur introduit `process: 2`, défini dans son propre module gelé
(`process2.rs`), identique à `process 1` à une seule différence près : les
deux fonctions de transfert sRGB des opérateurs en lumière linéaire passent
par des tables de 4096 intervalles (4097 entrées, entrée `i` = formule
exacte évaluée en `i / 4096`) avec interpolation linéaire en `f32`. Les
tailles de table et le mode d'interpolation font partie du contrat de
rendu : les changer exigerait un `process: 3`.

`CURRENT_PROCESS` passe à 2 : les nouvelles révisions écrivent `process: 2`.
Les révisions existantes déclarent `process: 1` et continuent d'être rendues
par le module `process1`, inchangé pour toujours — conformément à la règle
« un moteur donné doit savoir rendre toutes les process versions passées ».
Une révision éditée hérite du process de son parent : aucune migration
implicite ; la migration volontaire (reprocessing, `pipeline.md` §4.5) crée
une nouvelle révision.

## Conséquences

* L'erreur d'approximation est < 2·10⁻⁵ dans le domaine gamma : invisible en
  sortie 8 bits (< 0,005 pas de quantification), mais pas bit-identique —
  d'où la nouvelle version. Un test borne l'écart process 1 / process 2 à un
  pas 8 bits au plus.
* Le module `process2.rs` duplique les opérateurs inchangés de `process 1`
  plutôt que de les partager : c'est le prix assumé du gel (« mêmes pixels
  dans dix ans »), un futur correctif d'un opérateur de process 3 ne devant
  jamais pouvoir altérer les rendus passés.
* Les benchmarks comparent les deux versions (`cargo bench -p
  leyline-engine`, groupes `process1` et `process2`).

## Alternatives écartées

* **Table exacte sans nouvelle version** : les entrées d'une table indexée
  sur les 256/65536 valeurs quantifiées du décodage seraient bit-exactes,
  mais seulement pour le premier opérateur du pipeline — le gain ne couvre
  pas `linear_to_srgb`, dont l'entrée est continue.
* **Approximation polynomiale de `powf`** : mêmes conséquences
  contractuelles qu'une table, précision plus difficile à borner.
