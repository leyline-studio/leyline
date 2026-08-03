# ADR 0065 — Choisir ce qu'on importe, en le voyant

**Statut :** Accepté — 2026-08

## Contexte

L'import de Leyline prend un dossier et prend **tout** ce qu'il y a dedans.
`Library::import` énumère la source, filtre sur l'extension, et écrit un asset
par fichier accepté (`crates/leyline-engine/src/import.rs`). Il n'existe aucun
moyen — ni dans le moteur, ni dans la CLI, ni dans Studio — de dire « ceux-là
oui, ceux-là non ».

Sur une carte de 800 déclenchements dont on garde quarante, cela veut dire
écrire 800 lignes de catalogue, 800 vignettes, puis retirer 760 photos une à
une. Le tri se fait donc *après* l'écriture, alors que le photographe l'a déjà
fait dans sa tête *avant* — il sait, en regardant les imagettes, lesquelles il
veut.

C'est aussi le dernier endroit où Leyline demande de faire confiance sans
montrer. Partout ailleurs, une opération de masse s'annonce : l'export dit ce
qu'il va écrire, la suppression demande confirmation, la grille montre avant
qu'on classe. L'import, lui, avale un dossier.

Un verrou technique explique en partie l'attente : **la vignette d'un fichier
pas encore catalogué n'existe nulle part**. Le cache d'aperçus est indexé par
`asset_id` (`docs/catalog.md` §21), et un candidat n'en a pas. Développer le
fichier pour le voir serait absurde — il n'a pas de révision, et un décodage
complet par fichier coûterait plus cher que l'import qu'on cherche à éviter.
Or les boîtiers écrivent déjà une imagette JPEG dans chaque RAW, et LibRaw sait
la sortir (`libraw_unpack_thumb`) — fonction présente dans la bibliothèque
liée, jamais exposée par `leyline-raw`.

## Décision

**Scanner et importer deviennent deux opérations distinctes.** Le scan
regarde et décrit ; l'import écrit, et reçoit une liste explicite de fichiers.

### 1. `scan_import` — ce qu'il y a, sans rien écrire

```rust
pub struct ScanOptions {
    pub recursive: bool,
    /// Extraire l'imagette de chaque candidat (voir §2).
    pub thumbnails: bool,
}

pub struct ImportCandidate {
    pub path: PathBuf,
    pub filename: String,
    pub media_type: MediaType,
    pub file_size: u64,
    pub capture_date: Option<i64>,
    pub camera: Option<String>,
    /// Déjà dans la bibliothèque, très probablement (§3).
    pub already_imported: bool,
    /// JPEG, arête maximale 256 px, orientation appliquée ; `None` si le
    /// fichier n'en porte pas ou si `ScanOptions::thumbnails` était faux.
    pub thumbnail: Option<Vec<u8>>,
}
```

Le scan **n'écrit rien** : ni asset, ni fichier copié, ni entrée de cache. Il
lit des en-têtes. C'est ce qui autorise à le lancer sur une carte qu'on
hésite encore à importer.

Il énumère exactement ce que `import` aurait retenu — même parcours, même
filtre d'extension, même tri par chemin. Deux listes qui divergeraient
seraient pires que pas de liste du tout : le code d'énumération est donc
partagé, pas recopié.

### 2. L'imagette vient du fichier, jamais du pipeline

Pour un RAW, c'est **l'imagette que le boîtier a écrite** : `libraw_unpack_thumb`
la sort telle quelle. Pour un JPEG/PNG/TIFF, c'est l'image elle-même. Dans les
deux cas, elle est réduite à 256 px d'arête et réencodée en JPEG, avec
l'orientation appliquée — une planche-contact de photos couchées ne sert à
rien.

Pas de rendu par le pipeline de développement : il n'y a pas de révision à
rendre, et le but de tout l'exercice est justement de ne pas payer un décodage
par fichier avant de savoir lesquels on garde.

Les imagettes sont **portées par le scan**, pas demandées ensuite une par une.
Le fichier est déjà ouvert, son en-tête déjà lu ; y ajouter l'extraction évite
une seconde traversée et un aller-retour asynchrone par ligne. `thumbnails:
false` existe pour l'appelant qui n'affiche rien (la CLI), et le scan est alors
purement métadonnées.

