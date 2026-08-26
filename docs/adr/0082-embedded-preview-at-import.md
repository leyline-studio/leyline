# ADR 0082 — L'import montre l'imagette du boîtier, il ne la rend pas

**Statut :** Accepté — 2026-08

## Contexte

`Library::import` rend une vignette par fichier importé, en série, à travers
le pipeline complet — décodage capteur compris. Mesuré le 2026-08-26,
`--release`, sur de vrais CR2 : importer 10 fichiers prend **6,95 s**, dont
**0,1 s** d'import réel.

| Poste, par fichier | Coût |
|---|---|
| BLAKE3 sur le fichier | 5 ms |
| Copie sous `Photos/` | 4 ms |
| LibRaw `identify` | 0,4 ms |
| `add_asset` (les 5 écritures) | 0,2 ms |
| Tout le reste d'`import_one` | ~10 ms |
| **`generate_import_thumbnails`** | **~680 ms** |

Sur le corpus de test réel — 15 000 CR2 — cela fait **2 h 50, dont cinq
minutes d'import**. Le reste est un décodage capteur par fichier, payé pour
produire une image de 256 px.

Le commentaire de la fonction énonçait déjà le verdict : *« Left serial;
worth revisiting if import-time thumbnailing shows up in the perf benches. »*
C'est fait, et ce n'est pas la sérialisation le sujet : c'est le décodage.

### La décision est déjà prise, une étape plus tôt

[ADR 0065](0065-selective-import.md) §2 s'appelle « L'imagette vient du fichier,
jamais du pipeline », et sa justification est mot pour mot celle-ci : *« le but
de tout l'exercice est justement de ne pas payer un décodage par fichier avant
de savoir lesquels on garde »*. Le scan, qui précède l'import, a donc raison
depuis 2026 ; l'import, qui le suit, paie exactement ce que le scan refusait.

ADR 0065 §2 donnait une raison précise de ne pas garder ces imagettes : *« le
cache est indexé par asset, ces fichiers n'en ont pas »*. Après l'import,
l'asset existe. La raison a disparu, la décision peut traverser.

### Ce que la mesure impose à la conception

Sept CR2, deux boîtiers (60D et 5D Mark IV), cinq dossiers du corpus :

* **L'imagette embarquée est un JPEG pleine taille** — 5184×3456 dans les sept
  cas, 1,3 à 3,2 Mo. Elle n'est jamais le facteur limitant d'une classe de
  taille ; sur les fichiers enregistrés en mRAW elle est même **plus grande que
  ce que le RAW décode** (5184×3456 contre 3888×2592).
* **L'extraire ne coûte presque rien, la décoder coûte le reste.**
  `leyline_raw::thumbnail` rend les octets en **41 ms** de moyenne ; le décodage
  JPEG de ces 17,9 Mpx en prend **76** ; la réduction à 256 px et l'écriture
  PNG, **5**. Total **122 ms**, contre **680** aujourd'hui.
* **Toute la ladder ne vaut pas son prix.** Produire les quatre classes
  (256, 1024, 2048, 4096) du même décodage JPEG coûte 276 ms et **16,9 Mo de
  cache par photo** — 250 Go sur une bibliothèque de 15 000 images. Une seule
  classe en coûte 53 à 98 ko.
* **L'orientation est un piège déjà désamorcé.** LibRaw n'applique aucune
  rotation à l'imagette embarquée, contrairement à `decode` ; et cette imagette
  porte *parfois* sa propre balise EXIF d'orientation, parfois non.
  `scan.rs::embedded_preview` traite déjà les deux cas — c'est cette fonction
  qui sert, pas une seconde écriture du même raisonnement.

## Décision

### 1. L'import écrit l'imagette du fichier, il ne développe rien

`generate_import_thumbnails` cesse d'appeler `preview()`. Pour chaque fichier
importé, elle produit la vignette 256 px à partir de ce que le fichier porte
déjà :

