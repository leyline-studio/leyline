# ADR 0038 — Capture tethering : import direct depuis l'appareil via USB (libgphoto2)

**Statut :** Accepté — 2026-07

## Contexte

Un photographe qui shoote en studio (ou tout contexte où l'appareil reste
relié à un ordinateur) veut voir chaque photo apparaître dans le logiciel
dès le déclenchement, sans retirer la carte mémoire — c'est le tethering,
la fonctionnalité « Tethered Capture » de Lightroom. Ce n'était couvert ni
par `docs/specification.md` §Inclus (qui ne prévoit que l'import d'un
dossier existant), ni par une exclusion volontaire : le sujet n'avait
simplement jamais été tranché.

Deux familles de solutions existent :

1. **SDK propriétaires par constructeur** (Canon EDSDK, Nikon SDK, Sony
   Imaging Edge SDK...) — c'est l'approche de Lightroom : un module par
   marque, chacun sous licence fermée, redistribution soumise à accord
   constructeur.
2. **libgphoto2** — bibliothèque C libre (LGPL) qui parle PTP (et les
   extensions propriétaires par-dessus PTP pour la plupart des marques),
   couvre plusieurs centaines de boîtiers Canon/Nikon/Sony/Fujifilm/Olympus
   etc. C'est déjà l'outil qu'utilisent les logiciels tethering libres
   (entangle, digiKam) et Linux la propose en paquet système standard.

## Décision

Leyline implémente le tethering via **libgphoto2**, jamais via un SDK
constructeur — cohérent avec le choix déjà fait pour LibRaw (ADR 0004),
Lensfun et LittleCMS (ADR 0005) : des bibliothèques C libres, pas de
dépendance à un accord de licence par marque d'appareil.

Nouveau crate **`leyline-tether`**, qui enveloppe le crate Rust `gphoto2`
(bindings sûrs par-dessus libgphoto2) et n'expose que :

* `TetherSession::connect(staging_dir, on_event)` — auto-détecte la
  première caméra USB trouvée, démarre un thread dédié qui interroge la
  caméra (`Camera::wait_event`, scrutation à 500 ms) et télécharge chaque
  fichier signalé (`CameraEvent::NewFile`) dans `staging_dir` ;
* `TetherSession::stop()` (et `Drop`) — arrête la scrutation et relâche la
  caméra ;
* `TetherEvent::{Captured, Disconnected}` — notifications remontées par
  `on_event`, appelé sur le thread de la session.

Ce crate ne touche jamais le catalogue : c'est au moteur d'importer chaque
fichier reçu. `Library::tether_connect()` (`leyline-engine`) démarre une
session et, pour chaque `TetherEvent::Captured`, appelle le cœur d'import
existant (`Library::import`, `copy_files: true`) — **une capture tethering
est un import comme un autre**, pas un chemin de données séparé : même
checksum BLAKE3, même vignette générée, même déclenchement de
`Event::AssetsAdded` (`docs/engine-api.md` §3.2). Aucun nouvel événement
« capture » n'existe : `Event::AssetsAdded` suffit, le client re-requête
comme pour tout import (principe déjà posé par ADR 0011 — les événements
sont des notifications, jamais des données).

Deux événements nouveaux, seulement pour le cycle de vie de la connexion,
que `AssetsAdded` ne peut pas porter :

```rust
pub enum Event {
    // ...
    TetherConnected,
    TetherDisconnected { reason: Option<String> },
}
```

`docs/engine-api.md` §3.2 est complété en conséquence. Une seule session
par `Library` (une seule caméra à la fois) : `tether_connect` refuse une
deuxième connexion tant qu'une session est ouverte — le multi-caméra
simultané reste hors périmètre, à revisiter si le besoin se présente.

Rien ne change au pipeline de rendu : le tethering ne touche que l'import,
aucune version de process n'est concernée.

## Conséquences

* Nouvelle dépendance système : `libgphoto2` (+ ses en-têtes de
  développement à la compilation) — même famille de contrainte que LibRaw,
  Lensfun, LittleCMS déjà packagées par l'installeur (`docs/adr/0019`).
  Windows/macOS devront embarquer ou lier `libgphoto2` comme ces
  bibliothèques ; ce travail de packaging par plateforme reste ouvert (le
  crate et le moteur sont prêts, seule la distribution binaire par OS
  reste à faire — même statut que le reste de Phase 8).
* `docs/specification.md` §Inclus gagne « Capture tethering (USB,
  libgphoto2) ».
* `leyline-cli` gagne `leyline tether <library>` : ouvre une session,
  affiche chaque asset importé, s'arrête proprement à la déconnexion ou à
  un Ctrl+C — la même API que Studio utilisera pour son panneau tethering
  (§13, ADR 0011 : CLI et Studio consomment la même surface SDK).
* Interface Studio : File ▸ Tethered Capture… (`T`) ouvre un panneau modal
  qui appelle `tether_connect`/`tether_disconnect`, affiche l'état de la
  connexion, le compte de prises de la session et le nom du dernier fichier
  reçu — juste un client de plus sur l'API ci-dessus, aucune décision
  d'architecture supplémentaire n'a été nécessaire pour l'ajouter.
* Sans caméra USB branchée (CI, poste de développement courant),
  `TetherSession::connect` échoue proprement avec `TetherError::NoCamera`
  plutôt que de bloquer ou paniquer — c'est le seul comportement testable
  sans matériel, et les tests de `leyline-tether`/`leyline-engine` le
  vérifient explicitement.

## Alternatives écartées

* **SDK propriétaire par constructeur (Canon EDSDK, etc.)** : redistribution
  et compilation par plateforme soumises à un accord constructeur distinct
  par marque, incompatible avec le modèle « un seul dépôt, licence GPL-3.0
  uniforme » (ADR 0009) — il aurait fallu un crate par marque, chacun avec
  ses propres contraintes de licence binaire.
* **Watch-folder générique (surveiller un dossier où un logiciel tiers ou
  la caméra elle-même dépose les fichiers)** : plus simple, zéro nouvelle
  dépendance, mais ne répond pas à la demande « comme Lightroom » — ajoute
  une latence (écriture sur disque avant détection) et dépend d'un outil
  tiers pour piloter réellement l'appareil. Reste une extension possible
  plus tard (ex. appareils non supportés par libgphoto2) mais n'est pas la
  voie principale retenue ici.
* **Modéliser la capture comme un `Job` (`JobId`/`JobFinished`)** : une
  session tethering n'a ni total ni fin déterminée à l'avance — elle dure
  tant que l'appareil reste branché. Le contrat `Job` (`docs/engine-api.md`
  §3.1) suppose une fin ; forcer ce cas dedans aurait signifié un
  `JobFinished` qui ne finit jamais, ou un total arbitraire. Un état de
  connexion (`TetherConnected`/`TetherDisconnected`) plus les `AssetsAdded`
  habituels décrit mieux ce qui se passe réellement.
