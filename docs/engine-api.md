# Engine API Specification

**Document:** `docs/engine-api.md`
**Version:** 1.0
**Status:** Draft

---

# 1. Objectif

Définir le contrat entre le moteur Leyline et ses clients.

L'interface graphique n'est qu'un client parmi d'autres :

```text
Leyline Studio ─┐
Leyline CLI    ─┼──→ leyline-sdk ──→ leyline-engine ──→ leyline-core
Scripts / apps ─┘
```

Principe fondateur du projet : **API avant interface graphique**.

Tout ce que Studio sait faire, la CLI et le SDK savent le faire — parce qu'ils appellent exactement la même API.

---

# 2. Principes

* L'API est une **bibliothèque Rust** (`leyline-sdk`), pas un serveur ni un protocole réseau.
* Aucune opération lourde ne bloque l'appelant : lectures rapides synchrones, travaux lourds asynchrones.
* Le moteur possède ses threads ; le client possède sa boucle d'événements.
* Aucun runtime asynchrone imposé (pas de dépendance `tokio`) : threads natifs + canaux.
* Toute erreur est une valeur (`Result`), jamais un panic à travers la frontière de l'API.
* Les identifiants sont des types dédiés, jamais des entiers nus.

---

# 3. Modèle d'exécution

## 3.1 Deux catégories d'appels

| Catégorie | Exemples | Comportement |
|---|---|---|
| **Requêtes** | grille, détails d'un asset, réglages courants, collections | Synchrones — SQLite répond en microsecondes, un aller-retour de thread coûterait plus cher |
| **Travaux** | import, rendu de preview, export, reprocessing | Asynchrones — retournent un `JobId` immédiatement, progressent via événements |

## 3.2 Événements

Le client s'abonne à un flux d'événements :

```rust
pub enum Event {
    AssetsAdded { asset_ids: Vec<AssetId> },
    AssetsChanged { asset_ids: Vec<AssetId> },
    VersionChanged { version_id: VersionId },
    PreviewReady { asset_id: AssetId, kind: PreviewKind },
    JobProgress { job_id: JobId, done: u64, total: u64 },
    JobFinished { job_id: JobId, result: JobResult },
    LibraryClosed,
    TetherConnected,
    TetherDisconnected { reason: Option<String> },
    WatchStarted { folder: PathBuf },
    WatchStopped { reason: Option<String> },
}

pub enum JobResult {
    Import(ImportReport),   // échecs par fichier inclus dans le rapport
    Export(ExportReport),   // échecs par version inclus dans le rapport
    Preset(PresetApplyReport), // échecs par version inclus dans le rapport
    Preview(PreviewFile),
    Failed(String),         // le job a échoué avant de produire quoi que ce soit
}
```

* La souscription rend un `Receiver<Event>` (canal standard) : `library.subscribe()`.
* Studio branche ce canal sur la boucle Slint ; la CLI le lit en séquence ; un script peut l'ignorer. Un récepteur abandonné se désabonne silencieusement.
* Les événements sont des **notifications**, jamais des données complètes : le client re-requête ce dont il a besoin. Cela évite tout problème de cohérence entre le flux et la base.
* `PreviewReady` porte l'asset (pas la version) : la surface preview est asset-based (§11), la preview rendue est toujours celle de la version courante de l'asset.
* `TetherConnected`/`TetherDisconnected` bornent le cycle de vie d'une session `tether_connect`/`tether_disconnect` (§6bis) — chaque photo capturée pendant la session notifie via `AssetsAdded`, exactement comme un import : ce n'est pas un événement distinct, seulement une source différente pour le même import.
* `WatchStarted`/`WatchStopped` suivent le même principe pour `watch_start`/`watch_stop` (§6ter, `docs/adr/0039-watched-folder-import.md`) : chaque fichier stabilisé dans le dossier surveillé notifie via `AssetsAdded`.
* **État livré** : `subscribe` et les jobs `import_async`, `preview_async`, `export_async` émettent `JobProgress`, `AssetsAdded`, `PreviewReady` et `JobFinished`. Les écritures de la façade notifient : classement (§8) → un `VersionChanged` par version du lot ; mots-clés (§8) → `AssetsChanged` avec le lot ; chaque écriture d'historique d'une session d'édition (§10.1 — commit, amendement, undo, redo) → `VersionChanged`. L'application d'un preset (§10.3) ne notifie rien de plus : c'est un commit de session par version ciblée, donc les mêmes `VersionChanged` que §10.1, portés par le job `apply_preset_async`. Un client qui écrit via `catalog_mut()` directement contourne les notifications : passer par la façade. `close()` (§5) émet `LibraryClosed` à tous les abonnés du flux partagé ; les autres clones de la `Library` restent utilisables — seule la connexion catalogue ferme, et seulement quand le dernier clone est abandonné.

