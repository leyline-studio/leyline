# ADR 0022 — Bibliothèque par défaut au premier lancement

**Statut :** Accepté — 2026-07

## Contexte

`leyline-studio` exigeait un argument obligatoire : le chemin d'une bibliothèque déjà existante (`leyline-studio <library-dir>`). Absent, `run()` renvoyait `Err("usage: leyline-studio <library-dir>")`, affiché par `eprintln!` puis `ExitCode::FAILURE`.

Cette erreur n'a jamais de spectateur : lancé depuis un raccourci graphique — l'entrée du menu Démarrer posée par l'installeur NSIS Windows, l'AppImage Linux, un double-clic sur le `.app` macOS — aucun de ces chemins n'attache de console. L'échec ressemble donc, du point de vue de l'utilisateur, à un crash instantané et silencieux : la fenêtre ne s'ouvre jamais et rien n'explique pourquoi. Constaté en conditions réelles ce soir : l'utilisateur a installé l'installeur Windows fraîchement construit et l'a lancé depuis l'Explorateur — exactement ce chemin.

Le bug n'est pas spécifique à Windows : les trois plateformes partagent le même `run()`, donc le même comportement.

## Décision

Quand `leyline-studio` est lancé **sans argument**, il ne renvoie plus d'erreur : il ouvre — ou crée, à la première utilisation — une bibliothèque à un emplacement par défaut, résolu via la crate `directories` (déjà un choix conventionnel, bien maintenue, dépendance triviale — même famille que `sys-locale` déjà dans le workspace) :

* `<Documents de l'utilisateur>/Leyline Library` sur les trois plateformes (`UserDirs::document_dir()`) ;
* repli sur `<home>/Leyline Library` (`UserDirs::home_dir()`) si le système — un conteneur minimal, par exemple — n'expose pas de dossier Documents.

Le dossier est créé s'il n'existe pas (`std::fs::create_dir_all`), puis :

* si un `catalog.db` y existe déjà (relances suivantes), `Library::open` — comportement inchangé ;
* sinon (premier lancement), `Library::create` avec le nom `"Leyline Library"`.

**Un argument explicite garde le comportement historique à l'identique** : `Library::open` seul, sans repli ni création automatique. Un chemin fautif ou inexistant continue donc d'échouer franchement — aucun changement pour les scripts, le harnais de tests CLI, ou un utilisateur qui passe déjà un chemin sur sa propre bibliothèque.

L'emplacement résolu est affiché dans **Aide ▸ À propos de Leyline** (le dialogue `about` ajouté par ADR 0020), sous la ligne de description existante — la seule addition faite à ce dialogue. Pas de nouveau réglage, pas de fenêtre dédiée : l'utilisateur qui se demande où sont passées ses photos peut ouvrir ce dialogue et voir le chemin exact, sans qu'une préférence supplémentaire n'ait à être conçue pour ça.

## Conséquences

* Premier lancement sans argument : plus jamais silencieux — soit la fenêtre s'ouvre sur une bibliothèque vide fraîchement créée, soit (relances suivantes) sur celle déjà créée au même endroit.
* `default_library_root` est une fonction pure prenant les dossiers candidats en paramètres (pas d'appel direct à `directories::UserDirs` à l'intérieur) : testable sans toucher au vrai dossier personnel du poste qui fait tourner les tests. `default_library_dir` est le mince appel qui la connecte aux vrais répertoires utilisateur.
* Nouvelle dépendance : `directories = "6"` (workspace), utilisée uniquement par `leyline-studio`.

## Alternatives écartées

* **Dialogue de sélection de dossier au premier lancement** (un `FolderDialog` natif demandant où créer la bibliothèque) : plus proche de ce qu'un installeur d'application photo propose habituellement, mais plus de portée que ce correctif du soir ne justifie — nouvelle UI, nouvel état de premier lancement à concevoir et traduire (ADR 0019). Un défaut silencieux mais découvrable (via À propos) referme le bug immédiat sans engager cette conception ; un vrai choix de dossier au premier lancement reste une amélioration future, digne de son propre ADR si elle est décidée.
* **Dossier de données applicatif (`ProjectDirs::data_dir`, ex. `%APPDATA%`/`~/.local/share`) plutôt que Documents** : techniquement plus proche des conventions « données d'app », mais une bibliothèque de photos est un contenu que l'utilisateur possède et voudrait retrouver, sauvegarder ou déplacer lui-même — Documents correspond mieux à cet usage que le dossier de données caché d'une application.
