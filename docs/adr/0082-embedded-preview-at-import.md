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

### 1. La vignette vient du fichier, quel que soit le demandeur

La décision porte sur **le chemin d'aperçu**, pas sur la passe d'import. C'est
`preview()` lui-même qui, pour la classe vignette, se sert de ce que le fichier
porte déjà au lieu de développer une révision :

* un RAW ou un DNG donne son imagette embarquée ;
* un JPEG, PNG ou TIFF se donne lui-même, décodé puis réduit ;
* un fichier dont l'imagette est absente, illisible, ou **plus petite que la
  classe demandée**, retombe sur le rendu d'aujourd'hui. Une vignette floue
  agrandie serait pire qu'une vignette lente ; `scaled_to_fit` n'agrandit
  jamais, et ce cas doit rester un rendu plutôt qu'une image dégradée.

Le placer là plutôt que dans `generate_import_thumbnails` est le point de
toute la décision, et c'est une correction : la première rédaction de cet ADR
le mettait dans la passe d'import, ce qui aurait laissé le **chemin paresseux**
— celui qui remplit la grille pendant qu'on la parcourt — payer 680 ms par
cellule visible. Un écran de cent vignettes aurait mis plus d'une minute, et
la passe d'import serait devenue le seul moyen d'avoir une grille utilisable :
exactement l'inverse du but.

Comme aujourd'hui, produire une vignette est **au mieux** : celle qu'on ne peut
pas produire ne fait jamais échouer un import.

**Une seule classe de taille**, la vignette. Les autres restent développées à la
demande — un aperçu de loupe est un vrai rendu, et c'est ce qu'on veut y voir.
Le grief est le temps d'import, pas le nombre de tailles disponibles, et la
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

Une cellule qui n'a pas encore son image **n'est pas vide** : elle porte déjà
son nom de fichier, sa note, son étiquette et ses pastilles — dont le `RAW+J`
d'[ADR 0079](0079-raw-jpeg-pairing.md) §6, dessiné qu'il y ait une vignette ou
non. Seul le rectangle de l'image manque, et le combler par un aplat neutre est
un détail de Studio, pas une décision de moteur.

La conséquence est celle qu'on veut : **on paie le rendu de ce qu'on regarde**,
pas de ce qu'on importe. Parcourir un dossier de cent photos rend cent
vignettes ; importer quinze mille n'en rend aucune.

### 4. L'import rend la main dès que le catalogue est écrit

Remplir le catalogue et remplir le cache sont deux travaux, et seul le premier
est l'import. `Library::import` ne bloque donc plus sur les vignettes : il rend
son rapport quand les assets existent — **~20 ms par fichier** — et la passe
part comme un travail de fond (§3.1 de `engine-api.md`), qui émet ses
`PreviewReady` comme n'importe quel rendu.

Et cette passe est **parallèle**. Le commentaire de `generate_import_thumbnails`
donnait une raison exacte de ne pas la paralléliser : `preview()` sérialise sur
le cache de décodage et le cache d'étages, deux mutex tenus pendant tout le
rendu — le verrou du catalogue, lui, est déjà relâché entre-temps
([ADR 0023](0023-catalog-lock-narrowing-preview.md)). Une vignette tirée de
l'imagette embarquée ne touche **ni l'un ni l'autre** : pas de décodage
capteur, pas d'étage. L'objection tombe avec la cause.

Mesuré sur 16 CR2, page cache chaud, 16 cœurs : **128 ms par fichier en série,
24 ms avec `rayon` — ×5,3**. Le facteur n'est pas le nombre de cœurs, la
lecture des fichiers et la bande passante mémoire des décodages 17,9 Mpx
faisant leur part.

`ImportOptions` gagne malgré tout `thumbnails: bool`, à `true` par défaut :
c'est exactement le drapeau que `ScanOptions` porte déjà, pour la raison
qu'énonce [ADR 0065](0065-selective-import.md) §2 — *« `thumbnails: false`
existe pour l'appelant qui n'affiche rien (la CLI) »*. Ce n'est pas une
préférence au sens d'[ADR 0078](0078-preferences-panel.md) §1 : cela porte sur
un import donné, pas sur l'installation.

**La passe suit l'ordre de la grille, pas celui de l'import.** Sa première
seconde doit aller aux photos qu'on verra en premier, et l'ordre de l'import
n'a aucune raison d'être celui-là — importer une carte de photos anciennes les
range au fond d'une grille triée par date de prise de vue. Demander au
catalogue les premières lignes de la requête par défaut coûte **0,46 ms**
depuis [ADR 0081](0081-grid-page-cost.md) : l'ordre est gratuit, le prendre est
donc obligatoire.

Cela dit, **la première page n'est déjà pas le problème**, et c'est ce qui
autorise la passe à être un simple travail de fond. Le client la préchauffe
tout seul, et mieux que le moteur ne saurait le faire : `AssetsAdded` déclenche
`reload`, qui appelle `load_window`, qui partitionne les vignettes manquantes
**lignes visibles d'abord** et remplit la file que `dispatch_thumbnails` vide
par trois. Studio connaît son filtre, son tri et son dossier ; le moteur ne les
connaît pas. Un écran de cent cellules se remplit ainsi en ~4 s à 122 ms par
vignette et trois travaux en vol, contre ~23 s aujourd'hui.

Une conséquence à connaître sans la trancher ici : `MAX_PREVIEW_JOBS = 3` a été
dimensionné contre un chemin qui tenait les mutex du cache de décodage et du
cache d'étages. §1 les libère, et cette borne mérite d'être reprise — c'est un
réglage de Studio, mesurable une fois le reste en place.

Ce qui reste vrai dans tous les cas, et qui est le vrai filet : **rien de tout
cela n'est nécessaire pour que la grille soit utilisable**. §1 vaut pour le
minuteur comme pour la passe. Un préchauffage annulé, une bibliothèque
importée avec `thumbnails: false`, un cache effacé à la main — dans les trois
cas la grille se remplit en la parcourant, à 122 ms par cellule et trois
travaux en vol.

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

Sur les 15 000 CR2 du corpus, la passe passe de **2 h 50 à 31 min** en série et
à **6 min** en parallèle (×5,3 mesuré). Mais c'est la ligne du dessous qui
compte le plus, puisque §4 la sort du chemin de l'import :

| 15 000 CR2 | aujourd'hui | après |
|---|---|---|
| avant que le catalogue soit utilisable | 2 h 50 | **~5 min** |
| avant que la grille soit entièrement chaude | 2 h 50 | ~6 min de plus, en fond |
| pour parcourir une grille jamais préchauffée | — | 122 ms par cellule visible |

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

* **Paralléliser la passe telle quelle**, sans changer la source des pixels.
  C'est ce que le commentaire du code proposait, et cela ne s'attaque pas au bon
  terme : seize cœurs sur un décodage capteur, c'est encore une vingtaine de
  minutes passées à démosaïquer pour produire des images de 256 px. Et c'est
  précisément dans cet ordre-là que la parallélisation était bloquée — par les
  mutex du cache de décodage et du cache d'étages. Changer la source d'abord
  (§1) supprime l'obstacle et rend le gain intéressant : §4 parallélise donc,
  mais seulement parce que §1 la précède.
* **Produire toutes les vignettes avant de rendre la main.** C'est le
  comportement d'aujourd'hui, et §4 le refuse : remplir le catalogue et remplir
  le cache sont deux travaux, et faire attendre le premier sur le second n'a
  jamais servi personne. La grille reste utilisable sans préchauffage — c'est
  §1 qui l'assure, à 122 ms par cellule visible, et non la passe.
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
