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
    PreviewReady { version_id: VersionId, kind: PreviewKind },
    JobProgress { job_id: JobId, done: u64, total: u64 },
    JobFinished { job_id: JobId, result: JobResult },
    LibraryClosed,
}
```

* La souscription rend un `Receiver<Event>` (canal standard).
* Studio branche ce canal sur la boucle Slint ; la CLI le lit en séquence ; un script peut l'ignorer.
* Les événements sont des **notifications**, jamais des données complètes : le client re-requête ce dont il a besoin. Cela évite tout problème de cohérence entre le flux et la base.

## 3.3 Threading

* `Library` est `Send + Sync` et se clone à coût nul (`Arc` interne).
* Le moteur gère un pool de rendu dimensionné sur la machine.
* Les écritures catalogue sont sérialisées en interne ; le client n'a aucune contrainte d'ordre à respecter.

---

# 4. Types fondamentaux

```rust
pub struct AssetId(i64);
pub struct VersionId(i64);
pub struct RevisionId(i64);
pub struct CollectionId(i64);
pub struct KeywordId(i64);
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
    DecodeFailed { asset: AssetId, reason: String },
    InvalidSettings(String),
    NewerSettings { schema: u32, process: u32 },
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

---

# 6. Import

```rust
pub struct ImportOptions {
    pub copy_files: bool,      // copier dans Photos/ ou référencer sur place
    pub recursive: bool,
}

impl Library {
    pub fn import(&self, source: &Path, options: ImportOptions) -> Result<JobId>;
}
```

L'import est un travail : extraction EXIF, checksum BLAKE3, création de la révision initiale et de la version `Default` (catalogue §18), génération des miniatures — le tout en flux, avec `JobProgress` par lot.

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
}
```

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

impl Library {
    pub fn preview(&self, version: VersionId, kind: PreviewKind) -> Result<Preview>;
}
```

Le client affiche toujours quelque chose immédiatement (`Ready` ou `Stale`), puis se met à jour sur `PreviewReady`. La validité suit strictement le catalogue §20 (`revision_id` de tête).

**État transitoire (jobs/événements différés)** : tant que le modèle à `JobId` n'est pas livré, la surface synchrone tient lieu de contrat :

```rust
impl Library {
    /// Get-or-generate synchrone : rend la preview si rien de valide en cache.
    pub fn preview(&mut self, asset: AssetId, kind: PreviewKind) -> Result<PreviewFile>;
    /// Lecture seule du cache : `None` si rien de valide, ne rend jamais.
    pub fn cached_preview(&self, asset: AssetId, kind: PreviewKind) -> Result<Option<PreviewFile>>;
}
```

`cached_preview` permet au client le même motif que `Ready`/`Generating` : afficher immédiatement ce qui existe, planifier lui-même la génération du reste (Studio remplit sa grille ainsi).

---

# 12. Export

```rust
pub struct ExportRequest {
    pub versions: Vec<VersionId>,
    pub preset: ExportPresetId,
    pub destination: PathBuf,
}

impl Library {
    pub fn export(&self, request: ExportRequest) -> Result<JobId>;
    pub fn export_presets(&self) -> Result<Vec<ExportPreset>>;
}
```

L'export rend chaque version à sa révision de tête, avec sa process version (`pipeline.md` §3.3), et journalise dans `export_history`.

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
