# Roadmap

État des phases de construction. Le détail de ce que contient chaque périmètre est dans [`specification.md`](specification.md).

**Les phases 0 à 8 sont terminées.** Le travail en cours ne consiste plus à combler des fonctionnalités manquantes du périmètre V1, mais à consolider ce qui existe.

---

## Phase 0 — Fondations documentaires ✅

Vision, architecture, conventions. Toute la documentation posée avant la première ligne de Rust, conformément à la devise du projet.

## Phase 1 — Workspace ✅

Workspace Cargo, découpage en crates, conventions de build et de test.

## Phase 2 — Lecture d'images ✅

Décodage RAW, JPEG, PNG et TIFF, et affichage.

## Phase 3 — Catalogue ✅

Catalogue SQLite et miniatures.

## Phase 4 — Pipeline ✅

Pipeline de développement non destructif, `settings_json`, process versions.

## Phase 5 — Leyline Studio ✅

Interface de bureau : navigateur, filtres, classement, module de développement, collections, dialogues, menus.

## Phase 6 — Export ✅

JPEG, TIFF, PNG, WebP, AVIF, presets d'export et export par lots.

## Phase 7 — Optimisations ✅

Parallélisme Rayon ([ADR 0012](adr/0012-rayon-data-parallelism.md)), benchmarks Criterion, rendu d'aperçu à la résolution d'affichage ([ADR 0041](adr/0041-interactive-preview-rendering.md)).

## Phase 8 — Distribution ✅

Installateur par plateforme et internationalisation FR/EN ([ADR 0019](adr/0019-distribution-i18n.md)).

---

## En cours

**Migration vers un pipeline d'étages versionnés** ([ADR 0042](adr/0042-versioned-stage-pipeline.md)) — remplacer les onze modules `processN.rs` dupliqués par des étages versionnés indépendamment, à rendu strictement identique. La migration procède en trois temps, dans l'ordre imposé par l'ADR :

1. ✅ Capturer des rendus de référence depuis le moteur actuel, et les committer.
2. ⬜ Refactoriser vers les étages composés.
3. ⬜ Prouver l'égalité bit à bit contre ces rendus, pour les onze versions.

**Épreuvage écran et filigrane** ([ADR 0034](adr/0034-softproofing-watermark-print.md)) — décidé, non implémenté. Dernier élément du cadrage post-V1 qui reste ouvert.

---

## Long terme

Plugins, SDK stable au sens semver, HDR, panorama, IA locale optionnelle.

Chacun de ces sujets est une fonctionnalité entière, qui devra faire l'objet de son propre ADR avant toute ligne de code.

---

## Documents liés

* [`v2-scope.md`](v2-scope.md) — cadrage architectural des fonctionnalités post-V1.
* [`v2-implementation-plan.md`](v2-implementation-plan.md) — séquencement recommandé : dépendances, effort, risque.
