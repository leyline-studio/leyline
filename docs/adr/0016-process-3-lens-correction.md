# ADR 0016 — Process version 3 : correction géométrique d'objectif (Lensfun)

**Statut :** Accepté — 2026-07

## Contexte

`settings_json` déclare `lens_correction` depuis le schéma 1
(`docs/pipeline.md` §3.2), et `docs/pipeline.md` §3.1 place la correction
d'objectif en tête du pipeline de rendu, avant la balance des blancs. Ni
`process 1` ni `process 2` ne la rendent : `enabled: true` y produit le même
résultat que `false` (ADR 0013). `docs/specification.md` liste « Correction
d'objectif (Lensfun) » dans le périmètre V1 — il fallait la brancher.

Le crate `leyline-lens` matche déjà les chaînes EXIF caméra/objectif contre
la base de profils Lensfun embarquée (crate `lensfun`, pur Rust) et expose
une carte de correspondance arrière par ligne (`Correction::source_row`).
Restait à l'appliquer aux pixels — ce qui change le rendu, donc exige une
nouvelle version de process (§3.3).

## Décision

Le moteur introduit `process: 3`, défini dans son propre module gelé
(`process3.rs`), identique à `process 2` à une seule différence près : la
correction d'objectif est rendue au lieu d'être un champ mort.

Quand `lens_correction.enabled` est vrai et que l'appelant fournit un
`LensShot` (fabricant/modèle caméra, fabricant/modèle objectif si connus,
focale en mm — construit depuis `Metadata` du catalogue par
`render::lens_shot`), le moteur :

1. cherche un profil via `leyline_lens::find_profile` ;
2. sans correspondance (objectif inconnu de la base, ou aucun `LensShot`
   fourni), laisse l'image inchangée — l'EXIF est *best-effort*, la
   correction n'est jamais devinée ;
3. avec une correspondance, construit une `leyline_lens::Correction` pour la
   focale et les dimensions de l'image, puis rééchantillonne chaque pixel de
   sortie par interpolation bilinéaire à la coordonnée source que
   `Correction::source_row` indique (remapping arrière, même famille que la
   rotation de `process2.rs`/`process3.rs`). Les échantillons dont la source
   tombe hors cadre restent noirs — même convention que la rotation, aucun
   canal alpha dans le tampon de travail.

Seule la distorsion géométrique est corrigée en V1. Le vignettage et
l'aberration chromatique transversale (TCA), que `lensfun::Modifier` sait
aussi calculer, restent hors périmètre de `process 3` : coupe de scope
volontaire, pas une limite de Lensfun. Seul le profil `"auto"` (correspondance
par métadonnées) est géré — `lens_correction.profile` n'a pas d'autre valeur
exploitée en V1.

`CURRENT_PROCESS` passe à 3 : les nouvelles révisions écrivent `process: 3`.
Les révisions existantes déclarant `process: 1` ou `process: 2` continuent
d'être rendues par leurs modules respectifs, inchangés pour toujours.

## Conséquences

* `render()` gagne un paramètre `shot: Option<&LensShot>`, ignoré par
  `process1`/`process2`. Les deux points d'appel réels (`preview::preview`,
  `export::export_version`) le construisent depuis `catalog.metadata(asset)`
  via `render::lens_shot`. Les appels de test/benchmark, sans EXIF
  synthétique disponible, passent `None`.
* `process3.rs` duplique les opérateurs inchangés de `process 2` plutôt que
  de les partager (même choix qu'ADR 0013) : le gel de chaque process
  version reste garanti même si un futur `process 4` change un opérateur
  différent.
* Deux conventions de coordonnées bilinéaires coexistent dans `process3.rs` :
  celle de `rotate`/`crop` (centres en `n + 0.5`, choix propre à Leyline) et
  celle de `lens_bilinear` (centres en coordonnées entières, convention de
  Lensfun) — elles ne sont pas interchangeables, `lens_bilinear` est un
  échantillonneur dédié plutôt qu'une réutilisation incorrecte de `bilinear`.
* Un test borne le rendu à celui de `process 2` quand `lens_correction` est
  désactivé (bit-exact, la nouvelle étape est un no-op), et un test
  d'intégration avec un profil réel de la base embarquée (Canon EOS 5D
  Mark III + EF 16-35mm f/2.8L II USM, déjà utilisé dans les tests de
  `leyline-lens`) vérifie que la correction déplace effectivement des
  pixels.

## Alternatives écartées

* **Corriger la distorsion sans nouvelle process version, en la traitant
  comme un pré-traitement hors contrat** : contredit §3.3 — toute étape qui
  change les pixels produits par une révision existante doit être une
  nouvelle version, sans exception pour son rang dans le pipeline.
* **Vignettage et TCA dans le même tour** : Lensfun les expose
  (`apply_color_modification_*`, `apply_subpixel_distortion`), mais le TCA
  nécessite un rééchantillonnage par canal (3 cartes de coordonnées au lieu
  d'une) et le vignettage un choix d'espace de calcul (linéaire ou gamma)
  non trivial à valider sans images de référence Lensfun sous la main —
  reporté à une prochaine process version plutôt que d'être approximé.
