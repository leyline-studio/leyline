# Architecture Decision Records

Chaque décision structurante est consignée ici : contexte, décision, conséquences, alternatives écartées.

Une décision acceptée ne se modifie pas : elle se remplace par un nouvel ADR qui la référence.

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
| [0028](0028-process-version-per-feature.md) | Une process version par fonctionnalité pixel : maintien de la duplication par module |
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