* un RAW ou un DNG donne son imagette embarquée ;
* un JPEG, PNG ou TIFF se donne lui-même, décodé puis réduit ;
* un fichier dont l'imagette est absente, illisible, ou **plus petite que la
  classe demandée**, retombe sur le rendu d'aujourd'hui. Une vignette floue
  agrandie serait pire qu'une vignette lente ; `scaled_to_fit` n'agrandit
  jamais, et ce cas doit rester un rendu plutôt qu'une image dégradée.

Comme aujourd'hui, la passe est **au mieux** : une vignette qu'on ne peut pas
produire ne fait jamais échouer un import.

**Une seule classe de taille**, la vignette. Les autres restent à la demande :
le grief est le temps d'import, pas le nombre de tailles disponibles, et la
ladder complète coûterait 250 Go sur le corpus.

### 2. Le catalogue dit d'où viennent les pixels

Une imagette de boîtier **n'est pas le rendu d'une révision**. L'y faire passer
serait un mensonge que tout le reste croirait : `valid_preview` (`catalog.md`
§20) répond « voici l'aperçu de la révision de tête », et un aperçu qui n'a
jamais traversé le pipeline s'y installerait pour toujours — aucun rendu ne
viendrait jamais le remplacer, et un `undo` revenant sur cette révision
ressortirait le JPEG du boîtier en croyant montrer un développement.

`previews` gagne donc une colonne (migration 6) :

```sql
origin INTEGER NOT NULL DEFAULT 0   -- 0 : rendu par le pipeline
                                    -- 1 : l'imagette que le fichier portait
```

Elle est portée par la ligne, pas déduite d'une convention de chemin, pour la
raison habituelle : une convention se relit de deux façons.

* `valid_preview` gagne `AND origin = 0`. La question qu'elle pose — « la tête
  a-t-elle son rendu ? » — garde exactement la réponse d'avant.
* Une imagette embarquée s'ancre sur la **révision initiale**, la seule qui
  existe à l'import ; la clé `UNIQUE(asset_id, revision_id, kind)` et la clé
  étrangère sont satisfaites sans invention.
* La fenêtre de rétention d'[ADR 0075](0075-preview-cache-retention.md) ne la
  voit pas : elle appartient au **fichier**, pas à une révision, donc elle ne
  vieillit pas avec l'historique et ne s'évince pas avec lui. Elle meurt avec
  l'asset, par la cascade. `retain_previews` et `remove_revision_previews`
  excluent `origin = 1`.

### 3. Ce qu'un client affiche, et ce qu'il en sait

`Library::cached_preview` rend désormais **la meilleure image affichable**, et
dit si elle est définitive : un rendu valide d'abord, à défaut l'imagette
embarquée, à défaut rien.

Studio affiche ce qu'on lui donne et — quand ce n'est pas définitif — met la
cellule dans la file que son minuteur de vignettes vide déjà. Le mécanisme
existe : `load_window` construit sa liste `missing` et rend en priorité les
lignes visibles. Il ne change pas de nature, seulement de critère.

La conséquence est celle qu'on veut : **on paie le rendu de ce qu'on regarde**,
pas de ce qu'on importe. Parcourir un dossier de cent photos rend cent
vignettes ; importer quinze mille n'en rend aucune.

### 4. Un import que personne ne regardera n'a pas de cache à préchauffer

`ImportOptions` gagne `thumbnails: bool`, à `true` par défaut. C'est
exactement le drapeau que `ScanOptions` porte déjà, pour exactement la même
raison, énoncée par [ADR 0065](0065-selective-import.md) §2 : *« `thumbnails: false`
existe pour l'appelant qui n'affiche rien (la CLI) »*. Un import en lot
tombe alors à ~20 ms par fichier.

Ce n'est pas une préférence au sens d'[ADR 0078](0078-preferences-panel.md) §1 :
cela porte sur un import donné, pas sur l'installation, et n'a rien à faire
survivre à un relancement.

