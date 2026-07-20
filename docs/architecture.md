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
 │   ├─ leyline-export
 │   ├─ leyline-sdk
 │   ├─ leyline-cli
 │   └─ leyline-studio
```

## Dépendances

Studio → SDK → Engine → Core

Catalog, RAW, Color, Lens, Preview et Export sont consommés par Engine.

## Choix techniques

* Rust Edition courante
* Slint
* SQLite
* LibRaw
* Lensfun
* LittleCMS
* rfd — sélecteurs de dossier natifs (Explorer/GTK/Finder) dans Studio, pour les dialogues d'import et d'export
* Tests unitaires + intégration + benchmarks.

