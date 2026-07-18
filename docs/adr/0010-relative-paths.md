# ADR 0010 — Bibliothèque autonome, chemins relatifs

**Statut :** Accepté — 2026-07

## Contexte

Une bibliothèque doit survivre à un déménagement de disque, un changement de machine ou d'OS (`C:\...` vs `/home/...`), et se sauvegarder par simple copie.

## Décision

Une bibliothèque est un dossier autonome (`catalog.db`, `Photos/`, `Cache/`, `Exports/`, `Backups/`). Le catalogue ne stocke **aucun chemin absolu** : toutes les références sont relatives à la racine, séparateur `/`.

Le chemin d'un asset est toujours dérivé (`folders.relative_path` + `filename`) — jamais stocké en double.

## Conséquences

* Portabilité totale Windows/Linux/macOS ; sauvegarde et restauration = copie de dossier.
* Déplacer la bibliothèque ne casse rien ; renommer un dossier ne désynchronise rien.
* Exception unique et documentée : les destinations d'export (`export_history.destination`) pointent hors bibliothèque.

## Alternatives écartées

* **Chemins absolus (Lightroom historique)** : source classique de catalogues cassés après migration.
* **Photos hors du dossier bibliothèque par défaut** : possible techniquement (`copy_files: false` à l'import), mais l'autonomie du dossier reste le cas nominal.
