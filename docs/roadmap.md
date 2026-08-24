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

Parallélisme Rayon ([ADR 0012](adr/0012-rayon-data-parallelism.md)), benchmarks Criterion, et [ADR 0041](adr/0041-interactive-preview-rendering.md) en entier : rendu d'aperçu à la résolution d'affichage, mise à l'échelle des rayons, et cache d'états intermédiaires du chemin preview (−78 % sur un curseur de fin de pipeline).

Le chemin **export**, qu'ADR 0041 laisse volontairement hors de ses optimisations, est mesuré depuis le 2026-08-03 (`benches/export.rs`) : décodage, rendu pleine résolution et encodage par format, tous linéaires en pixels. Les chiffres et les deux suites qu'ils désignent — vitesse d'encodage AVIF, recouvrement encodage/rendu dans un lot — sont dans [`competitive-plan.md`](competitive-plan.md) §B2.

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

Le cadrage post-V1 est refermé, et le plan de rattrapage concurrentiel aussi : [`competitive-plan.md`](competitive-plan.md), issu d'une comparaison à froid avec les logiciels établis, ne laisse **rien d'ouvert dans ce dépôt** — ses axes A (justesse du rendu) et B (performance) sont livrés ou écartés avec leurs raisons, et son axe C dépend d'un détecteur qui vit hors du dépôt ([ADR 0073](adr/0073-external-mask-detectors.md)).

Depuis, le travail porte sur ce qui manque à une **première publication** :

* le rendu suit le curseur pendant qu'on tire un réglage, et le proxy d'affichage se met en cache ([ADR 0074](adr/0074-live-preview-while-dragging.md), [ADR 0076](adr/0076-proxy-cache.md)) ; le cache d'aperçus tient dans une fenêtre bornée ([ADR 0075](adr/0075-preview-cache-retention.md)) ;
* une version installée sait qu'une plus récente existe, sans rien transmettre et sans jamais s'installer seule ([ADR 0077](adr/0077-application-updates.md)), et le catalogue est sauvegardé avant toute migration ;
* le panneau Préférences existe, avec la règle d'admission qui décide ce qui a le droit d'y entrer ([ADR 0078](adr/0078-preferences-panel.md)) ;
* un boîtier réglé en RAW+JPEG ne double plus la bibliothèque ([ADR 0079](adr/0079-raw-jpeg-pairing.md)).

Ce que [`readme.md`](readme.md) liste comme encore ouvert reste la référence : la colorimétrie DCP non validée contre un rendu Adobe, et deux finitions d'outillage des retouches locales laissées hors périmètre par [ADR 0049](adr/0049-local-adjustments-clients.md).

---

## Long terme

Plugins, SDK stable au sens semver, HDR, panorama, IA locale optionnelle.

Chacun de ces sujets est une fonctionnalité entière, qui devra faire l'objet de son propre ADR avant toute ligne de code.

---

## Documents liés

* [`v2-scope.md`](v2-scope.md) — cadrage architectural des fonctionnalités post-V1.
* [`v2-implementation-plan.md`](v2-implementation-plan.md) — séquencement recommandé : dépendances, effort, risque.
