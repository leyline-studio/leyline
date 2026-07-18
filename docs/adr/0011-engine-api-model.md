# ADR 0011 — API moteur : bibliothèque Rust, requêtes synchrones, travaux asynchrones

**Statut :** Accepté — 2026-07

## Contexte

Le moteur sert plusieurs clients (Studio, CLI, scripts). Il faut une frontière stable, réactive pour l'UI, sans imposer d'infrastructure aux clients légers.

## Décision

* L'API est une **bibliothèque Rust** (`leyline-sdk`), pas un serveur ni un protocole.
* **Requêtes catalogue synchrones** (SQLite répond en microsecondes) ; **travaux lourds asynchrones** (`JobId` + flux d'événements).
* Aucun runtime async imposé : threads natifs + canaux standard.
* Les événements sont des notifications, jamais des données : le client re-requête.
* L'édition passe par `EditSession`, qui implémente la coalescence des révisions.

Détails : `docs/engine-api.md`.

## Conséquences

* Studio, CLI et scripts appellent strictement la même API — « API avant interface graphique » est structurel, pas déclaratif.
* Pas de dépendance tokio dans le SDK ; intégration Slint par simple canal.
* Une passerelle C FFI reste possible (signatures sans génériques ni lifetimes exposés).
* Le schéma SQLite n'est **pas** une API publique : les clients passent par le SDK.

## Alternatives écartées

* **Serveur local (gRPC/HTTP)** : sérialisation et latence injustifiées pour du in-process ; possible plus tard par-dessus le SDK.
* **API entièrement async (tokio)** : impose un runtime à tous les clients, y compris une CLI de trois lignes.
* **Accès SQLite direct par les clients** : couplage au schéma, fin de la liberté de migration.
