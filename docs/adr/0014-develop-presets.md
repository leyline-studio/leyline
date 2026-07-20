# ADR 0014 — Presets de développement : catégories partielles, application par révision normale

**Statut :** Accepté — 2026-07

## Contexte

La vision (`readme.md`) et la spécification V1 n'avaient jamais couvert les presets de développement (jeux de réglages nommés, réutilisables, applicables à une photo ou en lot) — seuls les presets d'export existaient (`catalog.md` §27). C'est pourtant une attente de base d'un outil de développement RAW non destructif, et son absence a été identifiée comme bloquante pour une v1 fidèle à la vision. `docs/presets.md` fixe le contrat produit ; cet ADR consigne les choix structurants qui en découlent.

## Décision

**Presets partiels par catégorie, pas des snapshots complets.** Un preset ne capture que les catégories de réglages explicitement incluses (`groups`, `presets.md` §3.1–3.2), à la granularité d'une case à cocher façon Lightroom (`white_balance`, `tone`, `presence`, `lens_correction`, `detail`, `geometry`) — jamais au champ individuel. `geometry` (rotation, crop) n'est jamais incluse par défaut.

**Un seul espace de schéma.** `preset_json.schema` référence directement la numérotation de `settings_json.schema` (`pipeline.md` §3.2) — un preset consomme le même vocabulaire de champs qu'une révision, pas un format indépendant à faire évoluer en parallèle. Aucun champ `process` : un preset ne fixe jamais de rendu, seulement des valeurs.

**Application = révision normale.** Appliquer un preset à une version n'introduit aucun nouveau mécanisme d'écriture : c'est un `EditSession` classique (`engine-api.md` §10.1) qui fusionne les champs des catégories incluses sur l'état courant puis commite — toujours une nouvelle révision, jamais un amendement. Aucune notion de « révision issue d'un preset » n'existe dans `develop_revisions` : pas de colonne, pas de FK vers `develop_presets`.

**Pas de transaction de lot.** L'application à un ensemble de versions traite chaque version indépendamment (une révision chacune), avec rapport de succès/échec par version, sur le modèle de `export_batch` et de l'import (`engine-api.md` §3.2, §12). Il n'existe pas d'unité « tout ou rien » couvrant le lot entier.

**Table catalogue dédiée.** `develop_presets` (`presets.md` §4) est distincte d'`export_presets`, malgré une forme SQL proche : deux domaines indépendants (réglages de développement vs. encodage de sortie).

## Conséquences

* Aucune règle nouvelle de non-destructivité ou de reproductibilité : un preset appliqué produit une révision comme n'importe quelle autre, undo/redo et historique complet fonctionnent sans code spécifique (`pipeline.md` §2, §6).
* Le cadrage et la rotation d'une photo ne sont jamais altérés par erreur en appliquant un style en série — condition nécessaire pour qu'un preset soit sûr à appliquer à toute une bibliothèque.
* Renommer ou supprimer un preset n'a aucun effet rétroactif : les révisions qu'il a produites restent des états de réglages ordinaires, sans lien traçable vers leur origine (accepté : `catalog.md` §17 définit déjà une révision comme « une intention utilisateur, jamais un événement d'interface »).
* Un lot partiellement en échec (ex. un preset référençant un schéma que le moteur ne connaît plus) laisse les versions déjà traitées commitées : cohérent avec le modèle « la version est l'unité de bibliothèque » (ADR 0008), pas une régression introduite ici.
* `preset_json` est sérialisable seul (aucune référence à une bibliothèque) : le partage volontaire de presets évoqué dans `readme.md` reste une extension d'UI, pas un changement de format, le jour où on le construit.

## Alternatives écartées

* **Snapshot complet du `settings_json`** : le rejeu écraserait systématiquement cadrage, rotation et correction d'objectif de la photo cible — inacceptable pour une application en lot, contredit l'esprit non destructif « jugement par photo » déjà acté pour le classement (ADR 0008).
* **Granularité par champ plutôt que par catégorie** : aucune demande produit ne va plus loin que le découpage à la Lightroom ; cela ajoutait de la surface de test et de configuration (matrices de champs cochables un par un) sans bénéfice identifié pour la V1.
* **`develop_revisions.preset_id` (FK vers l'origine)** : romprait l'invariant qu'une révision est un état anonyme, forcerait tout consommateur du graphe de révisions (undo, export, reprocessing) à un cas particulier « révision issue d'un preset » sans qu'aucun besoin produit ne le justifie ; `author` existe déjà si la provenance devient nécessaire un jour (§17).
* **Transaction unique sur tout le lot (all-or-nothing)** : demanderait un rollback distribué sur des graphes de révisions indépendants par asset, incohérent avec le comportement déjà spécifié d'`export_batch` et de l'import, qui rapportent des échecs par élément sans annuler le reste.
* **Réutiliser `export_presets`** : structure SQL quasi identique, mais domaines sans rapport (réglages de rendu vs. paramètres d'encodage) ; les fusionner aurait imposé des colonnes optionnelles ou une discrimination par type dans une table déjà simple.
