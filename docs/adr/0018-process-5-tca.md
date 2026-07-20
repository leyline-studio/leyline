# ADR 0018 — Process version 5 : aberration chromatique transversale (TCA)

**Statut :** Accepté — 2026-07

## Contexte

ADR 0016 (distorsion) et ADR 0017 (vignettage) ont laissé l'aberration
chromatique transversale (TCA) explicitement hors périmètre. `lensfun::Modifier`
la calcule via `enable_tca_correction` + `apply_subpixel_distortion` : un
gain de position radial *par canal* (rouge/vert/bleu n'ont pas exactement le
même grandissement à travers l'objectif, d'où des franges de couleur près
des bords du cadre). Le brancher change les pixels produits, donc exige une
nouvelle process version (§3.3).

## Décision

Le moteur introduit `process: 5`, défini dans son propre module gelé
(`process5.rs`), identique à `process 4` à une seule différence près :
l'étape de correction d'objectif corrige en plus le TCA, avec le même
profil déjà matché pour la distorsion.

Détails :

1. `leyline_lens::Correction` (déjà utilisée pour la distorsion) appelle
   aussi `enable_tca_correction` à la construction, et expose
   `tca_row(y, width) -> Vec<[(f32, f32); 3]>` — les trois coordonnées
   sources (une par canal) pour chaque pixel de la ligne, via
   `Modifier::apply_subpixel_distortion`. Contrairement au vignettage, le
   sens de `reverse` de Lensfun est le même pour la distorsion et le TCA
   (`rescale_tca` passe `self.reverse` tel quel) : les deux corrections
   partagent donc la même instance `Modifier`/`Correction`, une seule
   recherche de profil.
2. `process5.rs` applique le TCA comme une **seconde passe géométrique
   indépendante**, juste après la passe de distorsion : chaque canal de
   chaque pixel de sortie est rééchantillonné séparément à sa propre
   coordonnée source, depuis le tampon *déjà* corrigé de sa distorsion —
   **pas** fusionné en un seul remapping distorsion+TCA combiné. C'est une
   simplification assumée (voir « Alternatives écartées ») : `apply_subpixel_distortion`
   du crate ne calcule que le décalage TCA seul, sans y composer la
   distorsion ; la composition manuelle des deux transformations de
   coordonnées en une seule passe n'est pas tentée dans cette version.
3. Aucune calibration TCA à cette focale (ou aucun profil matché du tout)
   laisse l'image exactement comme `process 4` l'aurait rendue — même
   garantie de repli que la distorsion et le vignettage.

`CURRENT_PROCESS` passe à 5 : les nouvelles révisions écrivent `process: 5`.
Les révisions existantes déclarant `process: 1` à `4` continuent d'être
rendues par leurs modules respectifs, inchangés pour toujours.

## Conséquences

* `process5.rs` duplique les opérateurs inchangés de `process 4` (même
  choix qu'ADR 0013/0016/0017). `undistort` est scindé de la construction
  de `Correction` (désormais faite une fois dans `develop`) pour que
  `correct_tca` réutilise la même instance sans reconstruire de `Modifier`.
* Deux rééchantillonnages bilinéaires successifs (distorsion, puis TCA)
  au lieu d'un seul combiné : un coût de qualité mineur (interpolation
  double sur les quelques pixels où les deux corrections sont actives
  simultanément) contre une implémentation beaucoup plus simple et sûre
  qu'une composition manuelle des transformations de coordonnées.
* Un test avec un profil réel n'ayant que des données de distorsion, pas de
  TCA (`Canon EF 17-35mm f/2.8L USM`, base bundled de `lensfun`) vérifie la
  parité bit-exacte avec `process 4` : la passe TCA est un vrai no-op sans
  données. Un second test avec le profil Canon EF 16-35mm f/2.8L II USM
  (qui a des données TCA à 20mm) vérifie que la correction change le rendu
  au-delà de ce que distorsion + vignettage produisent déjà.

## Alternatives écartées

* **Composer distorsion et TCA en une seule passe de rééchantillonnage** :
  la manière la plus rigoureuse, mais `apply_subpixel_distortion` du crate
  ne fait que la partie TCA du calcul — la fusion demanderait de recomposer
  manuellement les transformations de coordonnées normalisées des deux
  passes (mod_coord + mod_subpix), un travail non trivial et risqué à
  implémenter correctement sans images de référence Lensfun sous la main.
  Reporté à une éventuelle process version future si le besoin de qualité
  se justifie.
* **Ignorer le TCA pour de bon** : la spec V1 dit juste « Correction
  d'objectif (Lensfun) » sans détail — la distorsion seule aurait suffi à
  cocher la case, mais Lensfun expose le TCA et le vignettage aussi
  simplement une fois le profil matché ; les laisser de côté aurait été un
  choix de paresse, pas d'ingénierie.
