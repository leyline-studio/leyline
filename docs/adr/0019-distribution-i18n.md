# ADR 0019 — Distribution : installateur par plateforme et internationalisation FR/EN

**Statut :** Accepté — 2026-07

## Contexte

Leyline Studio ne se lance aujourd'hui que par `cargo run -p leyline-studio <dossier>` : aucun mécanisme d'installation, aucune traduction, l'interface est en anglais codé en dur. Une V1 destinée à des utilisateurs finaux (pas seulement des développeurs) doit pouvoir s'installer sans passer par Cargo, et l'équipe veut au moins le français et l'anglais dès le lancement, avec la possibilité d'ajouter d'autres langues sans toucher au code.

## Décision

### Installateur par plateforme

Génération via `cargo-packager`, piloté depuis un nouveau répertoire `packaging/` à la racine (fichiers de configuration par OS + scripts, **pas un nouveau crate Rust** — même séparation code/outillage que le reste du workspace) :

* **Windows** : installeur NSIS (`.exe`), assistant classique avec une page « choisir le dossier d'installation » — répond directement au besoin exprimé.
* **macOS** : bundle `.app` livré en `.dmg`, glisser-déposer vers `/Applications` — convention native macOS ; pas de choix de dossier ici, ce n'est pas l'usage sur cette plateforme et un installeur « chemin personnalisé » y paraîtrait suspect plutôt que pratique.
* **Linux** : AppImage portable en premier livrable — l'utilisateur choisit lui-même où le placer, l'équivalent le plus proche d'un « dossier d'installation » sans dépendre d'un gestionnaire de paquets. Un `.deb` pourra s'ajouter ensuite si la demande existe, mais n'a pas la même flexibilité de dossier (chemins FHS imposés).

### Internationalisation (FR/EN, extensible)

Slint a un support natif de traduction : les chaînes de l'UI passent par `@tr(...)` dans les fichiers `.slint` (et `slint::tr!()` côté Rust), extraites vers des fichiers `.po` par `slint-tr-extractor`, puis chargées au runtime. Décision :

* Toutes les chaînes visibles de Leyline Studio passent par `@tr(...)` au lieu d'être codées en dur (elles le sont actuellement, cf. `docs/roadmap.md` phase 5 déjà livrée — reste un travail d'extraction à faire, voir Conséquences).
* Deux langues au lancement : l'anglais existant devient la locale de référence (source des extractions), le français est la première traduction ajoutée.
* Détection de la langue système au démarrage, repli sur l'anglais si aucune traduction disponible pour cette langue ; un réglage explicite pourra la surcharger plus tard (hors scope immédiat).
* Ajouter une langue = ajouter un fichier `.po` traduit, sans toucher au code Rust ni aux fichiers `.slint`.
* La CLI (`leyline-cli`) reste en anglais uniquement pour la V1 : un outil scriptable n'a pas le même besoin de traduction qu'une UI graphique, et traduire ses messages casserait le parsing pour tout script qui les inspecterait (`docs/engine-api.md` §1 — la CLI est un client fin, ses messages ne font pas partie du contrat API).

## Conséquences

* Nouveau dossier `packaging/` (icônes, scripts de build par OS, pas de code applicatif) — même logique que `docs/adr/0004-libraw.md` : l'outillage de build reste séparé du code métier. Correction après implémentation : la table `[package.metadata.packager]` elle-même vit dans `crates/leyline-studio/Cargo.toml`, pas dans un fichier autonome sous `packaging/` — `cargo-packager` ne fait le rapprochement automatique avec les métadonnées du crate (binaires, version, out-dir) que lorsqu'il lit cette table depuis un `Cargo.toml` de workspace, pas via un fichier passé en `-c`. Seuls les chemins vers les assets (icônes, `.ico`) et les scripts de build par OS restent sous `packaging/`.
* Un inventaire des chaînes UI de Leyline Studio à faire passer par `@tr(...)` est un prérequis avant que la traduction soit réellement effective — c'est un chantier propre (Phase 8, voir `docs/roadmap.md`), pas fait d'un coup avec cet ADR.
* Produire les trois installateurs demande de compiler sur (ou de cross-compiler pour) Windows/macOS/Linux ; les vérifications `fmt`/`clippy`/`test` restent locales comme aujourd'hui (`docs/no-github-ci-yet` reste la décision en vigueur — cet ADR n'y touche pas).

## Alternatives écartées

* **cargo-dist** : plus orienté binaires CLI livrés par GitHub Releases (scripts shell/PowerShell, formules Homebrew) que véritables installeurs graphiques avec assistant et dossier au choix — moins adapté à une application desktop grand public comme Leyline Studio.
* **gettext appelé directement, en dehors de Slint** : réinventerait ce que `slint-tr-extractor`/`@tr(...)` fait déjà nativement pour du code UI Slint, sans bénéfice.
* **Traduire aussi la CLI dès la V1** : reporté — voir le dernier point de la décision.