### 5. Un compagnon n'a pas de vignette à produire

Un boîtier réglé en RAW+JPEG écrit deux fichiers, et l'import en enregistre
deux assets. [ADR 0079](0079-raw-jpeg-pairing.md) §5 sort le compagnon de la
grille par une clause, et le volet de détails ne montre de lui que **son nom**
(§6) — aucun écran de Studio n'affiche la vignette d'un compagnon.
`generate_import_thumbnails` en produit une quand même, pour chaque fichier
importé : sur un dossier réglé ainsi, **la moitié de la passe est jetée**.

La passe saute donc les assets dont `companion_of` n'est pas nul. C'est une
décision **neutre par construction** : une bibliothèque sans paires ne saute
rien, et §1 la porte entièrement. Personne n'y perd, un cas fréquent y gagne
un facteur deux.

Deux suites en découlent, toutes deux couvertes par le chemin paresseux de §3 :

* **Dépairer rend le JPEG à la grille** ([ADR 0079](0079-raw-jpeg-pairing.md)
  §6). Il n'a alors pas de vignette, et la file du minuteur la produit comme
  pour toute cellule visible qui n'en a pas.
* **La passe d'appairage explicite** sur une bibliothèque existante
  ([ADR 0079](0079-raw-jpeg-pairing.md) §7) laisse en place les vignettes déjà
  produites. Elles deviennent inutiles sans devenir fausses ; les effacer
  rendrait un dépairage lent pour récupérer quelques dizaines de kilo-octets.

Et **le compagnon n'est pas non plus une meilleure source** pour la vignette du
maître, ce qu'on pouvait croire — il est sur le disque, pleine taille, déjà en
JPEG. Mesuré sur six paires réelles, page cache chaud :

| Source de la vignette du RAW | Coût par prise |
|---|---|
| l'imagette embarquée dans le CR2 | **119 ms** |
| le fichier JPEG compagnon | 161 ms |

Le compagnon est un encodage de **meilleure qualité** que l'imagette embarquée
— 5,2 à 10,6 Mo contre 1,3 à 3,2 — donc plus long à décoder (111 à 159 ms
contre 69 à 93), pour la même image de 5184×3456. Le lire coûte moins cher que
d'ouvrir le RAW, et cela ne rattrape pas l'écart. §1 s'applique donc au maître
sans exception, et la paire ne change qu'une chose : le compagnon ne coûte
rien du tout.

### 6. Ce que l'utilisateur voit, et qu'il faut dire

Une vignette de boîtier n'est pas un rendu neutre de Leyline : elle porte le
contraste, la saturation et la balance que le fabricant applique. La grille
montrera donc, sur une photo jamais développée et jamais regardée en loupe, le
rendu Canon — puis le rendu Leyline dès qu'elle aura été visitée. **La couleur
changera sous les yeux de l'utilisateur**, une fois, sans qu'il ait rien
demandé.

C'est le prix, il est assumé, et c'est celui que Lightroom fait payer sous le
nom « Embedded & Sidecar ». Le refuser coûterait 2 h 50 sur quinze mille
fichiers, et l'immense majorité de ces vignettes ne sera jamais regardée de
près.

## Conséquences

Par fichier, sur les sept CR2 mesurés :

| | aujourd'hui | après | rapport |
|---|---|---|---|
| vignette d'import | ~680 ms | **122 ms** | ×5,6 |
| import complet (une photo) | ~700 ms | **~142 ms** | ×4,9 |
| import complet, `thumbnails: false` | ~700 ms | **~20 ms** | ×35 |
| cache écrit par photo | 53–98 ko | 53–98 ko | inchangé |

Sur les 15 000 CR2 du corpus : **2 h 50 → 31 min**, et **5 min** pour un import
sans vignettes.

