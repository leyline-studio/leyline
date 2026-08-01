# Architecture Decision Records

Chaque décision structurante est consignée ici, selon le même plan : **contexte**, **décision**, **conséquences**, **alternatives écartées**. C'est la dernière rubrique qui fait la valeur d'un ADR — elle dit ce qui a été envisagé puis rejeté, et pourquoi, ce qu'aucune lecture du code ne permet de reconstituer.

## Comment lire ce dossier

Il n'est pas fait pour être lu d'un bout à l'autre. Trois usages :

* **Comprendre une brique externe** — 0001 à 0005 (Rust, Slint, SQLite, LibRaw, Lensfun/LittleCMS).
* **Comprendre le modèle de données** — 0007 (modèle inspiré de Git), 0008 (la version comme unité), 0010 (chemins relatifs).
* **Comprendre le rendu** — 0012 (parallélisme), 0028 puis **0042** et **0043** (versionnage du rendu), et les ADR de fonctionnalité pixel 0013, 0016–0018, 0029–0035.

## Règle d'édition

**Avant la première publication publique, un ADR reste librement modifiable** : un ADR qui ne correspond plus à l'implémentation est corrigé sur place, et non remplacé. Cette souplesse s'arrête le jour de l'ouverture au monde — les décisions acceptées gèlent alors, et sont remplacées par de nouveaux ADR qui les référencent, jamais éditées.

Ce qui ne se relâche d'aucun côté de cette ligne : **décider et documenter avant de coder**.

Un ADR remplacé n'est jamais supprimé : il reste lisible, avec sa raison d'époque. [ADR 0028](0028-process-version-per-feature.md) en est l'exemple — son raisonnement était correct pour les données dont il disposait, et il avait lui-même prévu sa réouverture.

