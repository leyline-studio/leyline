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
| Pipeline (§3.1) | S'insère dans le bloc tonal, après Blancs/Noirs et avant Vibrance/Saturation. Insertion = événement de process, sans réordonnancement du reste. |
| Crate | `leyline-engine` : construction d'une LUT 1D depuis les points de contrôle. `leyline-core` porte les champs. Aucun nouveau crate. |
| Catalogue | Aucun. Tout tient dans `settings_json`. |
| API moteur | Nouveaux `Param` (points de courbe, mode paramétrique). Rejoint un `SettingsGroup::Tone` élargi ou un groupe dédié. |
| Complexité | **S/M**. |

Champs pressentis (`settings_json`, schema inchangé) :

| Champ | Rôle | Valeur neutre |
|---|---|---|
| `tone_curve.points` | Courbe par points, liste `{x, y}` normalisés `[0,1]` | absent — identité |
| `tone_curve.parametric` | Régions highlights/lights/darks/shadows + points de bascule | absent — 0 partout |
| `tone_curve.channel` | Cible : `rgb` (luminance) ou `r`/`g`/`b` | `rgb` |

Questions ouvertes :

1. **Interpolation gelée** : la spline (cubique monotone recommandée) fait partie du contrat de rendu — deux moteurs doivent produire les mêmes pixels (`docs/pipeline.md` §5). Le choix se fige avec la process version.
2. **Courbes par canal RGB** dès la V2 ou luminance seule d'abord.

---

# 4. Color grading / mélangeur TSL

Vibrance et saturation existent (globales). Il manque le mélangeur **TSL par teinte** (8 bandes teinte/saturation/luminance) et les **roues de color grading** ombres/tons moyens/hautes lumières (couleur + luminance), à la manière du *color balance rgb* de Darktable ou du panneau *color grading* de Lightroom.

| Aspect | Analyse |
|---|---|
| Contrat | **Process +1**. **Schema : additif** — `hsl` et `color_grading` optionnels, absents = neutres. |
| Pipeline (§3.1) | Bloc couleur, après Vibrance/Saturation. Insertion = événement de process. |
| Crate | `leyline-engine`. Aucun nouveau crate. |
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

1. **Modèle de teinte gelé** : la définition exacte de la teinte et l'espace de calcul font partie du contrat de rendu, à figer avec la process version.
2. **Color grading régional** : appliquer les roues sous masque (item 2) est une extension naturelle — à considérer dans le référentiel commun, pas à recoder.

---

# 5. Suppression de tache / correction

Aucun outil de clonage/correction pour poussières de capteur ou imperfections.

Intrinsèquement local : partage le problème de **référentiel de coordonnées** de l'item 2 (géométrie dessinée sur l'image affichée).

| Aspect | Analyse |
|---|---|
| Contrat | **Process +1**. **Schema : additif** — `spot_removal` optionnel (liste), absent = vide. |
| Pipeline (§3.1) | Tôt dans la chaîne (avant le bloc tonal), à la manière de Lightroom, pour opérer sur des données proches du linéaire. Placement à trancher — même référentiel que les masques. |
| Crate | `leyline-engine` (clonage trivial ; correction *seamless* plus lourde). Un crate dédié n'est pas justifié d'emblée. |
| Catalogue | Liste de géométries par révision. Tient dans `settings_json` (liste compacte), cohérent avec « état complet et autonome » (§17). Une table par révision est l'alternative si les listes explosent, mais elle casserait l'autonomie de `settings_json` — écarté par défaut. |
| API moteur | `EditSession` : `Param` d'ajout/déplacement/suppression de taches. |
| Complexité | **M** (clone) à **L** (heal). |

Champs pressentis : `spot_removal: [ { target:{x,y}, source:{x,y}, radius, feather, mode: "clone"|"heal", opacity } ]` en coordonnées normalisées.

Questions ouvertes :

