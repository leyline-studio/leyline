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

Pipeline de développement non destructif, `settings_json`, versions de rendu.

## Phase 5 — Leyline Studio ✅

Interface de bureau : navigateur, filtres, classement, module de développement, collections, dialogues, menus.

## Phase 6 — Export ✅

JPEG, TIFF, PNG, WebP, AVIF, presets d'export et export par lots.

## Phase 7 — Optimisations ✅

Parallélisme Rayon ([ADR 0012](adr/0012-rayon-data-parallelism.md)), benchmarks Criterion, rendu d'aperçu à la résolution d'affichage ([ADR 0041](adr/0041-interactive-preview-rendering.md)).

## Phase 8 — Distribution ✅

Installateur par plateforme et internationalisation FR/EN ([ADR 0019](adr/0019-distribution-i18n.md)).

## Pipeline d'étages versionnés ✅

Les onze modules `processN.rs` dupliqués (14 968 lignes, 70 à 93 % de duplication) sont remplacés par des étages versionnés indépendamment, puis l'historique de rendu antérieur à la publication est effondré. Dans l'ordre imposé par les ADR :

1. ✅ Capturer des rendus de référence depuis le moteur d'alors, et les committer.
2. ✅ Refactoriser vers les étages composés ([ADR 0042](adr/0042-versioned-stage-pipeline.md)) — ~1 800 lignes d'étages versionnés (`crate::stages`), un registre, et une table d'expansion `process: N` → versions d'étages.
3. ✅ Prouver l'égalité bit à bit contre ces rendus, pour les onze versions — les 77 cas de `golden_renders.rs` sont passés sans re-bénissage.
4. ✅ Effondrer l'historique ([ADR 0043](adr/0043-collapse-prerelease-render-history.md)) — le projet n'ayant jamais été publié, ces onze versions n'engageaient personne : une seule version par opérateur (`v1`), la table d'expansion supprimée, et le §2 d'ADR 0042 livré dans le même mouvement — la révision enregistre sa propre carte `stages`, le champ `process` disparaît. Les catalogues de développement antérieurs ne sont pas migrés (ADR 0043 §5).

---

## En cours

Le cadrage post-V1 est refermé : l'épreuvage écran et le filigrane texte, derniers éléments restés ouverts, sont livrés ([ADR 0051](adr/0051-watermark-rasterization-and-soft-proof-surface.md)). Le travail porte désormais sur la prise en main et le confort d'usage de Studio — ADR 0054 à 0058 — et sur ce que [`readme.md`](readme.md) liste comme encore ouvert : la colorimétrie DCP à valider, les finitions d'outillage des retouches locales, le profil de bruit par boîtier.

---

## Long terme

Plugins, SDK stable au sens semver, HDR, panorama, IA locale optionnelle.

Chacun de ces sujets est une fonctionnalité entière, qui devra faire l'objet de son propre ADR avant toute ligne de code.

---

## Documents liés

* [`v2-scope.md`](v2-scope.md) — cadrage architectural des fonctionnalités post-V1.
* [`v2-implementation-plan.md`](v2-implementation-plan.md) — séquencement recommandé : dépendances, effort, risque.
