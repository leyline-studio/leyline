# Architecture

## Workspace

```
leyline/
 ├─ crates/
 │   ├─ leyline-core
 │   ├─ leyline-engine
 │   ├─ leyline-raw
 │   ├─ leyline-catalog
 │   ├─ leyline-preview
 │   ├─ leyline-color
 │   ├─ leyline-lens
 │   ├─ leyline-tether
 │   ├─ leyline-export
 │   ├─ leyline-sdk
 │   ├─ leyline-cli
 │   └─ leyline-studio
```

## Dépendances

Studio → SDK → Engine → Core

Catalog, RAW, Color, Lens, Tether, Preview et Export sont consommés par Engine.

## Choix techniques

* Rust Edition courante
* Slint
* SQLite
* LibRaw
* Lensfun
* LittleCMS
* libgphoto2 — capture tethering (`docs/adr/0038-tethered-capture.md`)
* rfd — sélecteurs de dossier natifs (Explorer/GTK/Finder) dans Studio, pour les dialogues d'import et d'export
* Tests unitaires + intégration + benchmarks.

