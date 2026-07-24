# ADR 0039 — Import automatique par dossier surveillé (watched-folder)

**Statut :** Accepté — 2026-07

## Contexte

ADR 0038 (capture tethering) notait déjà le watch-folder générique comme
extension possible, écartée à l'époque au profit de libgphoto2 pour le cas
« appareil branché en USB ». Le besoin réapparaît pour un cas différent :
un dossier où un logiciel tiers, un lecteur de carte réseau ou tout
processus externe dépose des fichiers — pas forcément un appareil photo — et
que l'utilisateur veut voir importés automatiquement, sans relancer un
import manuel à chaque fois. C'est la fonctionnalité « Auto Import » de
Lightroom.

Le survol des écarts Studio/Lightroom-Darktable du 2026-07-24 (voir
[[studio-workflow-gaps-progress]]) l'avait identifié comme le dernier écart
« workflow » réel, volontairement reporté : nouvelle dépendance
(surveillance de système de fichiers), nouveau cycle de vie de thread
d'arrière-plan — plus lourd que les 7 autres écarts déjà livrés ce jour-là.

## Décision

Implémenté directement dans `leyline-engine` (pas un nouveau crate séparé,
contrairement à `leyline-tether`) : la dépendance ajoutée, `notify` (crate
Rust pur, portable Linux/macOS/Windows), n'enveloppe aucune bibliothèque C
système à faire packager par plateforme — elle ne justifie pas la même
séparation que LibRaw/Lensfun/LittleCMS/libgphoto2.

Nouveau module `watch.rs`, qui reprend délibérément la forme de
`leyline_tether::TetherSession` (même contrat « thread d'arrière-plan +
callback non-bloquant ») :

* `WatchSession::watch(folder, on_event)` — démarre un `notify::Watcher`
  récursif sur `folder` et un thread dédié qui débounce les événements bruts
  du système de fichiers en fichiers « stabilisés » : un événement create/
  modify (re)démarre le suivi d'un chemin, et à chaque tick (500 ms) tout
  chemin dont la taille n'a pas changé depuis `STABILITY_WINDOW` (2 s) est
  promu en `WatchSessionEvent::Ready`. Nécessaire parce qu'un dépôt de
  fichier réel (copie depuis une carte, écriture réseau) n'est pas atomique :
  sans ce debounce, un import démarrerait sur un fichier encore à moitié
  écrit.
* Seules les extensions reconnues par `leyline_engine::import::media_type`
  entrent dans le suivi — un sidecar XMP ou un fichier temporaire ne
  déclenche jamais d'événement.
* `WatchSession::stop()` (et `Drop`) — arrête la surveillance.

Comme pour le tethering, ce module ne touche jamais le catalogue :
`Library::watch_start(folder)` démarre une session et, pour chaque fichier
prêt, appelle le cœur d'import existant (`Library::import`,
`copy_files: true`) — **un import par dossier surveillé est un import comme
un autre**, même checksum BLAKE3, même vignette, même `Event::AssetsAdded`.
Deux événements nouveaux, seulement pour le cycle de vie de la session,
même schéma que `TetherConnected`/`TetherDisconnected` :

```rust
pub enum Event {
    // ...
    WatchStarted { folder: PathBuf },
    WatchStopped { reason: Option<String> },
}
```

Une seule session par `Library` (un seul dossier surveillé à la fois),
refusée si une session est déjà ouverte — même contrat que
`tether_connect`.

**Chaque fichier stabilisé est importé individuellement, jamais en lot.**
`Library::import` tient le mutex du catalogue pendant tout son appel (voir
sa propre doc) ; ce mutex est aussi sur le chemin de toute opération
interactive en mode développement (commit, note, aperçu — ADR 0023,
ADR 0024). Un dossier recevant beaucoup de fichiers d'un coup (import en
lot depuis une carte) importés un par un garde donc chaque fenêtre de
verrouillage courte — jamais plus longue qu'un seul fichier — au lieu de
bloquer le catalogue pour la durée du lot entier. `handle_watch_event`
reprend ici exactement la même construction que `handle_tether_event`.
Un benchmark dédié (`leyline-engine/benches/import.rs`, groupe `import`)
mesure ce coût par fichier — checksum + écriture catalogue + rendu de
vignette — pour repérer toute régression qui allongerait cette fenêtre.