Elles ne sont **pas mises en cache sur disque** : le cache est indexé par asset,
ces fichiers n'en ont pas, et une imagette de candidat ne survit pas à la
décision qu'elle sert à prendre.

### 3. Les doublons sont montrés, et la vérité reste l'empreinte

Un candidat est marqué `already_imported` quand le catalogue contient déjà un
asset de **même nom et même taille**. C'est une comparaison d'index, sans
lecture du contenu.

Le vrai refus reste celui de l'import : l'empreinte BLAKE3 du contenu
(`import.rs`), qui est la seule réponse exacte. Marquer au scan par empreinte
imposerait de lire intégralement chaque fichier — 20 Go pour une carte pleine,
avant même que l'utilisateur ait choisi quoi que ce soit. Le marquage est donc
un **indice fiable en pratique et jamais autoritaire**, et il est nommé pour ce
qu'il est. Un candidat marqué reste listé et reste cochable : c'est
précisément le geste dont ADR 0043 §5 avait besoin (réimporter après un
effondrement d'historique).

### 4. `import_files` — importer une liste, pas un dossier

```rust
pub fn import_files(&self, source: &Path, files: &[PathBuf],
                    options: &ImportOptions) -> Result<ImportReport>;
```

Même pipeline par fichier que `import`, même `ImportReport`, mêmes règles de
copie. `source` reste nécessaire : c'est lui qui donne le chemin relatif sous
`Photos/` quand `copy_files` est vrai, et il est donc la racine sous laquelle
les fichiers doivent se trouver — un fichier hors de `source` est **refusé**,
pas rangé au hasard.

`import(source)` **ne change pas**. C'est ce qu'utilisent la CLI, le dossier
surveillé (ADR 0039) et le tethering (ADR 0038), et transformer « importe ce
dossier » en cérémonie à deux temps serait une régression pour les trois.
`import` devient un appel à `import_files` avec tout ce que le scan a trouvé.

### 5. Les trois clients

* **Moteur** : `scan_import_async` s'ajoute aux travaux de fond, avec
  `JobResult::Scan`. Un scan de carte lit des centaines d'en-têtes ; le faire
  sur le fil d'interface figerait la fenêtre, ce que ADR 0033 interdit déjà
  pour l'import lui-même.
* **CLI** : `leyline scan <library> <source> [--flat]` liste les candidats,
  doublons marqués, et `import` gagne `--only <nom>` (répétable) pour importer
  une sélection sans interface. Sans cette option, la CLI verrait la liste sans
  pouvoir s'en servir.
* **Studio** : le dialogue d'import montre la planche-contact des candidats,
  cases à cocher, doublons décochés d'office, avec « tout / rien ». Le bouton
  d'import n'importe que ce qui est coché.

## Conséquences

* `leyline-raw` expose enfin une imagette (`thumbnail`), avec sa propre
  extraction — c'est la seule addition à la surface FFI.
* Le dialogue d'import de Studio devient une vue à part entière : il garde une
  liste en mémoire le temps de la décision, et la relâche à la fermeture.
* Un scan puis un import se paient deux traversées du dossier. C'est le prix de
  la décision qu'on prend entre les deux, et il est payé une fois par carte,
  pas par photo.
* `docs/engine-api.md` §6 décrit le scan à côté de l'import.

## Alternatives écartées

* **Importer puis trier dans la grille** : c'est l'état actuel, et c'est ce que
  la décision refuse — 760 assets écrits pour 40 gardés, et un catalogue qui
  porte la trace de ce qu'on n'a jamais voulu.
* **Une vignette par appel asynchrone, à l'affichage de chaque ligne** : plus
  paresseux, et plus cher ici — le fichier serait rouvert une seconde fois par
  ligne, et une liste qui se remplit case par case pendant qu'on la parcourt
  est plus difficile à lire qu'une liste complète deux secondes plus tard.
* **Détecter les doublons par empreinte au scan** : exact, et il faut lire
  toute la carte pour l'obtenir (§3).
* **Un filtre par motif (`*.CR2`, plage de dates) plutôt qu'une sélection** :
  utile un jour, mais ce n'est pas la question posée — on choisit en
  *regardant*, pas en décrivant ce qu'on cherche. Rien n'empêche d'ajouter un
  filtre au-dessus de la liste plus tard.
