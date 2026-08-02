# Spécification

Ce document répond à : **qu'est-ce que Leyline fait, qu'est-ce qu'il ne fera pas**. Il dit aussi ce qui est réellement livré — l'état phase par phase est dans [`roadmap.md`](roadmap.md).

Toutes les fonctionnalités listées comme livrées le sont sur les **trois clients** : Studio, la CLI et le SDK.

---

## 1. Périmètre V1 — livré et clos

**Bibliothèque et catalogue**

* Import d'un dossier (RAW, DNG, JPEG, PNG, TIFF), en copie ou par référence
* Catalogue SQLite — bibliothèques, collections (manuelles et dynamiques), mots-clés, notes, libellés de couleur, statut de sélection
* Retirer des photos du catalogue, ou les supprimer du disque vers la corbeille du système — deux gestes distincts ([ADR 0060](adr/0060-asset-removal.md))
* Miniatures et aperçus en cache
* Lecture EXIF, recherche plein texte
* Sidecars XMP : écriture à la demande, et lecture comme amorce à l'import — le chemin de migration depuis un autre logiciel ([ADR 0047](adr/0047-xmp-sidecar-read.md))
* Capture tethering USB — chaque photo importée dès la prise de vue ([ADR 0038](adr/0038-tethered-capture.md))
* Import automatique par dossier surveillé ([ADR 0039](adr/0039-watched-folder-import.md))
* Vue carte GPS : fond de carte mondial embarqué ([ADR 0059](adr/0059-bundled-world-basemap.md)), affiné par un pack MBTiles hors-ligne que l'utilisateur apporte s'il veut du détail ; aucun appel réseau ([ADR 0040](adr/0040-gps-map-view.md))

**Développement non destructif**

* Exposition, balance des blancs, contraste
* Ombres / hautes lumières, blancs / noirs
* Vibrance / saturation
* Rotation, recadrage
* Choix de l'algorithme de dématriçage : AHD, VNG, DCB, DHT ([ADR 0061](adr/0061-demosaic-algorithm.md))
* Réduction du bruit (luminance et chroma, préservant les contours — [ADR 0046](adr/0046-edge-preserving-denoise.md)), netteté
* Correction d'objectif via Lensfun : distorsion, vignettage, aberration chromatique transversale ([ADR 0016](adr/0016-process-3-lens-correction.md), [0017](adr/0017-process-4-vignetting.md), [0018](adr/0018-process-5-tca.md))
* Gestion des couleurs via LittleCMS ([ADR 0015](adr/0015-color-management-srgb.md), [ADR 0027](adr/0027-color-management-beyond-srgb.md))
* Presets de développement : créer, appliquer, appliquer en lot ([`presets.md`](presets.md), [ADR 0014](adr/0014-develop-presets.md))
* Retraitement d'une photo vers les versions d'étages courantes

**Sortie**

* Export JPEG, TIFF, PNG, WebP, AVIF — presets d'export, export par lots
* Module d'impression : dimension physique, marges, profil de destination ([ADR 0036](adr/0036-print-module.md))

**Distribution**

* Installateur par plateforme (Windows, macOS, Linux), dossier d'installation au choix quand la plateforme le permet ([ADR 0019](adr/0019-distribution-i18n.md))
* Interface multilingue (français, anglais), extensible sans changement de code

---

## 2. Au-delà de la V1 — livré

Ces fonctionnalités étaient cadrées comme candidates post-V1 dans [`v2-scope.md`](v2-scope.md). Elles sont implémentées.

| Fonctionnalité | Décision |
|---|---|
| Courbe tonale (spline cubique monotone, appliquée en luminance) | [ADR 0030](adr/0030-tone-curve.md) |
| Suppression de tache (clonage déterministe, sans mode *heal*) | [ADR 0032](adr/0032-spot-removal-clone.md) |
| Réglages locaux masqués — brosse, radial, gradué | [ADR 0029](adr/0029-process-6-local-adjustments.md) |
| Masques par plage — bande de luminance et bande de teinte, raffinant un masque géométrique | [ADR 0048](adr/0048-range-masks.md) |
| Mélangeur TSL et color grading | [ADR 0031](adr/0031-hsl-color-grading.md) |
| Clarté, texture, dehaze | [ADR 0033](adr/0033-clarity-texture-dehaze.md) |
| Profils caméra DCP — **expérimental** | [ADR 0035](adr/0035-camera-profile-dcp.md), [ADR 0037](adr/0037-dcp-parsing-dependency.md) |
| Correction de perspective (deux curseurs, homographie) | [ADR 0052](adr/0052-perspective-correction.md) |
| LUT créative `.cube` importée dans la bibliothèque, dosable | [ADR 0053](adr/0053-creative-lut.md) |
| Reconstruction des hautes lumières écrêtées (`clip`/`blend`/`rebuild`, avant dématriçage) | [ADR 0050](adr/0050-highlight-reconstruction.md) |
| Filigrane texte à l'export et épreuvage écran (vue seule) | [ADR 0034](adr/0034-softproofing-watermark-print.md), [ADR 0051](adr/0051-watermark-rasterization-and-soft-proof-surface.md) |

Les réglages locaux masqués et leurs masques par plage sont exposés dans les trois clients depuis [ADR 0049](adr/0049-local-adjustments-clients.md) : outils de tracé et éditeur dans Studio, payload JSON du `LocalAdjustment` stocké dans la CLI. Ce qu'ADR 0049 laisse hors périmètre, et qui reste à faire : la surimpression de la couverture calculée par le moteur (le « masque rouge »), la pipette de plage, et les poignées de déplacement d'une géométrie déjà tracée.

La mention *expérimental* est littérale : la justesse colorimétrique du chemin matriciel DCP n'a pas été validée contre de vrais `.dcp` Adobe et leurs rendus de référence, et les tables `ProfileHueSatMapData` / `ProfileLookTableData` / `ProfileToneCurve` ne sont pas appliquées. Studio et la CLI le signalent à l'utilisateur.

---

## 3. Décidé, non implémenté

| Sujet | Décision |
|---|---|
| Profil de bruit mesuré par boîtier et par sensibilité | À trancher par son propre ADR ([ADR 0046](adr/0046-edge-preserving-denoise.md) §7) |
| Filigrane image (logo) | Coupé par [ADR 0034](adr/0034-softproofing-watermark-print.md) : le problème de référence de ressource n'est pas tranché |

---

## 4. Exclusions volontaires

Ces absences sont des **décisions**, pas des retards. Elles ne sont pas à proposer comme fonctionnalités manquantes.

| Exclu | Raison |
|---|---|
| Cloud | Contredit le principe Local First |
| Comptes utilisateur | Aucun service à authentifier |
| Abonnement | Contredit la propriété des données par le photographe |
| Intelligence artificielle | Hors périmètre ; une IA locale optionnelle reste envisageable à très long terme |
| HDR | Fonctionnalité entière, à trancher par un ADR propre le jour venu |
| Panorama | Idem |
| Reconnaissance faciale | Idem, avec une dimension vie privée qui exige sa propre décision |
| Synchronisation automatique | Contredit Local First |

Le raisonnement sur ce qui mérite d'être construit après la V1 — et sur ce qui a été délibérément coupé — est dans [`v2-scope.md`](v2-scope.md) et [`v2-implementation-plan.md`](v2-implementation-plan.md).