## Un bug de deadlock découvert en écrivant les tests

`Library::watch_start`/`watch_stop` ont d'abord été écrits en reprenant
littéralement le code de `tether_connect`/`tether_disconnect`, y compris
`if let Some(session) = lock(&self.inner.watch).take() { session.stop(); }`.
Comme `leyline-tether` ne peut pas être exercé sans matériel USB réel, cette
ligne n'avait jamais tourné en dehors d'un banc de test physique. `notify`,
lui, se teste entièrement en local (pas de matériel requis) — et le tout
premier test bout-en-bout (`tests/watch.rs`) s'est bloqué indéfiniment sur
`watch_stop()`.

Cause : en Rust, le garde de mutex temporaire produit par `lock(...)` dans
le scrutinee d'un `if let Some(x) = EXPR { BODY }` vit pour **tout le
bloc**, pas seulement pour l'évaluation d'`EXPR` (extension de durée de vie
des temporaires). Le mutex `watch` restait donc verrouillé pendant tout
`session.stop()`, qui bloque en attendant que le thread d'arrière-plan se
termine — sauf que ce thread, en sortant de sa boucle, doit lui-même
verrouiller `watch` pour publier son propre `WatchSessionEvent::Stopped`
(`handle_watch_event`). Interblocage classique, thread principal contre
thread de session, sur le même mutex.

Correctif — dans `watch_stop` **et** dans `tether_disconnect`, qui portait
le même bug latent, jamais détecté faute de test capable de l'exercer :

```rust
// Avant (deadlock si un thread d'arrière-plan doit reprendre ce même
// mutex avant de se terminer) :
if let Some(session) = lock(&self.inner.watch).take() {
    session.stop();
}

// Après : le `take()` est sa propre instruction, le garde est donc
// relâché avant l'appel bloquant.
let session = lock(&self.inner.watch).take();
if let Some(session) = session {
    session.stop();
}
```

Les deux méthodes portent maintenant un commentaire expliquant pourquoi la
forme en une seule instruction est incorrecte, pas seulement un style à
préférer.

## Conséquences

* Nouvelle dépendance : `notify` (pure Rust, `default-features = false` +
  `macos_fsevent` — le seul défaut du crate). Aucun packaging par
  plateforme à prévoir, contrairement à `libgphoto2` (ADR 0038).
* `docs/specification.md` §Inclus gagne « Import automatique par dossier
  surveillé ».
* `leyline-cli` gagne `leyline watch <library> <folder>` — même schéma que
  `leyline tether`.
* Interface Studio : File ▸ Auto Import… ouvre un panneau modal
  (sélecteur de dossier, Start/Stop, compteur de fichiers importés cette
  session) — client de plus sur l'API ci-dessus.
* `tether_disconnect` a été corrigé du même bug de deadlock que
  `watch_stop`, alors que le sujet de ce ticket était l'auto-import : un
  correctif motivé par le test réel, pas une extension de portée
  volontaire.

## Alternatives écartées

* **Un nouveau crate `leyline-watch`** (comme `leyline-tether`) : rejeté —
  `notify` est du Rust pur, sans bibliothèque C système à isoler ; la
  séparation en crate de `leyline-tether` sert précisément à isoler la
  liaison FFI avec libgphoto2, absente ici.
* **Import du dossier entier en un seul lot par cycle de scrutation** :
  plus simple, mais tiendrait le mutex du catalogue pour la durée du lot
  entier à chaque vague de fichiers — voir la section dédiée ci-dessus.
* **Suivi par `mtime` seul plutôt que par taille stable** : plus simple,
  mais un `mtime` ne change pas forcément à chaque écriture selon le
  système de fichiers/l'outil de copie utilisé, alors qu'une taille stable
  sur deux scrutations consécutives est une garantie directe qu'aucune
  écriture n'est en cours.
