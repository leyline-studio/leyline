# ADR 0003 — SQLite comme unique base du catalogue

**Statut :** Accepté — 2026-07

## Contexte

Le catalogue doit gérer des centaines de milliers d'assets, fonctionner hors ligne, tenir dans un fichier sauvegardable par simple copie, et rester lisible dans des décennies.

## Décision

Le catalogue est une base SQLite unique par bibliothèque (`catalog.db`), en mode WAL, avec FTS5 pour le texte libre. Schéma complet : `docs/catalog.md`.

## Conséquences

* Aucun serveur, aucune installation ; sauvegarde = copie de dossier.
* Format de fichier parmi les plus pérennes de l'industrie (SQLite est un format d'archivage recommandé par la Library of Congress).
* Une seule instance en écriture par bibliothèque (verrou) ; lecteurs multiples via WAL.
* Recherche plein texte sans index externe (FTS5 intégré).

## Alternatives écartées

* **PostgreSQL** : serveur à administrer, contraire au Local First.
* **Fichiers sidecar seuls (modèle Darktable XMP)** : recherche et collections impraticables à grande échelle ; les XMP restent un export optionnel (`catalog.md` §29).
* **Bases embarquées clé-valeur (sled, RocksDB)** : pas de requêtes relationnelles, pérennité de format inférieure.
