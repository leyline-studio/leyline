# Périmètre V2 — fonctionnalités candidates

**Document :** `docs/v2-scope.md`
**Version :** 0.1
**Statut :** Exploration

---

# 1. Objectif

Le périmètre V1 (`docs/specification.md` §Inclus) est **livré et clos**.

Ce document ne rouvre pas la V1. Il cadre, au niveau architecture, sept fonctionnalités qui **manquent** aujourd'hui — sans être exclues par conception. Les exclusions volontaires de V1 (`docs/specification.md` §Exclus volontairement : cloud, comptes, IA, HDR, panorama, reconnaissance faciale, synchronisation) restent hors sujet ici. Les sept items ci-dessous sont d'une autre nature : ce sont des attentes classiques d'un développeur RAW (parité Lightroom/Darktable/Capture One) simplement pas encore construites.

Chaque section décrit ce que l'item exige du **contrat de rendu** (`docs/pipeline.md` §3.2/§3.3 : `schema` de format vs `process` de rendu), sa place dans l'**ordre du pipeline** (§3.1), le ou les **crates** propriétaires, les implications **catalogue** (`docs/catalog.md`) et **API moteur** (`docs/engine-api.md`), une lecture de complexité (S/M/L/XL) et les questions ouvertes.

Ce n'est **pas** un plan d'implémentation ni un calendrier de phases : c'est une analyse de contexte parallèle à celle qu'un ADR pose avant décision. Aucune décision n'est prise ici — donc aucun ADR n'accompagne ce document (`docs/adr/README.md` : un ADR consigne une décision **acceptée**). Le §9 note, pour chaque item, s'il mérite son propre ADR une fois tranché.

Rappels de contrat utilisés partout ci-dessous :

* une étape qui **change les pixels** d'une révision impose une nouvelle **process version** (`docs/pipeline.md` §3.3), y compris une simple insertion dans l'ordre du pipeline (§3.1) ;
* ajouter un **paramètre optionnel de valeur neutre** est compatible et n'incrémente **pas** le `schema` (§3.4) — les champs inconnus d'un moteur ancien sont déjà préservés verbatim (`Settings::extra`, `leyline-core/src/settings.rs`) ;
* la plupart des items ci-dessous sont donc **process +1, schema inchangé** : la structure additive est tolérée, seul le rendu bouge.

---

# 2. Réglages locaux / masqués

Aucun réglage local aujourd'hui : brosse, filtre radial, filtre gradué, dégradé linéaire sont absents. Tout `Settings` s'applique globalement.

C'est l'item **fondateur** : il introduit la notion générique de *masque* (une couverture spatiale `[0,1]` par pixel) au-dessus de laquelle un sous-ensemble de réglages s'applique localement. Les items 4 (retouche), 5 (dehaze masqué) et le color grading régional (item 3) s'y adossent naturellement.

| Aspect | Analyse |
|---|---|
| Contrat | **Process +1** (change les pixels). **Schema : additif** — un tableau optionnel `local_adjustments`, absent = neutre ; pas d'incrément strictement requis, mais un incrément assumé se défend pour un ajout structurel de cette taille. |
| Pipeline (§3.1) | **Tranché, [ADR 0029](adr/0029-process-6-local-adjustments.md) : process 6**, étage **Réglages locaux** inséré immédiatement après Vibrance/Saturation et avant Réduction du bruit — une passe masquée réutilisant les formules d'opérateur globales. Le référentiel de coordonnées est celui de `crop` ([ADR 0026](adr/0026-mask-spot-coordinate-referential.md)). Le diagramme §3.1 sera amendé par la PR d'implémentation, pas par l'ADR. |
| Crate | **Tranché, [ADR 0029](adr/0029-process-6-local-adjustments.md) : aucun nouveau crate.** Géométrie et valeurs dans `leyline-core` (`Settings`) ; rastérisation et compositing dans un module `leyline-engine` (p. ex. `mask.rs`) consommé par `process6.rs`. Pas de `leyline-mask` : le masquage n'enveloppe rien d'externe (contrairement à `leyline-lens`/Lensfun), il est étroitement couplé aux internes du tampon de rendu. |
| Catalogue | **Tranché, [ADR 0029](adr/0029-process-6-local-adjustments.md) : aucune table dédiée.** Masques **paramétriques** (radial/gradient) : quelques flottants, compacts dans `settings_json` — cohérent avec « une révision = état complet et autonome » (`docs/catalog.md` §17). Masques **brosse** : liste de **traits vectoriels** (x/y/rayon/flux/dureté), jamais un raster. Une table dédiée reste un problème de futur ADR *avec données réelles* si le volume l'exige un jour. |
| API moteur | **Tranché, [ADR 0029](adr/0029-process-6-local-adjustments.md) :** aucune méthode `EditSession` nouvelle — le cycle de vie des masques passe par `set`/`commit` plus deux variantes, `Param::LocalAdjustment(usize)` et `Value::LocalAdjustment(Option<LocalAdjustment>)`. **Pas** de `SettingsGroup` local en V2 (§10.3) : la géométrie de masque est spécifique à la composition, non transférable en preset (même coupe que `Geometry`). |
| Complexité | **XL** — c'est de l'infrastructure, pas une fonctionnalité isolée. L'architecture est désormais tranchée ([ADR 0029](adr/0029-process-6-local-adjustments.md)) ; la complexité restante est d'implémentation (rastérisation brosse, UI), plus une question architecturale ouverte. |