## 3.3 Threading

* `Library` est `Send + Sync` et se clone à coût nul (`Arc` interne). Chaque clone partage le même catalogue et le même flux d'événements.
* Les accès catalogue passent par des gardes (`catalog()` / `catalog_mut()`) qui tiennent le verrou interne : une opération, puis relâcher — ne jamais garder une garde en travers d'un autre appel à la `Library` (une session d'édition tient la garde pour sa durée de vie, c'est voulu : rien d'autre ne mute pendant l'édition).
* Chaque job `*_async` (`import_async`, `preview_async`, `export_async`, `export_with_preset_async`, `apply_preset_async`, `reprocess_async`) s'exécute sur un pool de jobs partagé et borné (un jeu fixe de threads dédiés, dimensionné à `available_parallelism()` plafonné à 16, un pool par `Library`) plutôt que sur un thread dédié par appel : la concurrence des jobs a un plafond imposé par le moteur, quel que soit le comportement du client — au-delà du plafond, les jobs suivants attendent en file. Ce pool est volontairement distinct du pool rayon global utilisé pour le rendu pixel (§10, `pixels.rs`, `process1..5.rs`) : partager un même pool rayon entre la distribution des jobs et le travail `par_iter`/`join` interne au rendu exposerait le vol de tâches à recruter le thread d'un job pour exécuter un *autre* job pendant qu'il tient encore le mutex catalogue — un verrou non réentrant, donc un deadlock. Le pool de jobs appartient au même `Arc` interne que le catalogue : ses threads tournent tant qu'un clone de la `Library` existe et s'arrêtent d'eux-mêmes quand le dernier disparaît ; les jobs en cours ou en attente ne sont ni annulés ni interrompus par `close()`.
* Les écritures catalogue sont sérialisées en interne ; le client n'a aucune contrainte d'ordre à respecter.

---

# 4. Types fondamentaux

```rust
pub struct AssetId(i64);
pub struct VersionId(i64);
pub struct RevisionId(i64);
pub struct CollectionId(i64);
pub struct KeywordId(i64);
pub struct PresetId(i64);
pub struct JobId(u64);

pub enum LeylineError {
    LibraryNotFound(PathBuf),
    LibraryLocked,
    NewerCatalog { found: u32, supported: u32 },
    AssetMissing(AssetId),
    VersionMissing(VersionId),
    RevisionMissing(RevisionId),
    KeywordMissing(KeywordId),
    CollectionMissing(CollectionId),
    PresetMissing(PresetId),
    DecodeFailed { asset: AssetId, reason: String },
    InvalidSettings(String),
    NewerSettings { schema: u32 },
    UnknownStage { stage: String, version: u16 },
    MixedWorkingSpaces {
        stage: String, version: u16, space: String,
        other_stage: String, other_version: u16, other_space: String,
    },
    InvalidImage(String),
    Io(std::io::Error),
    Db(String),
}

pub type Result<T> = std::result::Result<T, LeylineError>;
```

`NewerCatalog` et `NewerSettings` matérialisent la règle de compatibilité ascendante (`pipeline.md` §3.4) : un moteur ancien ouvre en lecture seule ou refuse — au niveau du catalogue comme d'une révision — mais ne modifie jamais. Face à `NewerSettings`, le client affiche la meilleure preview en cache avec un avertissement.

---

# 5. Bibliothèque

```rust
impl Library {
    /// Crée une nouvelle bibliothèque (dossier + catalog.db).
    pub fn create(root: &Path, name: &str) -> Result<Library>;

    /// Ouvre une bibliothèque existante. Applique les migrations si besoin.
    pub fn open(root: &Path) -> Result<Library>;

    /// Ouvre sans droit d'écriture (catalogue plus récent que le moteur).
    pub fn open_read_only(root: &Path) -> Result<Library>;

    pub fn subscribe(&self) -> Receiver<Event>;

    pub fn close(self) -> Result<()>;
}
```

Une seule instance en écriture par bibliothèque (verrou fichier) ; plusieurs lecteurs sont libres (WAL).

