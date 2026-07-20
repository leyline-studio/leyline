# ADR 0017 — Process version 4 : correction du vignettage (Lensfun)

**Statut :** Accepté — 2026-07

## Contexte

ADR 0016 a introduit `process: 3`, qui corrige la distorsion géométrique
d'un objectif via le profil Lensfun matché depuis l'EXIF, mais laissait
explicitement le vignettage et l'aberration chromatique transversale (TCA)
hors périmètre. `leyline-lens` matche déjà un profil ; `lensfun::Modifier`
sait aussi calculer un gain de vignettage par pixel (`enable_vignetting_correction`
+ `apply_color_modification_f32`) à partir du même profil, de l'ouverture et
d'une distance de mise au point. Restait à le brancher — un changement de
pixels, donc une nouvelle process version (§3.3).

## Décision

Le moteur introduit `process: 4`, défini dans son propre module gelé
(`process4.rs`), identique à `process 3` à une seule différence près :
l'étape de correction d'objectif dé-vignette en plus de corriger la
distorsion, avec le même profil déjà matché.

Détails :

1. `LensShot` gagne un champ `aperture_f: Option<f32>` (l'ouverture EXIF,
   déjà extraite par `leyline-raw` — seul le fil jusqu'à `LensShot` manquait),
   rempli par `render::lens_shot` depuis `Metadata::aperture`.
2. `leyline-lens` expose `Vignetting`, un second type construit avec son
   propre `lensfun::Modifier` — **pas** celui de `Correction` : le drapeau
   `reverse` de Lensfun a un sens opposé pour la distorsion et le
   vignettage (`true` corrige la distorsion mais *simule* le vignettage,
   d'après la doc de la crate `lensfun`), donc les deux corrections ne
   peuvent pas partager une instance.
3. La distance sujet n'est pas dans l'EXIF (aucune caméra grand public ne
   l'enregistre de façon fiable) : `leyline-lens` assume 1000 m, la valeur
   que Lensfun utilise lui-même comme casier « effectivement l'infini » —
   le choix le moins faux pour de la photo non-macro. Assumé, documenté,
   jamais deviné au cas par cas.
4. Le vignettage est un phénomène physique de chute de lumière
   *multiplicatif en lumière linéaire* : `process4.rs` fait donc
   l'aller-retour par les mêmes tables de transfert que `linear_gains`
   (balance des blancs/exposition) plutôt que de multiplier directement en
   gamma, contrairement à une implémentation naïve qui appliquerait le gain
   sur les échantillons gamma tels quels.
5. Aucune ouverture connue (`aperture_f: None`) ou aucune calibration de
   vignettage pour cette focale/ouverture laisse l'image exactement comme
   `process 3` l'aurait rendue.

`CURRENT_PROCESS` passe à 4 : les nouvelles révisions écrivent `process: 4`.
Les révisions existantes déclarant `process: 1`, `2` ou `3` continuent
d'être rendues par leurs modules respectifs, inchangés pour toujours.

## Conséquences

* `process4.rs` duplique les opérateurs inchangés de `process 3` (même
  choix qu'ADR 0013/0016) : la fonction de distorsion (`undistort`) est
  scindée de la recherche de profil pour que `devignette` réutilise le même
  `Profile` matché une seule fois, plutôt que de refaire le lookup deux fois
  comme l'aurait fait une copie mécanique de `process3::correct_lens`.
* Un test vérifie la parité bit-exacte avec `process 3` quand `lens_correction`
  est désactivé ou sans ouverture connue (la nouvelle étape est un no-op
  dans ces deux cas), et un test avec le profil réel Canon EOS 5D Mark III +
  EF 16-35mm f/2.8L II USM (déjà utilisé dans `leyline-lens` et `process3`)
  vérifie que le vignettage change le rendu au-delà de ce que la distorsion
  seule produit déjà.
* TCA reste hors périmètre — coupe de scope supplémentaire, pas une limite
  de Lensfun.

## Alternatives écartées

* **Appliquer le gain directement en gamma** : plus simple, mais
  physiquement faux — le gain de vignettage de Lensfun est calibré en
  lumière linéaire ; l'appliquer post-gamma changerait la réponse tonale de
  façon dépendante du niveau du pixel, pas seulement de sa position.
* **Deviner la distance sujet depuis le mode de mesure ou la focale** :
  aucune heuristique fiable sans données EXIF réelles ; 1000 m (convention
  Lensfun elle-même) est déjà le choix par défaut le plus défendable.
* **Partager un seul `Modifier` entre distorsion et vignettage** : impossible
  proprement à cause du sens opposé de `reverse` entre les deux passes (voir
  point 2 ci-dessus) — inverser le gain à la main (`1.0 / gain`) aurait été
  possible mais moins lisible qu'un second `Modifier` dédié.