Questions ouvertes :

1. ~~Référentiel des masques~~ — résolu, [ADR 0026](adr/0026-mask-spot-coordinate-referential.md) : même référentiel que `crop` (après rotation, avant recadrage).
2. ~~**Stockage brosse** : traits vectoriels dans `settings_json` vs table dédiée~~ — résolu, [ADR 0029](adr/0029-process-6-local-adjustments.md) : traits vectoriels (x/y/rayon/flux/dureté) dans `settings_json`, jamais de raster ; une table dédiée reste un problème de futur ADR *avec données réelles* si le volume l'exige un jour.
3. ~~**Interaction undo/redo et coalescence** (§17 catalogue) : un trait de brosse est-il une intention ou un geste continu à coalescer ?~~ — résolu, [ADR 0029](adr/0029-process-6-local-adjustments.md) : un trait complet (appui→relâchement) est **un** point de commit (§17, « fin de drag ») ; aucun mécanisme nouveau — `Param::LocalAdjustment(usize)`/`Value::LocalAdjustment` réutilisent la coalescence et la fenêtre d'amendement existantes.

---

# 3. Courbe tonale

Aucune courbe paramétrique ni par points. Seuls les curseurs grossiers existent (exposition, contraste, hautes lumières/ombres, blancs/noirs).

| Aspect | Analyse |
|---|---|
| Contrat | **Process +1**. **Schema : additif** — `tone_curve` optionnel, absent = courbe identité (neutre) ; pas d'incrément requis. |
| Pipeline (§3.1) | **Tranché, [ADR 0030](adr/0030-tone-curve.md)** : étage courbe inséré dans le bloc tonal, après Blancs/Noirs et avant Vibrance/Saturation. Nouvelle process version (le prochain numéro disponible à la sortie, [ADR 0028](adr/0028-process-version-per-feature.md)). Le diagramme §3.1 sera amendé par la PR d'implémentation, pas par l'ADR. |
| Crate | **Tranché, [ADR 0030](adr/0030-tone-curve.md) : aucun nouveau crate.** `leyline-engine` : construction d'une LUT 1D depuis les points de contrôle (convention [ADR 0013](adr/0013-process-2-lut-transfer.md)). `leyline-core` porte les champs. |
| Catalogue | Aucun. Tout tient dans `settings_json`. |
| API moteur | Nouveaux `Param` (points de courbe, mode paramétrique). Rejoint un `SettingsGroup::Tone` élargi ou un groupe dédié. |
| Complexité | **S/M**. |

Champs pressentis (`settings_json`, schema inchangé) :

| Champ | Rôle | Valeur neutre |
|---|---|---|
| `tone_curve.points` | Courbe par points, liste `{x, y}` normalisés `[0,1]` | absent — identité |
| ~~`tone_curve.parametric`~~ | ~~Régions highlights/lights/darks/shadows~~ — **coupé, [ADR 0030](adr/0030-tone-curve.md)** : une courbe paramétrique n'est qu'une UI générant des points ; Studio calcule la liste de points côté client si besoin, le moteur n'a qu'un chemin de courbe. | — |
| ~~`tone_curve.channel`~~ | ~~Cible `rgb`/`r`/`g`/`b`~~ — **luminance seule en V2, [ADR 0030](adr/0030-tone-curve.md)** : courbes par canal retranchées comme fonctionnalité séparée plus lourde. | luminance |