Pour un boîtier réglé en RAW+JPEG, §5 s'ajoute. Coûts par fichier mesurés
aujourd'hui — 700 ms pour un CR2, **248 ms pour un JPEG** (pas de décodage
capteur) — reportés sur un dossier réel du corpus, `2022_04_17`, qui tient
29 paires :

| 29 paires (58 fichiers) | aujourd'hui | après |
|---|---|---|
| fichiers traités par la passe | 58 | **29** |
| durée de l'import | ~27,5 s | **~4,6 s** |

Le facteur global y est de **~6**, dont un facteur deux vient de §5 seul.

Le décodage JPEG (76 ms) devient le poste dominant de la passe, à 62 % de son
temps. C'est un décodage pleine résolution — 17,9 Mpx — pour produire 256 px.

## Alternatives écartées

* **Paralléliser la passe telle quelle.** C'est ce que le commentaire du code
  proposait, et cela ne s'attaque pas au bon terme : huit cœurs sur un décodage
  capteur, c'est encore 21 min de décodage pour des images de 256 px. La
  parallélisation reste possible **après** celle-ci, sur une passe déjà cinq
  fois moins chère. Elle est d'ailleurs bloquée aujourd'hui par le verrou du
  catalogue, tenu pendant tout le décodage de chaque `preview()`, et par un
  cache de décodage qui est un petit LRU non conçu pour des rendus concurrents.
* **Ne rien produire du tout à l'import.** Le plus rapide, et il laisse une
  grille vide au premier lancement — l'écran qui suit un import de quinze mille
  photos est précisément celui où il faut montrer quelque chose. Reste
  accessible par §4 à qui le veut.
* **Produire les quatre classes du même décodage JPEG.** Séduisant (un seul
  décodage sert tout), mesuré, et rejeté sur le chiffre : 250 Go de cache sur le
  corpus, pour des tailles que personne n'a demandées.
* **Stocker l'imagette sous une révision fictive, ou par une convention de
  chemin.** Les deux évitent la colonne de §2 et les deux font dire au
  catalogue quelque chose de faux. La clé étrangère de `previews` refuse la
  première ; la seconde encode un fait dans un nom de fichier, où il se déduit
  au lieu de se lire — [ADR 0047](0047-xmp-sidecar-read.md) a montré ce que
  coûte une convention de nommage dont on n'est pas seul maître.
* **Prendre la vignette du maître dans son JPEG compagnon**, quand il y en a un
  — il est déjà sur le disque, pleine taille, et déjà en JPEG. Mesuré sur six
  paires réelles : **161 ms contre 119**. Le compagnon est un encodage de
  meilleure qualité que l'imagette embarquée (5,2 à 10,6 Mo contre 1,3 à 3,2),
  donc plus long à décoder pour exactement la même image. L'idée coûterait en
  plus un chemin de code qui ne servirait qu'aux paires.
* **Garder l'imagette pour toujours, sans jamais rendre.** La grille mentirait
  durablement sur ce que les réglages produisent, et le premier retour de la
  loupe vers la grille montrerait deux images différentes de la même photo.

## Ce que cet ADR ne fait pas

Aucun pixel du pipeline ne bouge : ni étage, ni version d'étage, ni révision.
`pipeline.md` §5.1 est hors de cause — une imagette embarquée n'est pas un
rendu, et c'est tout l'objet de §2 que de l'écrire dans le catalogue plutôt que
de le laisser deviner.

Il ne touche pas non plus au **décodage JPEG pleine résolution** qui devient le
poste dominant. Un décodage à l'échelle (le DCT du JPEG permet 1/2, 1/4, 1/8 —
648×432 suffirait très largement pour 256 px) le réduirait encore beaucoup, et
`zune-jpeg`, le décodeur que la caisse `image` embarque ici, ne l'expose pas.
Cela demanderait un décodeur de plus dans l'arbre : une décision de dépendance,
non mesurée à ce jour, et qui n'a pas à être prise dans le même mouvement.
