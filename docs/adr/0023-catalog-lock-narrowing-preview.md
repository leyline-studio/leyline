# ADR 0023 — Ne pas tenir le verrou catalogue pendant un rendu de preview

**Statut :** Accepté — 2026-07

## Contexte

`Library::preview()` tient le `Mutex<Catalog>` unique — celui que traverse
toute lecture (`catalog()`) et toute écriture (`catalog_mut()`, `edit()`) —
pendant toute sa durée, y compris le rendu complet (décodage RAW + pipeline
`process1`–`5` + encodage PNG), qui ne touche jamais le catalogue. Un rendu
coûte de quelques dizaines à ~100 ms (bancs `process1.rs`) ; le pool de rendu
borné admet jusqu'à 16 jobs concurrents. Chaque rendu — et chaque job
`preview_async` du pool — bloque donc toute la navigation, la recherche et
l'édition de métadonnées de Studio le temps du rendu, alors que le rendu
lui-même n'a besoin d'aucun accès au catalogue.

`rusqlite::Connection` est `Send` mais `!Sync` : `Catalog` encapsule une
connexion unique, donc un `RwLock<Catalog>` ne compilerait pas pour des
lectures concurrentes — ce n'est pas seulement la cohérence logique qui est
protégée par le mutex, c'est l'unique connexion elle-même. Un pool de
connexions (lecteurs multiples + un écrivain, permis par WAL) résoudrait la
contention plus largement, mais c'est un changement invasif à toute
l'API `leyline-catalog` (emprunts, transactions, ouverture/migration) pour un
gain qui dépasse le bug ciblé ici — écarté de cette décision, à revisiter
séparément si la contention lecture-vs-écriture s'avère un jour mesurable.

Réduire simplement la fenêtre du verrou (le prendre pour la lecture des
réglages, le relâcher pendant le rendu, le reprendre pour l'écriture de la
preview) semblait à première vue sans risque : entre les deux prises, la
révision de tête pourrait avancer (`commit_revision`) ou bouger (`undo`/
`redo`), mais ces opérations créent une nouvelle révision ou déplacent la
tête vers une révision immuable existante — la preview qu'on enregistre
reste une preview *correcte* de la révision R qu'on a rendue, simplement non
tête, donc ignorée par `valid_preview` tant qu'on ne revient pas dessus.
Rien d'incorrect.

Il existe cependant une exception : `try_amend_head` (§17, fenêtre
d'amendement de 2 s) **réécrit `settings_json` d'une révision en place, en
conservant le même identifiant R**, et supprime les previews de R (règle déjà
documentée, `catalog.md` §17). Sans le verrou tenu de bout en bout, la
séquence suivante devient possible :

1. Le thread de rendu lit tête = R, réglages = S1.
2. Il rend S1 sans verrou.
3. Un amendement concurrent réécrit R : R signifie maintenant S2, les
   previews de R sont supprimées.
4. Le thread de rendu enregistre sa preview pour R avec les pixels S1.
5. `valid_preview` pour tête = R trouve cette ligne et la sert comme preview
   valide de R — alors que les pixels sont S1 et que R signifie S2 : preview
   périmée servie comme fraîche, sans aucun signal de fraîcheur pour le
   détecter.

Réduire la fenêtre sans rien d'autre réintroduit donc une vraie corruption
silencieuse, pas seulement un gaspillage de rendu.

## Décision

`preview::preview` se scinde en trois phases séquencées par l'appelant, avec
le verrou catalogue tenu seulement pour les deux premières et la dernière —
jamais pendant le rendu :

1. **`plan_preview`** (lecture seule, verrou court) : sert le cache si une
   preview valide existe déjà, sinon lit tête, réglages, chemin source et
   métadonnées, et capture la chaîne `settings_json` brute de la révision.
2. **Décodage + rendu** : aucun verrou catalogue tenu ; le cache de
   décodages (`Mutex<DecodeCache>`) n'est tenu que le temps de
   `get_or_insert_with`, qui rend un `Arc<RawImage>` possédé.
3. **`record_render`** (écriture, verrou court) : enregistre la preview via
   une nouvelle méthode catalogue, `record_preview_if_current`, qui compare
   — dans la même transaction que l'écriture — la chaîne `settings_json`
   captée en phase 1 à celle actuellement stockée pour cette révision.
   Égalité ⇒ écriture ; différence (ou révision disparue) ⇒ rien n'est
   écrit, le fichier rendu reste affichable pour cet appel mais n'est pas
   marqué valide, un appel `preview` ultérieur régénère.

L'invariant garanti : un amendement concurrent ne peut jamais faire passer
pour valide un rendu obtenu avec d'anciens réglages. `commit_revision`,
`undo` et `redo` ne modifient jamais `settings_json` d'une révision
existante, donc ne déclenchent jamais ce garde-fou — seul `try_amend_head`
peut le faire, exactement le cas visé.

## Conséquences

* Toute la navigation/recherche/édition de métadonnées du catalogue reste
  disponible pendant un rendu, y compris sous charge du pool de rendu borné
  (jusqu'à 16 jobs concurrents) — le blocage global disparaît.
* Nouvelle méthode catalogue additive (`Catalog::record_preview_if_current`),
  aucune migration de schéma, aucun changement de l'API publique du moteur
  (`Library::preview` garde sa signature).
* Pire cas en cas de course avec un amendement : un rendu gaspillé, jamais
  une preview corrompue — le prochain appel régénère normalement. Ceci
  renforce la règle déjà documentée en `catalog.md` §17 (l'amendement
  invalide les previews de la révision) au lieu de la changer.
* Le décodage RAW reste sérialisé par `Mutex<DecodeCache>` pendant sa propre
  durée (inchangé, hors scope) — un futur resserrement de cette section
  précise resterait un correctif indépendant et plus modeste si jamais
  mesuré nécessaire.
* Aucun changement aux formules `process1`–`5` : uniquement le moment où le
  verrou catalogue est tenu, jamais l'ordre ou la valeur des calculs de
  pixels.

## Alternatives écartées

* **`RwLock<Catalog>`** : ne compile pas pour des lectures concurrentes,
  `rusqlite::Connection` étant `!Sync` — aucun bénéfice.
* **Pool de connexions lecteur/écrivain (WAL)** : direction pertinente à long
  terme pour la contention lecture-vs-écriture en général, mais changement
  invasif sur toute `leyline-catalog` pour un gain qui dépasse le bug ciblé
  ici ; reporté à une décision séparée si la contention devient mesurable.
* **Verrou par asset** : ajoute une carte de verrous et de la complexité sans
  répondre à la contention catalogue globale (la navigation touche tous les
  assets) ; le mutex catalogue global resterait de toute façon tenu pour les
  opérations SQL elles-mêmes.
* **Réduire la fenêtre sans garde-fou d'atomicité** : rejeté — réintroduit la
  corruption silencieuse décrite ci-dessus au bénéfice de la seule fenêtre
  d'amendement, un cas réel et déjà utilisé (glissement de curseur).