Questions ouvertes :

1. ~~**Interpolation gelée** : la spline (cubique monotone recommandée) fait partie du contrat de rendu~~ — résolu, [ADR 0030](adr/0030-tone-curve.md) : **spline cubique monotone** (Fritsch–Carlson ou équivalent), choix de modèle gelé avec la process version pour éviter l'overshoot d'une cubique naïve ; constantes numériques exactes laissées à la PR. Précalcul en LUT (convention [ADR 0013](adr/0013-process-2-lut-transfer.md)), pas d'évaluation par pixel.
2. ~~**Courbes par canal RGB** dès la V2 ou luminance seule d'abord~~ — résolu, [ADR 0030](adr/0030-tone-curve.md) : **luminance seule** en V2 ; les courbes par canal sont une fonctionnalité séparée plus lourde, à concevoir plus tard dans son propre ADR si voulue.

---

# 4. Color grading / mélangeur TSL

Vibrance et saturation existent (globales). Il manque le mélangeur **TSL par teinte** (8 bandes teinte/saturation/luminance) et les **roues de color grading** ombres/tons moyens/hautes lumières (couleur + luminance), à la manière du *color balance rgb* de Darktable ou du panneau *color grading* de Lightroom.

| Aspect | Analyse |
|---|---|
| Contrat | **Process +1**. **Schema : additif** — `hsl` et `color_grading` optionnels, absents = neutres. |
| Pipeline (§3.1) | **Tranché, [ADR 0031](adr/0031-hsl-color-grading.md)** : bloc couleur, après Vibrance/Saturation. Nouvelle process version (le prochain numéro disponible à la sortie, [ADR 0028](adr/0028-process-version-per-feature.md)) ; TSL et color grading conçus ensemble mais livrables séparément. Le diagramme §3.1 sera amendé par la PR d'implémentation. |
| Crate | **Tranché, [ADR 0031](adr/0031-hsl-color-grading.md) : aucun nouveau crate.** `leyline-engine`. |
| Catalogue | Aucun. Tient dans `settings_json`. |
| API moteur | Nouveaux `Param` ; un `SettingsGroup` couleur pour les presets. |
| Complexité | **M**. |

Champs pressentis :

| Champ | Rôle | Valeur neutre |
|---|---|---|
| `hsl` | 8 bandes de teinte × `{hue, saturation, luminance}` | absent — 0 partout |
| `color_grading.shadows` / `.midtones` / `.highlights` | `{hue, saturation, luminance}` par zone | absent — neutre |
| `color_grading.blending` / `.balance` | Recouvrement des zones, bascule ombres↔hautes lumières | 0 |

Questions ouvertes :

1. ~~**Modèle de teinte gelé** : la définition exacte de la teinte et l'espace de calcul~~ — résolu, [ADR 0031](adr/0031-hsl-color-grading.md) : **HSL dérivé du RGB de travail** (pas d'espace perceptuel/CIE), 8 bandes à centres fixes avec falloff entre bandes adjacentes ; zones de color grading séparées par **pondération de luminance** (smoothstep + `balance`/`blending`), orthogonale au masque spatial d'[ADR 0029](adr/0029-process-6-local-adjustments.md). Constantes numériques exactes laissées à la PR.
2. ~~**Color grading régional** : appliquer les roues sous masque (item 2)~~ — résolu (hors périmètre V2), [ADR 0031](adr/0031-hsl-color-grading.md) : le color grading régional est déféré à un futur ADR adossé à l'infrastructure de masquage d'[ADR 0029](adr/0029-process-6-local-adjustments.md) ; la version globale (par zone tonale) est autonome et livrée d'abord.

---

# 5. Suppression de tache / correction

Aucun outil de clonage/correction pour poussières de capteur ou imperfections.

Intrinsèquement local : partage le problème de **référentiel de coordonnées** de l'item 2 (géométrie dessinée sur l'image affichée).

