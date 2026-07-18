# ADR 0002 — Slint pour l'interface graphique

**Statut :** Accepté — 2026-07

## Contexte

Leyline Studio doit être multiplateforme, léger, fluide sur des grilles de centaines de milliers d'éléments, et s'intégrer naturellement à un moteur Rust.

## Décision

L'interface de Leyline Studio est construite avec Slint.

## Conséquences

* Intégration Rust native, pas de pont JavaScript ni de runtime embarqué.
* Rendu GPU, empreinte mémoire faible.
* Licence : branche GPL-3.0 pour la version community ; licence royalty-free Slint pour une future version desktop propriétaire (voir ADR 0009).
* Écosystème plus jeune que Qt : certains widgets seront à construire.

## Alternatives écartées

* **Qt** : mature mais C++ et licence commerciale coûteuse ; bindings Rust de second ordre.
* **Tauri / Electron** : runtime web, empreinte mémoire et latence incompatibles avec l'objectif « Fast ».
* **egui** : excellent pour l'outillage, mode immédiat inadapté à une UI riche persistante.
* **GTK** : intégration macOS/Windows médiocre.