| ADR | Décision |
|---|---|
| [0001](0001-rust.md) | Rust comme langage unique |
| [0002](0002-slint.md) | Slint pour l'interface graphique |
| [0003](0003-sqlite.md) | SQLite comme unique base du catalogue |
| [0004](0004-libraw.md) | LibRaw (branche LGPL) pour le décodage RAW |
| [0005](0005-lensfun-littlecms.md) | Lensfun et LittleCMS |
| [0006](0006-blake3.md) | BLAKE3 pour les checksums |
| [0007](0007-git-develop-model.md) | Modèle de développement inspiré de Git |
| [0008](0008-version-as-library-unit.md) | La version comme unité de bibliothèque |
| [0009](0009-gpl3-cla-dual-license.md) | GPL-3.0 + CLA, modèle double licence |
| [0010](0010-relative-paths.md) | Bibliothèque autonome, chemins relatifs |
| [0011](0011-engine-api-model.md) | API moteur : lib Rust, sync/async, événements |
| [0012](0012-rayon-data-parallelism.md) | Parallélisme de données Rayon, rendu bit-pour-bit |
| [0013](0013-process-2-lut-transfer.md) | Process 2 : fonctions de transfert sRGB par table |
| [0014](0014-develop-presets.md) | Presets de développement : catégories partielles, application par révision normale |
| [0015](0015-color-management-srgb.md) | Gestion des couleurs V1 : pipeline et export figés en sRGB |
| [0016](0016-process-3-lens-correction.md) | Process 3 : correction géométrique d'objectif (Lensfun) |
| [0017](0017-process-4-vignetting.md) | Process 4 : correction du vignettage (Lensfun) |
| [0018](0018-process-5-tca.md) | Process 5 : aberration chromatique transversale (Lensfun) |
| [0019](0019-distribution-i18n.md) | Distribution : installateur par plateforme et internationalisation FR/EN |
| [0020](0020-menu-bar.md) | Barre de menu comme surface de commandes |
| [0021](0021-context-menus.md) | Menus contextuels (clic droit) |
| [0022](0022-default-library-fallback.md) | Bibliothèque par défaut au premier lancement |
| [0023](0023-catalog-lock-narrowing-preview.md) | Ne pas tenir le verrou catalogue pendant un rendu de preview |
| [0024](0024-catalog-lock-narrowing-export.md) | Ne pas tenir le verrou catalogue pendant un rendu d'export |
| [0025](0025-unified-export-request.md) | Une seule requête d'export : `ExportRequest`/`ExportRecipe` |
| [0026](0026-mask-spot-coordinate-referential.md) | Référentiel de coordonnées des masques et corrections locales : celui de `crop` |
| [0027](0027-color-management-beyond-srgb.md) | Gestion des couleurs au-delà de sRGB : profil de sortie découplé du rendu |
| [0028](0028-process-version-per-feature.md) | ~~Une process version par fonctionnalité pixel : duplication par module~~ — **remplacé par [0042](0042-versioned-stage-pipeline.md)** |
| [0029](0029-process-6-local-adjustments.md) | Process 6 : réglages locaux masqués (brosse, radial, gradient), stockés dans `settings_json`, pilotés par `Param::LocalAdjustment` |
| [0030](0030-tone-curve.md) | Courbe tonale : courbe par points seule, spline cubique monotone, appliquée en luminance via LUT |
| [0031](0031-hsl-color-grading.md) | Mélangeur TSL et roues de color grading : HSL dérivé du RGB, zones tonales pondérées par luminance |
| [0032](0032-spot-removal-clone.md) | Suppression de tache : clonage seul (heal coupé de V2), copie bilinéaire déterministe, tôt dans le pipeline |
| [0033](0033-clarity-texture-dehaze.md) | Clarté/texture (contraste local unifié à deux rayons) et dehaze (dark channel prior en forme close), globaux en V2 |
| [0034](0034-softproofing-watermark-print.md) | Épreuvage écran (vue seule) et filigrane texte (à l'export) : deux surfaces de sortie, aucun process ; le module d'impression reste hors décision |
| [0035](0035-camera-profile-dcp.md) | Profil caméra DCP : nouvel étage colorimétrique en tête de pipeline, fichiers fournis par l'utilisateur, référencés par chemin relatif et checksummés (BLAKE3) |
| [0036](0036-print-module.md) | Module d'impression : « export avec dimension physique + profil de destination », une photo par page (planches coupées), hand-off OS laissé à Studio |
| [0037](0037-dcp-parsing-dependency.md) | Dépendance de parsing DCP : lecteur de tags maison minimal au-dessus du crate `tiff` déjà lié, lecture seule, dans `leyline-color` |
| [0038](0038-tethered-capture.md) | Capture tethering : import direct depuis l'appareil via USB (libgphoto2), une capture est un import comme un autre |
| [0039](0039-watched-folder-import.md) | Import automatique par dossier surveillé (`notify`), un fichier stabilisé à la fois, même cœur d'import que le tethering |
| [0040](0040-gps-map-view.md) | Vue carte GPS : tuiles MBTiles hors-ligne fournies par l'utilisateur, aucun appel réseau, GPS extrait des RAW via LibRaw |
| [0041](0041-interactive-preview-rendering.md) | Rendu interactif : preview développée à la résolution d'affichage (rayons pixel mis à l'échelle) et cache d'étages de pipeline ; export inchangé, aucune nouvelle process version |
| [0042](0042-versioned-stage-pipeline.md) | Pipeline composé d'étages versionnés : le gel porte sur l'opérateur et non sur la version entière ; remplace ADR 0028, §5 amendé par 0043 |
| [0043](0043-collapse-prerelease-render-history.md) | Effondrement de l'historique de rendu avant publication : une version par opérateur, la révision porte sa carte `stages`, `process` disparaît |
| [0044](0044-linear-wide-gamut-working-space.md) | Tampon de travail en lumière linéaire large gamut : Rec. 2020 D65 non borné, étages `input`/`output_rendering`, espace déclaré par version d'étage |
| [0045](0045-studio-ui-modularisation.md) | Modularisation de l'interface Studio : un `global` Slint par domaine, un fichier par panneau/dialogue, modules Rust `wiring/` en miroir ; frontière SDK inchangée |
| [0046](0046-edge-preserving-denoise.md) | Débruitage préservant les contours : ondelettes à trous et seuillage doux par échelle, `noise_luminance::v2` / `noise_color::v2` (premier `v2` réel d'ADR 0042) |
| [0047](0047-xmp-sidecar-read.md) | Lecture des sidecars XMP : une amorce à l'import et à la demande, jamais une synchronisation ; remplir sans écraser, `roxmltree` pour le parsing |
| [0048](0048-range-masks.md) | Masques par plage : raffinement de luminance et de teinte multipliant la couverture géométrique, `local_adjustments::v2` ; un réglage inexprimable par la version épinglée est refusé |
| [0049](0049-local-adjustments-clients.md) | Exposer les retouches locales aux clients : outils de tracé sur la preview dans Studio (« neutre = absent »), payload JSON du `LocalAdjustment` stocké dans la CLI |
| [0050](0050-highlight-reconstruction.md) | Reconstruction des hautes lumières : modes `clip`/`blend`/`rebuild` de LibRaw avant dématriçage, épinglés par `input::v2` ; refus de validation sur une révision en `input: 1` |
| [0051](0051-watermark-rasterization-and-soft-proof-surface.md) | Filigrane texte rastérisé par `ab_glyph` avec police DejaVu Sans embarquée, dessiné dans `encode` ; épreuvage écran = `Library::preview_soft_proofed` en mémoire, alerte de gamut par LittleCMS |
| [0052](0052-perspective-correction.md) | Correction de perspective : homographie à deux curseurs (vertical, horizontal), étage `perspective` au rang 205 entre rotation et recadrage ; boîte englobante, pas de recadrage automatique |
| [0053](0053-creative-lut.md) | LUT créative : fichiers `.cube` importés dans `Profiles/LUT/` et référencés avec checksum, appliqués sur l'axe d'affichage au rang 160, dosables ; interpolation trilinéaire |
| [0054](0054-first-run-and-basic-mode.md) | Prise en main : une bibliothèque vide qui explique les deux gestes d'entrée, et un mode Basique par défaut dans develop (six groupes, deux outils) que le mode Complet rouvre en un clic ; aucun réglage ni rendu touché |
| [0055](0055-library-navigation.md) | Navigation de la bibliothèque : sélecteur de module dans la barre de menus, arbre des dossiers en lecture seule à gauche, barre d'outils (taille de vignette, loupe sur `E`), filmstrip partagé et badges de cellule ; aucune écriture nouvelle au catalogue |
| [0056](0056-non-raw-exif-import.md) | Métadonnées EXIF des fichiers non-RAW à l'import (`kamadak-exif` dans le moteur, à côté du lecteur XMP) : LibRaw garde la préséance, lecture seule, meilleur effort, `OffsetTimeOriginal` renseigne `capture_offset_minutes` |
| [0057](0057-compare-and-survey.md) | Départager deux photos : vue Comparaison à zoom et déplacement liés en coordonnées normalisées (retenue à gauche, candidate à droite) et vue Mosaïque qui réduit la sélection ; aucune écriture, previews en cache |
| [0058](0058-preset-provenance-and-shelf.md) | Préréglages : panneau gauche dans develop, un niveau de dossiers et des favoris, contenu lisible et essayable au survol, et surtout **provenance** — la révision enregistre de quel préréglage et de quelle version elle vient, dans le catalogue et non dans `settings_json` |