1. **Déterminisme de la correction** : le *heal* (clonage sans couture type Poisson) doit produire les mêmes pixels à chaque rendu (`docs/pipeline.md` §5) — l'algorithme se fige avec la process version.
2. **Sélection automatique de la source** : si le moteur propose une source, la proposition doit être déterministe et **enregistrée** dans les paramètres (§5 reproductibilité), jamais recalculée à la volée.
3. ~~Référentiel de coordonnées~~ — résolu, [ADR 0026](adr/0026-mask-spot-coordinate-referential.md), commun à l'item 2.

---

# 6. Dehaze / texture / clarté

Seul « détail » existe (réduction de bruit + netteté). Pas de contrôles séparés clarté, texture, dehaze — trois traitements de contraste local / fréquentiel distincts.

| Aspect | Analyse |
|---|---|
| Contrat | **Process +1**. **Schema : additif** — trois curseurs optionnels, neutre 0. |
| Pipeline (§3.1) | Clarté/texture (contraste local) dans le bloc tonal/présence ; dehaze après le bloc tonal. Insertion = événement de process. |
| Crate | `leyline-engine`. Clarté/texture : contraste local multi-échelle (masque flou à grand rayon / filtre guidé). Dehaze : estimation de la lumière atmosphérique (*dark channel prior*), déterminisme à figer. |
| Catalogue | Aucun. |
| API moteur | Nouveaux `Param` ; rejoignent un groupe présence/détail élargi pour les presets. |
| Complexité | **M** (clarté/texture) à **L** (dehaze). |

Champs pressentis : `clarity`, `texture`, `dehaze`, curseurs `[-100, +100]`, neutre 0.

Questions ouvertes :

1. **Déterminisme du dehaze** : l'estimation atmosphérique se fige avec la process version.
2. **Global d'abord, masqué ensuite** : des curseurs globaux sont livrables sans l'item 2 ; leur version masquée s'y adosse ensuite.
3. **Coût CPU** du contraste local multi-échelle sur les grandes previews (`docs/engine-api.md` §11).

---

# 7. Épreuvage écran, filigrane, module d'impression

Aucun des trois n'est implémenté. **Important : c'est l'item le moins « pipeline »** — l'essentiel n'est pas une modification pixel de la révision stockée, mais du travail preview / export / gestion des couleurs / UI.