**Studio, lancement sans argument (ADR 0022).** `leyline-studio` prend en argument facultatif un chemin de bibliothèque (`leyline-studio <library-dir>`), exactement comme un client CLI ferait `Library::open`. Lancé sans argument — ce qui est le cas normal depuis un raccourci graphique (menu Démarrer de l'installeur Windows, AppImage Linux, double-clic sur le `.app` macOS, aucun n'attachant de console) — Studio ne remonte plus une erreur d'usage : il ouvre ou crée, via `Library::create`/`Library::open` selon qu'un `catalog.db` existe déjà, une bibliothèque par défaut sous `<Documents de l'utilisateur>/Leyline Library` (repli sur `<home>/Leyline Library` si le système n'a pas de dossier Documents). Cet emplacement reste visible dans l'app (Aide ▸ À propos de Leyline). Un argument explicite garde le comportement historique à l'identique : `Library::open` seul, donc une erreur franche si le chemin donné n'existe pas.

---

# 6. Import

```rust
pub struct ImportOptions {
    pub copy_files: bool,      // copier dans Photos/ ou référencer sur place
    pub recursive: bool,
}

impl Library {
    /// Le cœur synchrone : tient le catalogue pendant tout le lot.
    pub fn import(&self, source: &Path, options: &ImportOptions,
                  progress: impl FnMut(u64, u64)) -> Result<ImportReport>;
    /// Le job : retourne immédiatement, progresse par `JobProgress`,
    /// annonce `AssetsAdded` puis `JobFinished` avec le rapport.
    pub fn import_async(&self, source: &Path, options: &ImportOptions) -> JobId;
}
```

L'import est un travail : extraction EXIF, checksum BLAKE3, création de la révision initiale et de la version `Default` (catalogue §18) — en flux, avec `JobProgress` par fichier candidat. Chaque asset importé avec succès reçoit aussi sa miniature (`PreviewKind::Thumbnail`) avant que `import`/`import_async` ne retourne, via le même cœur que `preview_async` (§11) : le client n'a plus besoin de la déclencher lui-même après coup. Un échec de rendu de miniature n'annule jamais l'import de l'asset — il reste importé, sans miniature en cache, et retombe sur le chemin paresseux existant (`cached_preview` puis `preview_async`) la première fois qu'il doit s'afficher. Ce rendu est fait séquentiellement, asset par asset : la miniature partage le verrou du catalogue avec le reste de l'import (§11), le paralléliser demanderait de revoir ce verrouillage, pas seulement d'itérer avec `rayon`.

Un **sidecar XMP** posé à côté du fichier source amorce l'asset qui vient d'être créé — note, libellé, mots-clés hiérarchiques, artiste, copyright ([ADR 0047](adr/0047-xmp-sidecar-read.md), catalogue §29) : c'est le chemin de migration depuis un autre logiciel, et il ne demande aucune option. Comme la miniature, c'est du meilleur effort : un sidecar illisible n'écarte jamais la photo, il s'écarte lui-même. Pour un asset déjà importé, `Library::read_xmp(asset) -> Result<bool>` fait la même chose à la demande, en remplissant sans jamais écraser.

---

# 6bis. Capture tethering (`docs/adr/0038-tethered-capture.md`)

```rust
impl Library {
    /// Se connecte à la première caméra USB détectée (libgphoto2) et
    /// démarre une session : chaque photo prise à partir de là est
    /// téléchargée et importée automatiquement, comme un import ordinaire.
    /// Refuse une deuxième session tant qu'une est déjà ouverte.
    pub fn tether_connect(&self) -> Result<()>;

    /// Termine la session en cours ; ne fait rien si aucune n'est ouverte.
    pub fn tether_disconnect(&self);
}
```

Une capture tethering n'est pas un chemin de données séparé : le fichier
reçu de l'appareil passe par le même cœur d'import que `Library::import`
(checksum, EXIF, révision initiale, vignette), donc émet le même
`Event::AssetsAdded` (§3.2). Seuls `TetherConnected`/`TetherDisconnected`
sont nouveaux, pour signaler la connexion elle-même — une caméra à la
fois par `Library` en V1.

---

# 6ter. Import automatique par dossier surveillé (`docs/adr/0039-watched-folder-import.md`)

```rust
impl Library {
    /// Démarre la surveillance de `folder` : chaque fichier qui s'y
    /// stabilise à partir de là est importé automatiquement, comme un
    /// import ordinaire. Refuse une deuxième session tant qu'une est déjà
    /// active.
    pub fn watch_start(&self, folder: &Path) -> Result<()>;

    /// Termine la session en cours ; ne fait rien si aucune n'est active.
    pub fn watch_stop(&self);
}
```

Même principe que le tethering (§6bis) : un fichier stabilisé dans le
dossier surveillé passe par le même cœur d'import que `Library::import`
(checksum, EXIF, révision initiale, vignette), donc émet le même
`Event::AssetsAdded` (§3.2). Seuls `WatchStarted`/`WatchStopped` sont
nouveaux, pour signaler la session elle-même — un dossier surveillé à la
fois par `Library` en V1. Contrairement au tethering, chaque fichier est
importé individuellement au fil de l'eau (jamais en lot), pour que le
verrou du catalogue que `Library::import` tient pendant tout son appel
(§3.3) ne reste jamais bloqué plus longtemps qu'un seul fichier, même si le
dossier en reçoit beaucoup d'un coup — sinon toute opération interactive en
mode développement partageant ce même verrou attendrait derrière le lot
entier.

---

# 7. Navigation et recherche

La grille énumère des **versions** (catalogue §16).

```rust
pub struct GridQuery {
    pub folder: Option<FolderId>,
    pub collection: Option<CollectionId>,
    pub rating_at_least: Option<u8>,
    pub color_label: Option<ColorLabel>,
    pub pick: Option<PickState>,
    pub keywords: Vec<KeywordId>,        // hiérarchique : inclut les descendants
    pub text: Option<String>,            // FTS5
    pub capture_range: Option<(i64, i64)>,
    pub sort: Sort,
    pub range: Range<u32>,               // pagination par fenêtre
}

pub struct GridItem {
    pub version_id: VersionId,
    pub asset_id: AssetId,
    pub filename: String,
    pub capture_date: Option<i64>,
    pub rating: Option<u8>,
    pub color_label: Option<ColorLabel>,
    pub pick: PickState,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub edited: bool,                    // plus que sa révision initiale (ADR 0055 §5)
}

impl Library {
    pub fn count(&self, query: &GridQuery) -> Result<u64>;
    pub fn grid(&self, query: &GridQuery) -> Result<Vec<GridItem>>;
    pub fn asset(&self, id: AssetId) -> Result<AssetDetails>;   // EXIF complet, versions, chemins
}
```

`grid` + `range` permettent le défilement virtuel : l'UI ne charge jamais que la fenêtre visible, quelle que soit la taille du catalogue.

---

# 8. Classement

Le classement vit sur la **version** ; les mots-clés sur l'**asset** (catalogue §16).

```rust
impl Library {
    pub fn set_rating(&self, ids: &[VersionId], rating: Option<u8>) -> Result<()>;
    pub fn set_color_label(&self, ids: &[VersionId], label: Option<ColorLabel>) -> Result<()>;
    pub fn set_pick(&self, ids: &[VersionId], pick: PickState) -> Result<()>;

    pub fn add_keyword(&self, assets: &[AssetId], keyword: KeywordId) -> Result<()>;
    pub fn remove_keyword(&self, assets: &[AssetId], keyword: KeywordId) -> Result<()>;
    pub fn create_keyword(&self, parent: Option<KeywordId>, name: &str) -> Result<KeywordId>;
    pub fn keyword_tree(&self) -> Result<Vec<KeywordNode>>;
}
```

Toutes les opérations acceptent des lots : le traitement par lots est un cas nominal, pas une option.

---

# 9. Collections

```rust
impl Library {
    pub fn create_collection(&self, parent: Option<CollectionId>, name: &str) -> Result<CollectionId>;
    pub fn create_smart_collection(&self, parent: Option<CollectionId>, name: &str, rules: SmartRules) -> Result<CollectionId>;

    pub fn add_to_collection(&self, id: CollectionId, versions: &[VersionId]) -> Result<()>;
    pub fn remove_from_collection(&self, id: CollectionId, versions: &[VersionId]) -> Result<()>;
    pub fn reorder_collection(&self, id: CollectionId, order: &[VersionId]) -> Result<()>;

    pub fn collections(&self) -> Result<Vec<CollectionNode>>;

    pub fn rename_collection(&self, id: CollectionId, name: &str) -> Result<()>;
    pub fn move_collection(&self, id: CollectionId, parent: Option<CollectionId>) -> Result<()>;
    // Emporte le sous-arbre ; rend le nombre de collections supprimées.
    pub fn delete_collection(&self, id: CollectionId) -> Result<u32>;
}
```

Les trois dernières ne touchent aucune version, aucune révision, aucun fichier
(catalogue §24) : un déplacement circulaire est refusé, et une suppression
emporte les descendants et les seules appartenances.

Les **dossiers** se lisent par la même forme, en lecture seule (ADR 0055 §2) — rien ici ne renomme, ne déplace ni ne supprime un dossier, c'est de la gestion de fichiers :

```rust
impl Library {
    pub fn folders(&self) -> Result<Vec<FolderNode>>;   // chemin, parent, nombre de photos
}
```

Les lignes arrivent triées par chemin, c'est-à-dire en profondeur d'abord : un client peut indenter sur le nombre de segments sans parcourir d'arbre.

Une collection intelligente s'interroge via `GridQuery { collection: Some(id), .. }` : le moteur traduit `SmartRules` en SQL (catalogue §26), le client ne voit pas la différence avec une collection manuelle.

---

# 10. Développement

## 10.1 Session d'édition

L'édition passe par une **session**, qui matérialise la coalescence (`pipeline.md`, catalogue §17) :

```rust
impl Library {
    pub fn edit(&self, version: VersionId) -> Result<EditSession>;
}

impl EditSession {
    /// Valeur en mémoire, aperçu temps réel — aucune écriture catalogue.
    pub fn set(&mut self, param: Param, value: Value) -> Result<()>;

    /// Point de commit : crée la révision, avance la tête.
    /// Le moteur applique la fenêtre d'amendement automatiquement.
    pub fn commit(&mut self) -> Result<RevisionId>;

    pub fn undo(&mut self) -> Result<Option<RevisionId>>;
    pub fn redo(&mut self) -> Result<Option<RevisionId>>;

    pub fn settings(&self) -> &Settings;       // état courant complet
    pub fn history(&self) -> Result<Vec<RevisionInfo>>;
}
```

* `set` est appelé à chaque mouvement de curseur : le moteur met à jour l'aperçu en mémoire.
* `commit` est appelé aux points de commit définis par `pipeline.md` (relâchement, changement d'outil...). La session décide seule s'il s'agit d'une nouvelle révision ou d'un amendement.
* Fermer la session (drop) commite l'état en attente : rien ne se perd jamais.

## 10.2 Versions

```rust
impl Library {
    /// Version virtuelle : nouvelle branche depuis la tête (ou une révision donnée).
    pub fn create_version(&self, from: VersionId, name: &str, at: Option<RevisionId>) -> Result<VersionId>;

    pub fn versions(&self, asset: AssetId) -> Result<Vec<VersionInfo>>;
    pub fn set_current_version(&self, asset: AssetId, version: VersionId) -> Result<()>;
    pub fn rename_version(&self, version: VersionId, name: &str) -> Result<()>;
    pub fn delete_version(&self, version: VersionId) -> Result<()>;   // refuse la dernière version
}
```

---

## 10.3 Presets

Un preset (`docs/presets.md`) capture un sous-ensemble de `Param` (§10.1) — jamais l'état complet — et s'applique en écrivant une révision normale sur chaque version ciblée. Aucun nouveau mécanisme de rendu ou d'écriture : l'application réutilise `EditSession::set`/`commit` tels quels (`docs/adr/0014-develop-presets.md`).

```rust
pub enum SettingsGroup {
    WhiteBalance,
    Tone,
    Presence,
    LensCorrection,
    Detail,
    Geometry,
}

pub struct PresetInfo {
    pub id: PresetId,
    pub name: String,
    pub groups: Vec<SettingsGroup>,
}

/// Outcome of one preset application batch — même forme qu'`ExportReport` (§12).
pub struct PresetApplyReport {
    pub applied: Vec<VersionId>,
    pub failed: Vec<(VersionId, String)>,
}

impl Library {
    /// Capture les champs des `groups` demandés depuis la tête de `from` (§10.1).
    pub fn create_preset(&self, name: &str, from: VersionId, groups: &[SettingsGroup]) -> Result<PresetId>;
    pub fn presets(&self) -> Result<Vec<PresetInfo>>;
    pub fn rename_preset(&self, id: PresetId, name: &str) -> Result<()>;
    pub fn delete_preset(&self, id: PresetId) -> Result<()>;

    /// Le job : `JobProgress` par version, puis `JobFinished` avec le rapport
    /// (échecs par version dans le rapport, échec du lot en `Failed`).
    pub fn apply_preset_async(&self, preset: PresetId, versions: Vec<VersionId>) -> JobId;
}
```

* `SettingsGroup` regroupe les `Param` de §10.1 à la granularité des cases à cocher du preset (`docs/presets.md` §3.1) : `Tone` = Exposure + Contrast + Highlights + Shadows + Whites + Blacks, `Presence` = Vibrance + Saturation, `Detail` = NoiseReduction + Sharpening, `Geometry` = Rotation + Crop ; les autres groupes correspondent chacun à un seul `Param`.
* `apply_preset_async` est un **travail** (§3.1) même pour une seule version : une sélection peut aller jusqu'à toute la bibliothèque (`docs/presets.md` §5.2), et une seule catégorie d'appel évite de faire dépendre requête/travail de la taille de la sélection au moment de l'appel.
* Pour chaque version du lot : ouvrir une session fraîche via `Library::edit` (§10.1), `set` chaque paramètre des groupes inclus, puis `commit` une seule fois. Une session neuve n'a pas d'historique de commit à amender (`last_commit` vide, §10.1) : le commit est donc toujours une nouvelle révision, jamais un amendement — sans avoir à modifier la politique de coalescence pour ce cas.
* Une version en échec (typiquement `NewerSettings`, catalogue §17/§3.4, si le preset ou la tête référence un schéma que le moteur ne connaît plus) rejoint `PresetApplyReport::failed` avec la raison ; les autres versions du lot continuent (`docs/presets.md` §5.2).
* `create_preset` ne connaît que le vocabulaire de `Settings` (`leyline-core`) : il lit la tête de `from`, garde les champs des groupes demandés, sérialise le `preset_json` avec `schema` = celui de cette tête (`docs/presets.md` §3.2).

---

## 10.4 Retraitement

Remonter chaque étage épinglé d'une version à sa version courante (`pipeline.md` §4.5) — typiquement pour qu'une photo éditée avant la correction d'un opérateur en bénéficie sans que l'utilisateur ne touche un seul curseur.

```rust
impl EditSession {
    /// Remonte la tête aux versions d'étages courantes : nouvelle révision, mêmes
    /// paramètres. Rien s'il n'y a rien à faire.
    pub fn reprocess(&mut self) -> Result<RevisionId>;
}

/// Outcome of one reprocess batch — même forme que `PresetApplyReport` (§10.3).
pub struct ReprocessReport {
    pub reprocessed: Vec<VersionId>,
    pub already_current: Vec<VersionId>,
    pub failed: Vec<(VersionId, String)>,
}

impl Library {
    pub fn reprocess(&self, versions: &[VersionId], progress: impl FnMut(u64, u64)) -> Result<ReprocessReport>;

    /// Le job : `JobProgress` par version, puis `JobFinished` avec le rapport.
    pub fn reprocess_async(&self, versions: Vec<VersionId>) -> JobId;
}
```

* Toujours une nouvelle révision, jamais un amendement : §4.5 dit explicitement que le retraitement « conserve les anciennes » exécutions — étendre la dernière modification de l'utilisateur en place perdrait la distinction entre « ce que l'utilisateur a réglé » et « ce que le moteur a migré ».
* Une version déjà sur `CURRENT_PROCESS` ne produit aucune révision : `EditSession::reprocess` renvoie la tête inchangée, `ReprocessReport::already_current` la compte séparément — ni un succès qui écrit, ni un échec.
* `EditSession::reprocess` commite d'abord tout état en attente (même règle que `undo`/`redo`, §10.1) avant de migrer : rien ne se perd.
* `reprocess_async` est un **travail** (§3.1) pour la même raison que `apply_preset_async` : une sélection peut aller jusqu'à toute la bibliothèque.

---

# 11. Previews

```rust
pub enum Preview {
    /// À jour pour la tête de la version.
    Ready(PathBuf),
    /// Obsolète : utilisable pour l'affichage immédiat, régénération lancée.
    Stale { path: PathBuf, job: JobId },
    /// Rien en cache : génération lancée.
    Generating(JobId),
}
```

Le client affiche toujours quelque chose immédiatement (`Ready` ou `Stale`), puis se met à jour sur `PreviewReady`. La validité suit strictement le catalogue §20 (`revision_id` de tête).

**Surface livrée** : le get-or-generate synchrone, la lecture seule du cache, le job de rendu, et l'enum `Preview` (`Ready`/`Stale`/`Generating`) qui fusionne les trois en un seul appel.

```rust
impl Library {
    /// Get-or-generate synchrone : rend la preview si rien de valide en cache.
    pub fn preview(&self, asset: AssetId, kind: PreviewKind) -> Result<PreviewFile>;
    /// Lecture seule du cache : `None` si rien de valide, ne rend jamais.
    pub fn cached_preview(&self, asset: AssetId, kind: PreviewKind) -> Result<Option<PreviewFile>>;
    /// Le job : `PreviewReady` en cas de succès, puis `JobFinished`.
    pub fn preview_async(&self, asset: AssetId, kind: PreviewKind) -> JobId;
    /// Fusionne les trois appels ci-dessus derrière l'enum `Preview` : un
    /// cache à jour rend `Ready` sans rien lancer ; un cache d'une révision
    /// non-tête rend `Stale` avec le fichier obsolète et lance `preview_async` ;
    /// rien en cache rend `Generating` et lance `preview_async`.
    pub fn preview_state(&self, asset: AssetId, kind: PreviewKind) -> Result<Preview>;
}
```

`cached_preview` permet au client le même motif que `Ready`/`Generating` : afficher immédiatement ce qui existe, planifier la génération du reste — désormais via `preview_async` et l'événement `PreviewReady` (Studio peut remplacer son timer par ce flux), ou directement via `preview_state`.

La `Library` garde en mémoire les derniers décodages source (cache MRU borné, phase 7) : la boucle de développement re-rend le même asset après chaque commit de curseur, et sans ce cache chaque ajustement payait un décodage LibRaw complet. Les fichiers source ne changeant jamais (édition non-destructive), une entrée reste valide toute la vie du processus ; les pixels servis sont bit-à-bit ceux d'un décodage frais (`pipeline.md` §5), la reproductibilité n'est pas affectée.

---

# 12. Export

```rust
pub enum ExportRecipe {
    /// Réglages fournis par l'appelant, non stockés.
    Adhoc(ExportSettings),
    /// Un preset stocké (§27), résolu à l'exécution de la requête.
    Preset(ExportPresetId),
}

pub struct ExportRequest {
    pub versions: Vec<VersionId>,
    pub recipe: ExportRecipe,
    pub destination_dir: PathBuf,
}

impl Library {
    /// Synchrone : `progress` reçoit `(done, total)` par version ; un échec
    /// individuel ne stoppe pas les autres, il atterrit dans
    /// `ExportReport::failed`.
    pub fn export(&self, request: &ExportRequest,
                  progress: impl FnMut(u64, u64)) -> Result<ExportReport>;
    /// Le job : `JobProgress` par version, puis `JobFinished` avec le
    /// rapport (échecs par version dans le rapport, échec de la requête
    /// entière en `Failed`).
    pub fn export_async(&self, request: ExportRequest) -> JobId;
    pub fn export_presets(&self) -> Result<Vec<ExportPreset>>;
}
```

**Surface livrée** — la forme à `ExportRequest` unique décrite ci-dessus, avec deux différences assumées par rapport au brouillon initial de ce document : `recipe` est une union (`Adhoc`/`Preset`) plutôt qu'un `preset: ExportPresetId` forcé — une recette ad hoc est un cas réel (dialogue d'export de Studio, `leyline export` de la CLI sans `--preset`), pas seulement les presets stockés ; et `export` a une forme synchrone en plus du job, nécessaire pour un usage scripté (CLI, SDK) qui n'a pas besoin d'attendre un événement pour un export ponctuel. Une seule requête couvre maintenant ce qui existait avant comme trois entrées synchrones (`export`, `export_batch`, `export_with_preset`) et deux jobs (`export_async`, `export_with_preset_async`).

Studio pilote désormais ses dialogues d'import et d'export ainsi que ses vignettes de grille par ce flux : `import_async`/`export_async` pour les dialogues (progression affichée depuis `JobProgress`, résultat depuis `JobFinished`), `preview_async` pour les vignettes (jusqu'à 3 rendus en vol, remplis depuis `PreviewReady`).

L'export rend chaque version à sa révision de tête, avec les versions d'étages qu'elle déclare (`pipeline.md` §3.3), et journalise dans `export_history`. Les pixels rendus sont en sRGB (`adr/0015-color-management-srgb.md`) ; `leyline-export` embarque le profil ICC sRGB canonique (généré par LittleCMS, `leyline-color::srgb_icc_profile`) dans les fichiers JPEG, PNG et TIFF — WebP et AVIF s'en passent, faute de support ICC dans leurs bibliothèques d'encodage.

Comme `Library::preview` (§11, `adr/0023-catalog-lock-narrowing-preview.md`), `Library::export` ne tient le verrou catalogue que pour la lecture des réglages (résolution du preset le cas échéant) et l'écriture du journal — jamais pendant le décodage/rendu/encodage d'une version (`adr/0024-catalog-lock-narrowing-export.md`). Une requête à plusieurs versions ne bloque donc plus la navigation, la recherche ou l'édition de métadonnées pour toute sa durée, seulement version par version.

## 12.1 Impression (ADR 0036)

```rust
pub enum PrintRecipe {
    /// Réglages fournis par l'appelant, non stockés.
    Adhoc(PrintSettings),
    /// Un preset stocké (catalog.md §42), résolu à l'exécution de la requête.
    Preset(PrintPresetId),
}

pub struct PrintRequest {
    pub versions: Vec<VersionId>,
    pub recipe: PrintRecipe,
    pub destination_dir: PathBuf,
    /// Donnée de job, jamais dans le preset — comme `ExportRequest.versions`
    /// l'est d'`ExportRecipe`.
    pub copies: u32,
}

impl Library {
    /// Synchrone, même discipline que `Library::export`.
    pub fn print(&self, request: &PrintRequest,
                 progress: impl FnMut(u64, u64)) -> Result<PrintReport>;
    /// Le job : `JobProgress` par version, puis `JobFinished`.
    pub fn print_async(&self, request: PrintRequest) -> JobId;
    pub fn create_print_preset(&self, name: &str, settings: &PrintSettings) -> Result<PrintPresetId>;
    pub fn print_presets(&self) -> Result<Vec<PrintPreset>>;
}
```

L'impression n'est ni une process version ni un étage de pipeline (ADR 0036) : c'est « un export avec une dimension physique et un profil de destination ». Le moteur rend exactement comme pour un export (décodage → `render` → les étages de la révision), puis met à l'échelle dans la zone imprimable en pixels (`PrintSettings::target_pixels`, papier × DPI, à la place du `max_edge` d'un export) au lieu de mettre à l'échelle vers un bord le plus long, transforme optionnellement vers un profil ICC de destination (`leyline_color::OutputTransform`, ADR 0027) si `PrintSettings.profile` est renseigné, et encode le résultat en PDF une page (`leyline_export::encode_print`) — le hand-off le plus portable vers un flux d'impression OS (le risque explicitement nommé et différé par ADR 0036) : le PDF porte déjà sa taille physique et ses pixels dans le profil de destination, prêt à être remis tel quel au dialogue d'impression du système. Ce hand-off (Studio invoquant effectivement ce dialogue sur le fichier rendu) reste à câbler dans `leyline-studio`, sans nouvelle surface moteur (même patron que la barre de menu, ADR 0020).

Contrairement à l'export, il n'y a pas de journalisation : pas de table `print_history` (catalog.md §42) — un print ne modifie aucune révision et n'a pas besoin d'être retrouvé plus tard depuis le catalogue.

---

# 13. Stabilité de l'API

* `leyline-engine` est **interne** : son API peut changer à chaque version.
* `leyline-sdk` est la **surface stable** : semver strict, `0.x` jusqu'à la V1, puis engagement de compatibilité sur `1.x`.
* Les types de `leyline-core` (ids, erreurs, `Settings`) font partie du contrat SDK.
* Une passerelle C FFI (et donc bindings Python, etc.) est une évolution prévue, hors V1 — l'API décrite ici est conçue pour la rendre possible (pas de génériques exposés, pas de lifetimes dans les signatures publiques).

---

# 14. Ce que l'API ne fait pas

* Pas de rendu à l'écran : le moteur produit des fichiers et des buffers, l'affichage appartient au client.
* Pas de gestion de fenêtres, de raccourcis, de sélection UI.
* Pas d'accès réseau.
* Pas de manipulation directe de SQLite par les clients : le catalogue est une implémentation, pas une interface. Le schéma (`catalog.md`) est documenté pour la pérennité des données, pas comme API publique.