| Aspect | Analyse |
|---|---|
| Contrat | **Process +1**. **Schema : additif** — `spot_removal` optionnel (liste), absent = vide. |
| Pipeline (§3.1) | **Tranché, [ADR 0032](adr/0032-spot-removal-clone.md)** : étage **Suppression de tache** inséré immédiatement après Correction d'objectif et avant Balance des blancs — plus tôt que l'étage masqué d'[ADR 0029](adr/0029-process-6-local-adjustments.md), pour opérer sur des données proches du linéaire. Nouvelle process version (le prochain numéro disponible à la sortie, [ADR 0028](adr/0028-process-version-per-feature.md)). Référentiel de `crop` ([ADR 0026](adr/0026-mask-spot-coordinate-referential.md)). Le diagramme §3.1 sera amendé par la PR d'implémentation. |
| Crate | **Tranché, [ADR 0032](adr/0032-spot-removal-clone.md) : aucun nouveau crate.** `leyline-engine` — clonage seul, copie bilinéaire déterministe réutilisant la fonction `bilinear` existante du module process. Le *heal seamless* est **coupé de V2**. |
| Catalogue | **Tranché, [ADR 0032](adr/0032-spot-removal-clone.md) : aucune table dédiée.** Liste `spot_removal` compacte dans `settings_json`, cohérent avec « une révision = état complet et autonome » (`docs/catalog.md` §17). Une table par révision reste un problème de futur ADR *avec données réelles* si le volume l'exige un jour. |
| API moteur | **Tranché, [ADR 0032](adr/0032-spot-removal-clone.md) :** aucune méthode `EditSession` nouvelle — le cycle de vie des taches passe par `set`/`commit` plus l'extension `Param`/`Value` (indice dans le tableau `spot_removal`), même schéma qu'[ADR 0029](adr/0029-process-6-local-adjustments.md). |
| Complexité | **M** — clonage seul ([ADR 0032](adr/0032-spot-removal-clone.md)) ; le *heal* (**L**) est coupé de V2, plus dans la fourchette. |

Champs pressentis : `spot_removal: [ { target:{x,y}, source:{x,y}, radius, feather, opacity } ]` en coordonnées normalisées — **champ `mode` abandonné, [ADR 0032](adr/0032-spot-removal-clone.md)** : le clonage étant le seul mode V2, aucun champ de mode n'est nécessaire.

Questions ouvertes :

1. ~~**Déterminisme de la correction** : le *heal* (clonage sans couture type Poisson) doit produire les mêmes pixels à chaque rendu~~ — résolu, [ADR 0032](adr/0032-spot-removal-clone.md) : le *heal* est **coupé de V2** (mode, pas drapeau) — un blending de Poisson non validable sans images de référence, esprit d'[ADR 0016](adr/0016-process-3-lens-correction.md) ; seul le clonage (copie bilinéaire déterministe) est livré, et il est reproductible par construction.
2. ~~**Sélection automatique de la source** : si le moteur propose une source, la proposition doit être déterministe et enregistrée~~ — résolu, [ADR 0032](adr/0032-spot-removal-clone.md) : **source manuelle uniquement** en V2, aucune suggestion moteur ; si elle est ajoutée plus tard, elle s'écrit dans `spot_removal[].source` comme toute autre valeur, jamais recalculée au rendu.
3. ~~Référentiel de coordonnées~~ — résolu, [ADR 0026](adr/0026-mask-spot-coordinate-referential.md), commun à l'item 2.

---

# 6. Dehaze / texture / clarté

Seul « détail » existe (réduction de bruit + netteté). Pas de contrôles séparés clarté, texture, dehaze — trois traitements de contraste local / fréquentiel distincts.

| Aspect | Analyse |
|---|---|
| Contrat | **Process +1**. **Schema : additif** — trois curseurs optionnels, neutre 0. |
| Pipeline (§3.1) | **Tranché, [ADR 0033](adr/0033-clarity-texture-dehaze.md)** : ordre **clarté → texture → dehaze**, tous **avant Vibrance/Saturation** (donc avant l'étage masqué d'[ADR 0029](adr/0029-process-6-local-adjustments.md), dont la position reste intacte). Nouvelle process version (le prochain numéro disponible à la sortie, [ADR 0028](adr/0028-process-version-per-feature.md)). Le diagramme §3.1 sera amendé par la PR d'implémentation. |
| Crate | **Tranché, [ADR 0033](adr/0033-clarity-texture-dehaze.md) : aucun nouveau crate.** `leyline-engine`. Clarté/texture : **une seule** fonction de contraste local par masque flou, appelée à deux rayons (flou approché par sous-échantillonnage, pas de grand noyau plein résolution). Dehaze : *dark channel prior*, lumière atmosphérique par percentile en **forme close**, déterminisme figé. |
| Catalogue | Aucun. |
| API moteur | Nouveaux `Param` ; rejoignent un groupe présence/détail élargi pour les presets. |
| Complexité | **M** (clarté/texture) à **L** (dehaze). |