| Sous-item | Nature réelle | Contrat de rendu |
|---|---|---|
| Épreuvage écran (soft proofing) | Simulation **à l'affichage** au travers d'un profil ICC de destination, avec alerte de gamut. Ne modifie **ni** `settings_json` **ni** les pixels de la révision — c'est un mode de vue. | **Aucune** process/schema : transformation non persistée. Touche `leyline-color` (LittleCMS, déjà dépendance) et l'API preview (intention de rendu / profil d'épreuve). |
| Filigrane | Surimpression **à l'export**, décoration d'étage de sortie au même titre que format/qualité. | **Aucune** process de développement. Vit dans `leyline-export` et `ExportSettings`/`ExportRecipe` (`docs/engine-api.md` §12) ; config dans `export_presets.settings_json` (`docs/catalog.md` §27). |
| Module d'impression | Sous-système mise en page + sortie (marges, planches, profil imprimante, épreuvage). Surtout Studio + un chemin de rendu d'impression. | **Aucune** process de développement ; large chantier UI + sortie. |

| Aspect | Analyse |
|---|---|
| Crate | `leyline-color` (épreuvage, profils), `leyline-export` (filigrane), `leyline-studio` + chemin de sortie dédié (impression). Pas de nouveau crate évident. |
| Catalogue | Filigrane : champs dans `export_presets.settings_json`. Épreuvage/impression : rien de persistant côté développement. |
| API moteur | `ExportSettings` gagne le filigrane. La preview gagne une option de profil d'épreuve. |
| Complexité | Filigrane **S/M** ; épreuvage **M** ; impression **L/XL**. |

~~Question ouverte majeure : épreuvage, impression et export non-sRGB butent tous sur le gel sRGB de V1~~ — résolu, [ADR 0027](adr/0027-color-management-beyond-srgb.md) : le pipeline reste sRGB, la sortie (export, épreuvage) gagne une transformation ICC vers un profil de destination via `leyline-color` élargi. Fil transversal partagé avec l'item 8, voir §8.

---

# 8. Auteur de profils caméra/objectif au-delà de Lensfun

Aucune calibration caméra personnalisée de type DCP. La correction d'objectif V1 (Lensfun, `docs/adr/0016`–`0018`, process 3–5) couvre distorsion/vignettage/TCA géométriques, pas la **couleur** du capteur.

Un profil DCP calibre le rendu couleur du capteur (matrices colorimétriques, tables TSL, courbe tonale, *look table*) — donc **très tôt** dans le pipeline, à la conversion RGB capteur → espace de travail.

| Aspect | Analyse |
|---|---|
| Contrat | **Process +1**, et **insertion d'un nouvel étage** de profil colorimétrique d'entrée dans §3.1 (aujourd'hui « RAW décodé → Correction d'objectif → Balance des blancs » sans étape colorimétrique explicite) — réordonnancement = événement de process. **Schema : additif** (sélection du profil). |
| Pipeline (§3.1) | Nouvel étage **profil caméra** entre RAW décodé et Balance des blancs. |
| Crate | Un **parseur DCP** est nécessaire : LittleCMS gère l'ICC, pas le format DCP (Adobe). Soit extension de `leyline-color`, soit un nouveau crate `leyline-profile`. **Risque de dépendance externe** (parsing DCP + science des couleurs) le plus élevé des huit items. |
| Catalogue | Le profil sélectionné se référence dans `settings_json` (nom/id). Les fichiers DCP eux-mêmes : dossier de profils de la bibliothèque ou données applicatives, référencés en **chemins relatifs** (`docs/catalog.md` §2.3). Une table `camera_profiles` est une option si l'on veut les cataloguer plutôt que les lire du disque — à trancher. |
| API moteur | Sélection de profil comme `Param` ; éventuellement une surface d'énumération des profils disponibles. |
| Complexité | **L/XL**. |

Questions ouvertes :

1. **Dépendance DCP** : parseur à écrire ou à intégrer, correctness colorimétrique à valider sans images de référence Adobe sous la main (même prudence qu'ADR 0016 §Alternatives sur le vignettage/TCA).
2. ~~Interaction avec le gel sRGB~~ — résolu en partie par [ADR 0027](adr/0027-color-management-beyond-srgb.md) : `leyline-color` sera déjà une bibliothèque de transformation ICC générale au moment où cet item se décide. Reste propre à l'item 8 : l'étage DCP touche le **début** du pipeline (capteur → espace de travail), qu'ADR 0027 ne couvre pas — son propre ADR reste à écrire.
3. **Reproductibilité** d'un chemin couleur entièrement nouveau, gelé par process version.

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
| 3 — Color grading / TSL | Oui — nouveau process, modèle de teinte gelé. |
| 4 — Courbe tonale | Oui — nouveau process, interpolation gelée (ADR léger). |
| 5 — Suppression de tache | Oui — nouveau process, déterminisme du heal, référentiel partagé. |
| 6 — Dehaze / texture / clarté | Oui — nouveau process ; possiblement un ADR groupé pour les trois. |
| 7 — Épreuvage / filigrane / impression | Oui, mais **pas** un ADR de développement : filigrane (export), épreuvage/impression (couleur + UI). Dépend d'un ADR préalable d'élargissement couleur (remplaçant ou complétant ADR 0015). |
| 8 — Profils caméra (DCP) | Oui — nouveau process, nouvel étage pipeline, dépendance externe, chemin couleur. ADR lourd, adossé au même élargissement couleur que l'item 7. |
