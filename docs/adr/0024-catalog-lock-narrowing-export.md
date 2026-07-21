# ADR 0024 — Ne pas tenir le verrou catalogue pendant un rendu d'export

**Statut :** Accepté — 2026-07

## Contexte

`ADR 0023` a resserré la fenêtre du verrou catalogue autour de `Library::preview` :
le rendu (décodage RAW + pipeline `process1`–`5` + encodage) ne touche jamais le
catalogue, donc le tenir verrouillé pendant ce temps bloquait toute la
navigation, la recherche et l'édition de métadonnées de Studio pour rien.

`Library::export`, et les lots qu'il sous-tend (`export_batch`,
`export_with_preset`, et leurs jobs `export_async` /
`export_with_preset_async`), avaient le même défaut, en pire : un rendu de
preview coûte quelques dizaines à ~100 ms, mais un export ajoute le scaling
et un encodage plein format (JPEG/TIFF/WebP/AVIF), et surtout `export_batch`
enchaînait toutes les versions du lot **sous un seul verrou pris une fois au
début** — un lot de plusieurs dizaines de photos pouvait donc geler tout
accès catalogue pendant plusieurs minutes, alors que Studio laisse
l'utilisateur continuer à trier et éditer pendant qu'un export tourne en
tâche de fond.

Contrairement à une preview, un export n'est pas un cache indexé par
révision que `valid_preview` pourrait plus tard servir comme « à jour » :
c'est un fichier one-shot que l'appelant a demandé une fois, écrit sur disque
et journalisé dans `export_history` (§28) pour l'historique — rien ne le
relit ensuite pour décider s'il est encore « valide ». Le garde-fou
d'atomicité d'ADR 0023 (`record_preview_if_current`), nécessaire parce qu'un
amendement concurrent pouvait faire passer un rendu périmé pour la preview
valide de la révision qu'il vient de réécrire, n'a donc pas d'équivalent
ici : rien ne peut faire passer un export pour autre chose que ce qu'il est.
Le pire cas d'une course avec un amendement reste identique à avant cette
décision — le fichier exporté reflète les réglages lus au moment du plan, pas
forcément les tout derniers — un comportement déjà inhérent à « exporter à la
révision de tête », pas quelque chose que cette décision change.

## Décision

`export::export_version` se scinde en trois phases séquencées par
l'appelant, sur le même modèle qu'ADR 0023, avec le verrou catalogue tenu
seulement pour la première et la dernière — jamais pendant le rendu :

1. **`plan_export`** (lecture seule, verrou court) : lit l'asset, la tête, les
   réglages de développement de la révision, le chemin source et les
   métadonnées objectif, et calcule le radical du nom de fichier de sortie.
2. **`render_export`** : aucun verrou catalogue tenu — décodage, rendu
   `process1`–`5`, mise à l'échelle éventuelle, refus si le fichier de
   destination existe déjà (règle « jamais d'écrasement »), et encodage sur
   disque.
3. **`journal_export`** (écriture, verrou court) : enregistre l'export dans
   `export_history`.

`Library::export` séquence ces trois phases en relâchant le verrou entre la
première et la deuxième. `Library::export_batch` et
`Library::export_with_preset` ne prennent plus le verrou une seule fois pour
tout le lot : ils appellent `Library::export` version par version, donc le
verrou n'est jamais tenu plus longtemps que le plan + le journal d'**une
seule** version à la fois — la même contrainte que si le client appelait
`export` en boucle lui-même.

Les fonctions libres `export::export_version` et `export::export_batch`
(utilisées directement par les tests d'intégration de ce crate) gardent leur
signature `&mut Catalog` tenu de bout en bout — elles pilotent les trois
phases sur un seul verrou, comme `preview::preview` le fait pour ADR 0023.

## Conséquences

* Toute la navigation/recherche/édition de métadonnées du catalogue reste
  disponible pendant un export ou un lot d'export, y compris un lot de
  plusieurs dizaines de versions — le gel global disparaît.
* Aucun changement de schéma catalogue, aucune nouvelle méthode catalogue
  (contrairement à ADR 0023, pas de garde-fou d'atomicité nécessaire ici) ;
  signatures publiques de `Library::export`, `export_batch`,
  `export_with_preset` et de leurs jobs inchangées.
* Aucun changement aux formules `process1`–`5` ni au format du fichier
  journalisé : uniquement le moment où le verrou catalogue est tenu et le
  découpage par version d'un lot.

## Alternatives écartées

* **Garder le verrou pour tout le lot mais le relâcher entre chaque
  version** (au lieu de router chaque version par `Library::export`) :
  équivalent en pratique mais duplique la logique de narrowing déjà écrite
  pour l'appel unique — router par `Library::export` réutilise le même code
  et garantit qu'un futur changement du narrowing (ex. un futur garde-fou)
  s'applique aux deux chemins sans double maintenance.
* **Garde-fou d'atomicité façon `record_preview_if_current`** : écarté, cf.
  Contexte — rien ne relit un export pour décider s'il est « à jour »,
  contrairement à une preview.