Champs pressentis : `clarity`, `texture`, `dehaze`, curseurs `[-100, +100]`, neutre 0 ([ADR 0033](adr/0033-clarity-texture-dehaze.md)).

Questions ouvertes :

1. ~~**Déterminisme du dehaze** : l'estimation atmosphérique se fige avec la process version~~ — résolu, [ADR 0033](adr/0033-clarity-texture-dehaze.md) : lumière atmosphérique par **percentile fixe du canal sombre**, sélection en **forme close** (aucune optimisation itérative), gelée avec la process version.
2. ~~**Global d'abord, masqué ensuite**~~ — résolu, [ADR 0033](adr/0033-clarity-texture-dehaze.md) : les trois curseurs sont livrés **en global** en V2 ; leur version masquée/régionale est déférée à un futur ADR adossé à [ADR 0029](adr/0029-process-6-local-adjustments.md) (même coupe qu'[ADR 0031](adr/0031-hsl-color-grading.md) pour le color grading régional).
3. ~~**Coût CPU** du contraste local multi-échelle sur les grandes previews~~ — résolu, [ADR 0033](adr/0033-clarity-texture-dehaze.md) : le flou à grand rayon est **approché par sous-échantillonnage** (type pyramide/filtre boîte), jamais un noyau gaussien plein résolution ; facteur exact laissé à la PR.

---

# 7. Épreuvage écran, filigrane, module d'impression

Aucun des trois n'est implémenté. **Important : c'est l'item le moins « pipeline »** — l'essentiel n'est pas une modification pixel de la révision stockée, mais du travail preview / export / gestion des couleurs / UI.

| Sous-item | Nature réelle | Contrat de rendu |
|---|---|---|
| Épreuvage écran (soft proofing) | Simulation **à l'affichage** au travers d'un profil ICC de destination, avec alerte de gamut. Ne modifie **ni** `settings_json` **ni** les pixels de la révision — c'est un mode de vue. | **Tranché, [ADR 0034](adr/0034-softproofing-watermark-print.md)** : **aucune** process/schema, transformation non persistée. Paramètre d'épreuvage **optionnel et vue seule** sur un appel de preview (`docs/engine-api.md` §11) — profil de destination + intention + alerte de gamut — jamais écrit au catalogue. Réutilise la primitive ICC d'[ADR 0027](adr/0027-color-management-beyond-srgb.md) dans `leyline-color`. |
| Filigrane | Surimpression **à l'export**, décoration d'étage de sortie au même titre que format/qualité. | **Tranché, [ADR 0034](adr/0034-softproofing-watermark-print.md)** : **aucune** process de développement. **Texte seul en V2** (image/logo coupée) ; champ `watermark` additif dans `ExportSettings`/`ExportRecipe` ([ADR 0025](adr/0025-unified-export-request.md), `docs/engine-api.md` §12), donc dans `export_presets.settings_json` (`docs/catalog.md` §27). Composité en tout dernier, **après** la transformation ICC de destination d'[ADR 0027](adr/0027-color-management-beyond-srgb.md), avant l'encodage. |
| Module d'impression | Sous-système mise en page + sortie (marges, profil imprimante, épreuvage). Surtout Studio + un chemin de rendu d'impression. | **Tranché, [ADR 0036](adr/0036-print-module.md)** : **aucune** process/schema de développement — « un export avec une dimension physique (`papier × DPI` au lieu de `max_edge`) et un profil de destination », réutilisant la plomberie de `leyline-export` et la primitive ICC d'[ADR 0027](adr/0027-color-management-beyond-srgb.md). **Une photo par page** (planches contact coupées de V2). Preset `print_presets` parallèle à `export_presets` (`docs/catalog.md` §27) ; `PrintRequest`/`PrintRecipe` sur la forme d'[ADR 0025](adr/0025-unified-export-request.md). Le **rendu** est moteur (extension `leyline-export`/`leyline-color`, pas de crate) ; le **hand-off à l'imprimante** (dialogue OS) vit dans `leyline-studio` (`docs/engine-api.md` §14, patron d'[ADR 0020](adr/0020-menu-bar.md)/[0021](adr/0021-context-menus.md)). Seul **risque ouvert laissé à la PR** : le mécanisme exact de hand-off OS (PDF portable vs raster + API plateforme vs surface Slint). |

