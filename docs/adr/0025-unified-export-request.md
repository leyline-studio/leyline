# ADR 0025 — Une seule requête d'export : `ExportRequest`/`ExportRecipe`

**Statut :** Accepté — 2026-07

## Contexte

`engine-api.md` §12 documentait depuis son premier jet une forme cible à
requête unique (`ExportRequest { versions, preset, destination }` → un seul
`Library::export(request) -> Result<JobId>`), marquée « reste à venir »: la
surface réellement livrée avait divergé en cinq points d'entrée sur
`Library` — `export` (une version, synchrone), `export_batch` (recette ad
hoc, synchrone), `export_with_preset` (preset stocké, synchrone),
`export_async` (recette ad hoc, job), `export_with_preset_async` (preset
stocké, job). Chaque paire synchrone/job dupliquait la même logique de
validation, de résolution de preset et de bouclage par version — déjà
factorée une fois en interne par `ADR 0024` (`export_batch_with_preset`)
mais restée invisible aux appelants (CLI, Studio) qui devaient choisir entre
cinq noms de méthode selon deux axes indépendants (recette ad hoc vs.
preset ; synchrone vs. job) au lieu d'une seule décision.

Le brouillon initial ne prévoyait cependant qu'un champ `preset:
ExportPresetId` obligatoire — pas de recette ad hoc — et un unique `export`
retournant toujours un `JobId`. Les deux prémisses ne correspondaient pas à
l'usage réel : la CLI (`leyline export` sans `--preset`) et le dialogue
d'export de Studio (« Web », recette non enregistrée) ont besoin d'une
recette ad hoc aussi souvent que d'un preset stocké ; et un usage scripté
(CLI, SDK) veut pouvoir exporter un lot et récupérer le rapport directement,
sans avoir à s'abonner aux événements pour un appel ponctuel.

## Décision

Une requête unique remplace les cinq méthodes :

```rust
pub enum ExportRecipe {
    Adhoc(ExportSettings),
    Preset(ExportPresetId),
}

pub struct ExportRequest {
    pub versions: Vec<VersionId>,
    pub recipe: ExportRecipe,
    pub destination_dir: PathBuf,
}

impl Library {
    pub fn export(&self, request: &ExportRequest,
                  progress: impl FnMut(u64, u64)) -> Result<ExportReport>;
    pub fn export_async(&self, request: ExportRequest) -> JobId;
}
```

`ExportRecipe` remplace le champ `preset: ExportPresetId` forcé du brouillon
par une union à deux variantes — la même « recette ad hoc ou preset stocké »
que `export_batch`/`export_with_preset` distinguaient déjà par le nom de la
méthode, maintenant portée par le type plutôt que par le choix de
l'appelant entre deux call sites. Un `ExportRecipe::Preset` est résolu une
seule fois, sous verrou court, avant la boucle par version — un preset
modifié en cours de requête ne change donc pas rétroactivement les versions
déjà exportées, la même garantie que `export_with_preset` offrait déjà.

`export` reste synchrone (recette : le brouillon ne prévoyait qu'un
`JobId`) : la CLI et un usage SDK scripté n'ont pas besoin du mécanisme
d'événements pour un export ponctuel, et `export_async` reste le point
d'entrée pour Studio, qui veut retourner immédiatement et suivre
`JobProgress`/`JobFinished`. `export_async` construit sur `export` (même
relation qu'avant cet ADR entre `export_batch` et son job), donc le
narrowing du verrou catalogue par version d'`ADR 0024` s'applique
identiquement aux deux formes.

## Conséquences

* `Library` passe de cinq méthodes d'export (`export`, `export_batch`,
  `export_with_preset`, `export_async`, `export_with_preset_async`) à deux
  (`export`, `export_async`) plus `export_presets`/`create_export_preset`
  inchangées. `leyline-engine` étant interne (§13), ce renommage n'est pas
  un changement cassant au sens semver — mais il touche les trois clients du
  dépôt : CLI, Studio, et les tests d'intégration, tous mis à jour dans le
  même changement.
* Les fonctions libres `export::export_version`/`export::export_batch`
  (utilisées directement par les tests d'intégration de `leyline-engine` sur
  un `&mut Catalog`) ne changent pas : elles restent le noyau bas niveau, pas
  la façade.
* Aucun changement de schéma catalogue, aucun changement aux formules
  `process1`–`5` : uniquement la forme de la requête et le nombre de points
  d'entrée.

## Alternatives écartées

* **Garder cinq méthodes, ajouter `ExportRequest` en sixième forme de
  confort** : rejeté — duplique la surface au lieu de la réduire, l'inverse
  de l'objectif ; les cinq méthodes existantes n'apportaient chacune aucune
  capacité que la requête unique ne couvre pas.
* **`preset: ExportPresetId` obligatoire, conforme au brouillon initial** :
  rejeté, cf. Contexte — la recette ad hoc est un cas réel et déjà utilisé
  partout (CLI par défaut, dialogue Studio), pas un besoin hypothétique à
  anticiper.
* **`export` retournant toujours un `JobId`, conforme au brouillon initial** :
  rejeté — un appel scripté ponctuel (CLI, SDK) n'a pas besoin du mécanisme
  d'événements ; la forme synchrone existait déjà (`export_batch`,
  `export_with_preset`) et n'avait pas de raison de disparaître.
