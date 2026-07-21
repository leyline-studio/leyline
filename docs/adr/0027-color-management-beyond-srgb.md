# ADR 0027 — Élargir la gestion des couleurs au-delà de sRGB : un profil de sortie découplé du rendu

**Statut :** Accepté — 2026-07

## Contexte

ADR 0015 fige V1 sur un unique espace, du décodage à l'export : sRGB. Elle
le documente explicitement comme une décision provisoire, pas une clôture :
« Un futur espace de travail plus large (ProPhoto, Adobe RGB en interne)
resterait un changement structurant à part entière — cet ADR ne le prépare
pas et ne l'exclut pas. »

`docs/v2-scope.md` §7 (épreuvage écran, filigrane, module d'impression) et
§8 (profils caméra DCP) buttent tous les deux sur ce gel, identifié comme
second verrou transversal du §9. Plutôt que de laisser chaque ADR de
fonctionnalité future re-dériver indépendamment jusqu'où la gestion des
couleurs s'étend, ce document tranche une fois où le pipeline s'arrête et
où la flexibilité commence.

`leyline-color` aujourd'hui (ADR 0015) n'expose qu'une fonction :
`srgb_icc_profile()`, un profil ICC canonique généré une fois via
`lcms2::Profile::new_srgb`. `leyline-export` l'embarque tel quel dans
JPEG/PNG/TIFF (`crates/leyline-export/src/lib.rs`). Aucun `cmsTransform`
n'existe encore dans le code.

## Décision

**L'espace de travail interne du pipeline de rendu ne change pas.** Le
tampon gamma-encodé sRGB entre opérateurs, la lumière linéaire interne pour
balance des blancs/exposition (invariant documenté dans `process3.rs`)
restent inchangés : cette décision **ne rouvre aucune process version**.
Ce qui s'élargit se situe strictement en **sortie**, entièrement porté par
`leyline-color`/`leyline-export`, découplé de `leyline-engine` :

1. **Export vers un profil ICC de destination.** `leyline-export` produit
   des pixels sRGB comme aujourd'hui ; quand un profil de sortie non
   défaut est demandé (Adobe RGB, ProPhoto, profil imprimante...),
   `leyline-color` applique un `cmsTransform` sRGB → profil destination
   comme toute dernière étape avant encodage — une transformation de
   pixels à l'export, jamais persistée dans `settings_json`, jamais une
   process version, au même titre que format/qualité ne sont déjà pas des
   préoccupations de process (ADR 0025, `ExportRequest`).
2. **Épreuvage écran, vue seule.** L'aperçu se rend normalement (sRGB,
   inchangé), puis `leyline-color` applique une transformation d'épreuvage
   (profil destination + intention de rendu + alerte de gamut optionnelle)
   pour l'affichage uniquement — jamais écrite au catalogue, conforme au
   constat de `docs/v2-scope.md` §7 : l'épreuvage ne touche aucun état
   persistant.
3. **`leyline-color` passe d'« exposer un profil statique » (ADR 0015) à
   « charger des profils ICC arbitraires et construire des
   `cmsTransform` entre eux »** — extension de la dépendance `lcms2`
   existante, pas une nouvelle dépendance.
4. **L'authoring de profils caméra DCP (`docs/v2-scope.md` §8) est
   explicitement hors décision ici.** Un DCP agit sur la conversion
   capteur → espace de travail, au **début** du pipeline (un véritable
   événement de process version), alors que cette décision n'élargit que
   la **sortie**. L'item 8 garde son propre ADR le moment venu, mais peut
   désormais supposer que `leyline-color` sera déjà une bibliothèque de
   transformation ICC générale à ce moment-là, pas un simple émetteur de
   profil statique.

## Conséquences

* ADR 0015 reste valide pour l'espace de travail interne du pipeline —
  rien ici ne rouvre `process1`..`process5`.
* La surface publique de `leyline-color` passe d'une fonction à une petite
  API de transformation (chargement de profil, construction de transform,
  application), testée avec la même rigueur déterministe que
  `srgb_icc_profile()` aujourd'hui.
* Épreuvage écran et export non-sRGB partagent la même primitive
  sous-jacente (une transformation ICC) : construire l'un dérisque
  substantiellement l'autre, même si `docs/v2-scope.md` les traite comme
  deux fonctionnalités séparées.
* Le module d'impression (`docs/v2-scope.md` §7) pourra s'appuyer sur la
  même primitive de transformation de sortie plutôt que d'en inventer une
  troisième.
* Le contrat de reproductibilité (`docs/pipeline.md` §5) n'est pas engagé
  pour les révisions de développement : la transformation vit entièrement
  hors `settings_json` et hors `process` — elle ne peut donc pas casser
  « même révision → mêmes pixels pour toujours ». Elle n'affecte que
  l'encodage à l'export et l'aperçu vue-seule, deux surfaces déjà hors du
  périmètre de ce contrat.

## Alternatives écartées

* **Élargir l'espace de travail interne (Adobe RGB/ProPhoto) directement
  dans le pipeline de rendu** : imposerait une nouvelle process version
  (chaque opérateur suppose sRGB et sa fonction de transfert, invariant
  documenté dans `process3.rs`), et un coût de retraitement/performance
  sur chaque asset pour un bénéfice — marge d'édition grand gamut — que ni
  V1 ni les items du §7/§8 ne réclament. Écarté, disproportionné par
  rapport au vrai manque (les items 7/8 ont besoin de flexibilité en
  **sortie**, pas d'une capacité de travail plus large).
* **Traiter épreuvage et profil d'export comme deux chantiers séparés et
  sans rapport** : aurait dupliqué la plomberie de transformation ICC dans
  `leyline-color`/`leyline-export` deux fois — écarté au profit d'une
  primitive commune.
* **Différer cette décision et laisser les futurs ADR des items 7 et 8
  choisir chacun leur approche** : c'était le statu quo (ADR 0015 le
  laissait explicitement ouvert), mais `docs/v2-scope.md` montre que les
  deux items convergent indépendamment vers le même prérequis — la
  trancher une fois maintenant évite deux ADR qui re-dérivent la même
  réponse.