| Aspect | Analyse |
|---|---|
| Crate | `leyline-color` (épreuvage, profils), `leyline-export` (filigrane), `leyline-studio` + chemin de sortie dédié (impression). Pas de nouveau crate évident. |
| Catalogue | Filigrane : champs dans `export_presets.settings_json`. Épreuvage/impression : rien de persistant côté développement. |
| API moteur | `ExportSettings` gagne le filigrane. La preview gagne une option de profil d'épreuve. |
| Complexité | Filigrane **S/M** ; épreuvage **M** ; impression **L/XL**. |

~~Question ouverte majeure : épreuvage, impression et export non-sRGB butent tous sur le gel sRGB de V1~~ — résolu, [ADR 0027](adr/0027-color-management-beyond-srgb.md) : le pipeline reste sRGB, la sortie (export, épreuvage) gagne une transformation ICC vers un profil de destination via `leyline-color` élargi. Fil transversal partagé avec l'item 8, voir §8.

~~Forme concrète de l'épreuvage et du filigrane~~ — résolue, [ADR 0034](adr/0034-softproofing-watermark-print.md) : filigrane **texte seul** (image/logo coupée) dans `ExportSettings`, composité après la transformation ICC de destination ; épreuvage = paramètre **vue seule** sur un appel de preview, jamais persisté ; les deux réutilisent la primitive ICC d'[ADR 0027](adr/0027-color-management-beyond-srgb.md). **Le module d'impression est désormais tranché lui aussi, [ADR 0036](adr/0036-print-module.md)** : « un export avec une dimension physique et un profil de destination » (aucune process, aucun étage pipeline), **une photo par page** (planches coupées), preset `print_presets`, rendu moteur et hand-off OS laissé à Studio — le seul point restant est le **risque ouvert** du mécanisme exact de hand-off OS (PDF portable vs raster + API plateforme vs surface Slint), flaggé pour la PR. L'item 7 est donc entièrement clos.

---

# 8. Auteur de profils caméra/objectif au-delà de Lensfun

Aucune calibration caméra personnalisée de type DCP. La correction d'objectif V1 (Lensfun, `docs/adr/0016`–`0018`, process 3–5) couvre distorsion/vignettage/TCA géométriques, pas la **couleur** du capteur.

Un profil DCP calibre le rendu couleur du capteur (matrices colorimétriques, tables TSL, courbe tonale, *look table*) — donc **très tôt** dans le pipeline, à la conversion RGB capteur → espace de travail.

| Aspect | Analyse |
|---|---|
| Contrat | **Process +1** (le prochain numéro disponible à la sortie, [ADR 0028](adr/0028-process-version-per-feature.md)), et **insertion d'un nouvel étage** de profil colorimétrique d'entrée dans §3.1 — réordonnancement = événement de process. **Schema : additif** (référence du profil). Tranché, [ADR 0035](adr/0035-camera-profile-dcp.md). |
| Pipeline (§3.1) | **Tranché, [ADR 0035](adr/0035-camera-profile-dcp.md)** : nouvel étage **profil caméra** en **tout premier**, entre RAW décodé et **Correction d'objectif** (précision de l'ordre : avant la correction géométrique, puisque DCP calibre la couleur et l'objectif la géométrie — aucune interaction, on opère sur les données les moins traitées, esprit du placement tôt d'[ADR 0032](adr/0032-spot-removal-clone.md)). Le diagramme §3.1 sera amendé par la PR d'implémentation. |
| Crate | **Tranché, [ADR 0035](adr/0035-camera-profile-dcp.md) : aucun nouveau crate — extension de `leyline-color`.** ADR 0027 en a déjà fait une bibliothèque de transformation couleur générale ; l'application DCP (matrice + LUT appliquées directement, pas via `cmsTransform`) est du travail de domaine couleur parallèle, sans couplage aux tampons du moteur (contraste avec le masquage d'[ADR 0029](adr/0029-process-6-local-adjustments.md), logé dans `leyline-engine` *parce qu'*il est couplé au tampon). Le **parseur DCP** (maison minimal ou crate existant) reste **un risque de dépendance ouvert**, laissé à la PR. |
| Catalogue | **Tranché, [ADR 0035](adr/0035-camera-profile-dcp.md) :** fichiers **fournis par l'utilisateur uniquement** (aucune base embarquée en V2, contraste avec Lensfun), déposés dans un dossier relatif (`Profiles/Camera/`, `docs/catalog.md` §2.3), **référencés par chemin relatif explicite** (pas d'auto-match EXIF), avec un **checksum BLAKE3** ([ADR 0006](adr/0006-blake3.md)) du fichier `.dcp` dans `settings_json`. Aucune table dédiée. |
| API moteur | Sélection de profil comme `Param` ; éventuellement une surface d'énumération des profils disponibles. |
| Complexité | **L/XL**. |

