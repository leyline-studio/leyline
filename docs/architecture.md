# Architecture

Ce document répond à : **comment le projet est découpé, et pourquoi ainsi**. Le *pourquoi* du projet lui-même est dans [`vision.md`](vision.md) ; ce qu'il fait, dans [`specification.md`](specification.md).

---

## Le principe directeur

**Le moteur ignore l'existence de l'interface graphique.**

Studio, la CLI et le SDK sont trois clients du même moteur, à égalité. Aucun n'a de passe-droit : ce que Studio sait faire, la CLI et un script Rust le savent faire aussi, parce que tous trois passent par la même surface. Une fonctionnalité qui n'existerait que dans Studio serait le signe d'une erreur de découpage.

Cette contrainte a un coût réel — il faut concevoir l'API avant l'écran — et une contrepartie : le moteur reste testable sans interface, remplaçable sans réécrire l'interface, et utilisable par des gens qui n'ouvriront jamais Studio.

---

## Les crates

Treize crates, chacun avec une responsabilité unique.

| Crate | Responsabilité |
|---|---|
| `leyline-core` | Types partagés, identifiants, erreurs, `Settings`. Ne dépend de rien. |
| `leyline-engine` | Orchestration : jobs, événements, rendu, sessions d'édition. Le cœur. |
| `leyline-raw` | Décodage des fichiers RAW (et JPEG/PNG/TIFF à l'import). |
| `leyline-catalog` | Catalogue SQLite : bibliothèques, assets, versions, révisions. |
| `leyline-preview` | Cache d'aperçus et de miniatures. |
| `leyline-color` | Gestion des couleurs (ICC), lecture des profils DCP. |
| `leyline-lens` | Corrections d'objectif : distorsion, vignettage, aberration chromatique. |
| `leyline-tether` | Capture tethering USB via libgphoto2. |
| `leyline-map` | Lecture de tuiles MBTiles hors-ligne pour la vue carte. |
| `leyline-export` | Encodage de sortie : JPEG, TIFF, PNG, WebP, AVIF, et impression PDF. |
| `leyline-sdk` | Surface publique stable du moteur. Contrat semver. |
| `leyline-cli` | Client en ligne de commande. |
| `leyline-studio` | Application de bureau (Slint). |

---

## Sens des dépendances

```
Studio  →  SDK  →  Engine  →  Core
```

`Catalog`, `RAW`, `Color`, `Lens`, `Tether`, `Map`, `Preview` et `Export` sont consommés par `Engine`.

**Aucune dépendance circulaire n'est admise.** La règle est vérifiable mécaniquement : `cargo tree` doit rester un arbre.

Deux conséquences qui reviennent souvent en revue :

* `leyline-core` ne dépend d'aucun autre crate du projet. Si un type a besoin d'y descendre, c'est qu'il est partagé ; s'il ne l'est pas, il n'y a pas sa place.
* `leyline-sdk` ne contient **que** des ré-exports. C'est délibéré : le SDK est le contrat semver, ce qui laisse `leyline-engine` libre d'évoluer à chaque version. Son seul mode de défaillance est le *trou* — un type que le moteur rend mais qu'un appelant externe ne peut pas nommer — d'où le test de surface qui l'accompagne.

---

## À l'intérieur de Studio

Studio consomme `leyline-sdk` et rien d'autre : `crates/leyline-studio/Cargo.toml` ne déclare qu'une seule dépendance Leyline, et tout `src/` ne référence que `leyline_sdk`. C'est la même position qu'un produit tiers qui intégrerait le SDK. Un besoin auquel Studio répondrait par un détour vers un crate interne est le signe qu'il manque quelque chose à l'API publique : le correctif est d'élargir le SDK, jamais de contourner.

Sous cette contrainte, l'interface se découpe en quatre couches ([ADR 0045](adr/0045-studio-ui-modularisation.md)) :

```
ui/studio.slint     assemblage : attributs de fenêtre, raccourcis clavier, ordre de montage
ui/types.slint      structs du contrat Rust ↔ UI, et les gabarits de traduction (Tr)
ui/state/           un `global` Slint par domaine — la seule surface qui traverse vers Rust
ui/widgets/         contrôles réutilisables, sans aucune connaissance de l'état
ui/panels/          les vues (develop, carte, browser), l'overlay de dialogues, la barre de menus
ui/dialogs/         un fichier par dialogue modal
```

Côté Rust, `src/wiring/` est le miroir exact de `ui/state/` : un module par global, qui atteint le sien par `Global::<T>::get(&window)` et laisse les autres tranquilles. Autour, `app.rs` (l'état applicatif), `library.rs` (quelle bibliothèque est ouverte), `events.rs` (la pompe d'événements moteur) et `models.rs` (les conversions vers l'affichage) — plus les modules de logique pure `develop.rs`, `map_view.rs`, `format.rs` et `classify.rs`, qui ne connaissent aucun type Slint et sont les seuls testables unitairement.

La règle qui borne l'usage des globals est dans [`contributing.md`](contributing.md#létat-dinterface--global-ou-local-).

---

## Briques externes

Chaque dépendance lourde a fait l'objet d'une décision écrite.

| Brique | Rôle | Décision |
|---|---|---|
| **Rust** | Langage unique du projet | [ADR 0001](adr/0001-rust.md) |
| **Slint** | Interface graphique de Studio | [ADR 0002](adr/0002-slint.md) |
| **SQLite** | Base du catalogue | [ADR 0003](adr/0003-sqlite.md) |
| **LibRaw** | Décodage RAW (branche LGPL) | [ADR 0004](adr/0004-libraw.md) |
| **Lensfun** | Profils de correction d'objectif | [ADR 0005](adr/0005-lensfun-littlecms.md) |
| **LittleCMS** | Transformations ICC | [ADR 0005](adr/0005-lensfun-littlecms.md) |
| **libgphoto2** | Capture tethering USB | [ADR 0038](adr/0038-tethered-capture.md) |
| **Rayon** | Parallélisme de données du moteur | [ADR 0012](adr/0012-rayon-data-parallelism.md) |
| **BLAKE3** | Empreintes de fichiers | [ADR 0006](adr/0006-blake3.md) |
| **rfd** | Sélecteurs de dossier natifs dans Studio | — |

Les raisons de fond, résumées : **Rust** pour des performances proches du C++ avec la sécurité mémoire et une portabilité qui ne coûte rien ; **Slint** parce qu'il est multiplateforme, léger et conçu pour Rust ; **SQLite** parce qu'un catalogue doit être un simple fichier, sans serveur, robuste et rapide — et lisible par n'importe quel outil dans vingt ans.

---

## Stockage

Une bibliothèque Leyline est **autonome et déplaçable** : tous les chemins qu'elle stocke sont relatifs à sa racine ([ADR 0010](adr/0010-relative-paths.md)).

```
Bibliothèque/
 ├─ catalog.db        le catalogue SQLite
 ├─ Photos/           les fichiers importés en mode copie
 ├─ Profiles/         profils ICC et DCP fournis par l'utilisateur
 └─ cache/
     ├─ thumbs/       miniatures
     └─ preview/      aperçus développés
```

Le catalogue ne contient **jamais** les photos : uniquement les références, les métadonnées, les réglages, les collections et les index. Les aperçus vivent dans un cache dédié, reconstructible par définition — le supprimer ne perd rien. Les RAW ne sont relus que lorsque c'est nécessaire.

Le schéma complet est spécifié dans [`catalog.md`](catalog.md).

---

## Le pipeline de développement

Toutes les corrections sont appliquées sous forme d'un pipeline d'étapes indépendantes, du RAW décodé jusqu'à l'encodage de sortie. L'ordre des opérations n'est pas un détail d'implémentation : il fait partie du contrat de rendu, au même titre que les formules.

L'ordre exact, le format `settings_json`, les versions d'étages et la promesse de reproductibilité sont spécifiés dans [`pipeline.md`](pipeline.md) — à lire avant toute intervention sur le moteur.

Côté code, chaque opérateur vit dans son propre module versionné et gelé (`leyline-engine/src/stages/`, un dossier par opérateur, un fichier par version), et le registre `stages.rs` dit à quel rang chaque version s'insère ([ADR 0042](adr/0042-versioned-stage-pipeline.md), [ADR 0043](adr/0043-collapse-prerelease-render-history.md)).

---

## Conventions de développement

* `cargo fmt`, `cargo clippy` sans le moindre avertissement, tests verts : les trois avant chaque commit.
* Tests unitaires, tests d'intégration et benchmarks pour chaque fonctionnalité.
* API publiques documentées.
* Pas de code mort.

Le détail est dans [`contributing.md`](contributing.md).
