# Spécification V1

## Inclus

* Import d'un dossier (RAW, DNG, JPEG, PNG, TIFF)
* Catalogue SQLite
* Miniatures
* Lecture EXIF
* Développement RAW non destructif
* Exposition
* Balance des blancs
* Contraste
* Ombres / Hautes lumières
* Blancs / Noirs
* Vibrance / Saturation
* Rotation
* Recadrage
* Correction d'objectif (Lensfun)
* Gestion des couleurs (LittleCMS)
* Presets de développement (créer, appliquer, en lot) — voir `docs/presets.md`
* Capture tethering (USB, libgphoto2) : import automatique de chaque photo dès la prise de vue — voir `docs/adr/0038-tethered-capture.md`
* Import automatique par dossier surveillé (watched-folder) — voir `docs/adr/0039-watched-folder-import.md`
* Export JPEG/TIFF/WebP/AVIF
* Installateur par plateforme (Windows/macOS/Linux), dossier d'installation au choix quand la plateforme le permet — voir `docs/adr/0019-distribution-i18n.md`
* Interface multilingue (français, anglais au lancement), extensible sans changement de code — voir `docs/adr/0019-distribution-i18n.md`

## Exclus volontairement

* Cloud
* Comptes
* IA
* HDR
* Panorama
* Reconnaissance faciale
* Synchronisation