Questions ouvertes :

1. ~~**Dépendance DCP**~~ — résolu, [ADR 0037](adr/0037-dcp-parsing-dependency.md) : lecteur de tags maison minimal au-dessus du crate `tiff` déjà lié (lecture seule, dans `leyline-color`), pas de nouveau crate à vérifier. La **correctness colorimétrique**, elle, reste le risque ouvert inchangé posé par ADR 0035/0016 : validation contre de vrais DCP Adobe et leurs rendus de référence exigée avant sortie.
2. ~~Interaction avec le gel sRGB~~ — résolu, [ADR 0027](adr/0027-color-management-beyond-srgb.md) puis [ADR 0035](adr/0035-camera-profile-dcp.md) : `leyline-color` est déjà une bibliothèque de transformation couleur générale, et [ADR 0035](adr/0035-camera-profile-dcp.md) y loge l'étage DCP d'entrée (capteur → espace de travail) qu'ADR 0027 ne couvrait pas, en **tout premier** du pipeline.
3. ~~**Reproductibilité** d'un chemin couleur entièrement nouveau, gelé par process version~~ — résolue, [ADR 0035](adr/0035-camera-profile-dcp.md) : nouvelle process version (le prochain numéro disponible, [ADR 0028](adr/0028-process-version-per-feature.md)) ; le fichier `.dcp` externe référencé est **checksummé en BLAKE3** ([ADR 0006](adr/0006-blake3.md)), un problème genuinely nouveau (les autres ADR stockent leur géométrie inline). Un checksum non concordant déclenche le mode d'échec existant §3.4 (« ne modifie rien, avertis »), pas une catégorie nouvelle — extension du contrat `docs/pipeline.md` §5 à un intrant référencé de l'extérieur.

---

# 9. Fils transversaux et éligibilité ADR

Trois fils traversent les huit items. Deux étaient des verrous à trancher **avant** les fonctionnalités qui en dépendent — les deux sont maintenant résolus :

* **Contrat de coordonnées post-recadrage — résolu, [ADR 0026](adr/0026-mask-spot-coordinate-referential.md).** Items 2, 4 et le color grading régional (3) dessinent de la géométrie sur l'image affichée, alors que §3.1 applique Rotation/Recadrage en fin de chaîne. La géométrie se stocke désormais dans le même référentiel que `crop` (normalisé, après rotation, avant recadrage) ; le moteur la fait remonter au tampon pré-rotation par la même famille de remapping arrière que `rotate`/la correction d'objectif.
* **Primitive de masquage générique — architecture tranchée, [ADR 0029](adr/0029-process-6-local-adjustments.md).** L'item 2 est l'infrastructure sur laquelle s'adossent l'item 5 (spatial), le dehaze masqué (6) et le color grading régional (3). Construire le masquage d'abord évite de recoder trois fois la même couverture spatiale. L'ADR 0029 fixe le socle : process 6, étage masqué réutilisant les opérateurs globaux, masques (brosse/radial/gradient) stockés dans `settings_json`, pilotés par `Param::LocalAdjustment`. Le color grading (3), la courbe (4) et clarté/texture/dehaze (6) restent livrables **en global** sans attendre les masques ; leur version régionale, elle, s'adosse à ce socle.
* **Gestion des couleurs au-delà de sRGB — résolu, [ADR 0027](adr/0027-color-management-beyond-srgb.md).** Items 7 (épreuvage, impression, export non-sRGB) et 8 (DCP) butaient tous sur le gel sRGB d'ADR 0015. Le pipeline de rendu reste sRGB (aucune process version rouverte) ; `leyline-color` s'élargit en bibliothèque de transformation ICC générale pour porter épreuvage et export vers un profil de destination. L'item 8 (DCP) reste distinct : il touche le **début** du pipeline (capteur → espace de travail), pas la sortie — son propre ADR reste à écrire.

Un quatrième point, non bloquant mais structurant : **la prolifération des process versions — résolu, [ADR 0028](adr/0028-process-version-per-feature.md).** La maison duplique un module `processN.rs` complet par version (ADR 0013, 0016) pour garantir le gel « mêmes pixels dans dix ans ». Empiler plusieurs fonctionnalités pixel en V2 signifie soit plusieurs bumps (plusieurs modules dupliqués), soit un bump groupé. Le choix est tranché : **un process par fonctionnalité**, chaque fonctionnalité pixel gardant son propre module gelé et pouvant sortir indépendamment. Le bump groupé est écarté (il n'économise aucune duplication, tout correctif ultérieur d'une fonctionnalité groupée forçant malgré tout un module complet de plus), de même qu'une bibliothèque d'opérateurs partagés (elle rouvrirait le risque d'altération silencieuse d'un rendu gelé que la duplication existe pour supprimer).

Éligibilité ADR une fois la décision prise (aucun n'est décidé aujourd'hui) :

| Item | ADR propre ? |
|---|---|
| 2 — Réglages locaux / masqués | **[ADR 0029](adr/0029-process-6-local-adjustments.md) — le socle est tranché** (process 6, étage masqué, référentiel, stockage, API, presets). D'autres ADR pourront suivre pour des raffinements par type d'outil, mais l'infrastructure ne les attend plus. |
| 3 — Courbe tonale | **[ADR 0030](adr/0030-tone-curve.md) — tranché** : courbe par points seule (mode paramétrique coupé), spline cubique monotone gelée, luminance seule, précalcul en LUT. |
| 4 — Color grading / TSL | **[ADR 0031](adr/0031-hsl-color-grading.md) — tranché** : HSL dérivé du RGB (8 bandes + falloff), zones de color grading pondérées par luminance ; color grading régional déféré. |
| 5 — Suppression de tache | **[ADR 0032](adr/0032-spot-removal-clone.md) — tranché** : clonage seul (heal coupé de V2), copie bilinéaire déterministe, étage tôt dans le pipeline (après Correction d'objectif), source manuelle, `Param`/`Value` d'[ADR 0029](adr/0029-process-6-local-adjustments.md). |
| 6 — Dehaze / texture / clarté | **[ADR 0033](adr/0033-clarity-texture-dehaze.md) — tranché** (ADR groupé pour les trois) : clarté/texture en contraste local unifié à deux rayons, dehaze par dark channel prior en forme close ; globaux en V2, masqué déféré. |
| 7 — Épreuvage / filigrane / impression | **Item entièrement tranché.** Épreuvage + filigrane, **[ADR 0034](adr/0034-softproofing-watermark-print.md)** — et, comme pressenti, **pas** un ADR de développement (aucun process, aucun étage pipeline) : filigrane texte (export, `ExportSettings`), épreuvage vue seule (preview), les deux sur la primitive couleur d'[ADR 0027](adr/0027-color-management-beyond-srgb.md) qui a complété ADR 0015. **Module d'impression tranché, [ADR 0036](adr/0036-print-module.md)** : « un export avec une dimension physique et un profil de destination » (aucun process, aucun étage), une photo par page (planches coupées), preset `print_presets`, rendu moteur, hand-off OS laissé à Studio (patron d'[ADR 0020](adr/0020-menu-bar.md)/[0021](adr/0021-context-menus.md)). Seul le **mécanisme exact de hand-off OS** reste un risque ouvert assumé, laissé à la PR. |
| 8 — Profils caméra (DCP) | **[ADR 0035](adr/0035-camera-profile-dcp.md) — tranché** : nouveau process (le prochain disponible, [ADR 0028](adr/0028-process-version-per-feature.md)), nouvel étage colorimétrique en tête de pipeline (avant la correction d'objectif), extension de `leyline-color` (pas de nouveau crate), fichiers utilisateur référencés par chemin relatif et checksummés BLAKE3. La **dépendance de parsing DCP est désormais résolue, [ADR 0037](adr/0037-dcp-parsing-dependency.md)** : lecteur de tags maison minimal au-dessus du crate `tiff` déjà lié (lecture seule, dans `leyline-color`), pas de nouveau crate à vérifier ; la **correctness colorimétrique** reste, elle, le risque ouvert inchangé d'ADR 0035 (validation contre de vrais DCP Adobe, barre d'[ADR 0016](adr/0016-process-3-lens-correction.md)). |
