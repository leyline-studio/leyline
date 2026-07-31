# ADR 0056 — Métadonnées EXIF des fichiers non-RAW à l'import

**Statut :** Accepté — 2026-08

## Contexte

À l'import, Leyline ne remplit la table `metadata` que lorsque LibRaw a su
lire le fichier (`leyline-engine/src/import.rs`, `exif_metadata(&raw)`). Un
JPEG, un TIFF ou un PNG importés — ce que fait tout le monde en arrivant avec
une bibliothèque existante, et ce que produit d'ailleurs l'export de Leyline
lui-même — n'ont donc **aucune métadonnée du tout** dans le catalogue :

* le panneau de détails affiche `—` pour le boîtier, l'objectif et
  l'exposition ;
* la bande de prise de vue de develop ([ADR 0054](0054-first-run-and-basic-mode.md) §2)
  ne s'affiche pas ;
* **la date de capture est vide**, donc le tri par défaut (par date de prise
  de vue) place ces photos ensemble, en fin de liste, quelle que soit la date
  réelle ;
* la recherche plein texte ne les trouve ni par boîtier ni par objectif.

Ce n'est pas une position tenable : le fichier contient l'information, et
Leyline ne la lit pas. Un dérawtiseur peut refuser de *développer* un JPEG ;
il ne peut pas prétendre ne pas savoir quand il a été pris.

## Décision

### 1. Un lecteur EXIF pour les fichiers que LibRaw ne lit pas

L'import lit les EXIF des fichiers non-RAW et remplit la même table
`metadata`, avec les mêmes champs, que le chemin RAW. **La source dépend du
fichier, pas la destination** : rien ne change dans le catalogue, dans les
requêtes, ni dans ce que l'interface affiche.

La règle de préséance est sans ambiguïté : **si LibRaw a lu le fichier, c'est
LibRaw qui fait foi**, et le lecteur EXIF ne tourne pas. C'est l'identification
du décodeur qui rend la photo, elle doit rester la même que celle affichée.
Le lecteur EXIF n'intervient donc que là où il n'y a rien aujourd'hui.

### 2. `kamadak-exif`, dans le moteur, à côté du lecteur XMP

La dépendance est `kamadak-exif` : Rust pur, sans `unsafe`, lecture seule,
sous licence compatible avec la GPL-3.0 du projet, et couvrant les conteneurs
qui nous concernent (JPEG, TIFF, PNG, WebP, HEIF).

Elle est consommée depuis un module `leyline-engine/src/exif.rs`, jumeau de
`xmp.rs` : même forme, même statut. Ni nouveau crate — il n'y a pas de
responsabilité nouvelle, seulement une seconde source pour une donnée déjà
modélisée — ni ajout à `leyline-raw`, dont le rôle est LibRaw et rien d'autre.

### 3. Ce qui est lu

Les champs que `Metadata` porte déjà et rien de plus : boîtier (marque,
modèle), objectif, sensibilité, vitesse, ouverture, focale, correction
d'exposition, flash, mode de balance des blancs, espace de couleur,
orientation, position GPS, auteur, copyright — plus la **date de capture**,
qui n'est pas dans `Metadata` mais dans l'asset lui-même, et qui est la
donnée la plus visible des cinq premières lignes de ce document.

Un champ absent du fichier reste absent du catalogue. **Aucune valeur n'est
inventée, aucune valeur par défaut n'est écrite.**

### 4. L'heure, et la seule chose qu'on en sait

`DateTimeOriginal` est une heure locale sans fuseau. Quand
`OffsetTimeOriginal` (EXIF 2.31) est présent, il est appliqué : l'instant
stocké est le vrai instant UTC, et le décalage est conservé dans
`capture_offset_minutes`, colonne qui existe pour exactement cela. Quand il est
absent, l'heure est prise telle quelle et le décalage reste inconnu — la même
convention que le chemin RAW, qui fait déjà ce choix. Une heure locale
enregistrée comme telle et signalée comme inconnue est honnête ; une heure
locale décalée d'un fuseau deviné ne l'est pas.

### 5. Meilleur effort, jamais bloquant

Un EXIF absent, tronqué ou aberrant **n'échoue pas l'import** : la photo entre
au catalogue sans métadonnées, exactement comme aujourd'hui. C'est la règle que
suivent déjà la vignette et le sidecar XMP au même endroit
([ADR 0047](0047-xmp-sidecar-read.md)) — l'import d'un fichier ne se joue pas
sur un bloc de métadonnées.

### 6. Lecture seule, définitivement

Leyline ne réécrit jamais les EXIF d'un fichier source. C'est le principe
non destructif (`docs/vision.md`), et ce n'est pas négociable ici : la seule
écriture de métadonnées du projet reste l'export, qui produit un fichier
nouveau.

## Hors périmètre

* **Appliquer l'orientation EXIF au rendu** d'un JPEG affiché. L'orientation
  est désormais *lue* et stockée ; qu'elle soit *appliquée* par le pipeline
  d'affichage est un autre sujet, avec sa propre question de version d'étage.
* **Les métadonnées propriétaires** (MakerNotes) : modes de prise de vue,
  points AF, corrections d'objectif du constructeur. Chaque marque a son
  format, et rien n'en dépend chez nous.
* **Une resynchronisation des photos déjà importées.** Les JPEG importés avant
  cette décision restent sans métadonnées jusqu'à un réimport ; leur relire les
  EXIF a posteriori suppose de décider ce qui gagne en cas de conflit avec ce
  que l'utilisateur a saisi entre-temps, ce qui est une décision de
  synchronisation, comme celle qu'ADR 0047 a explicitement refusé de prendre.
* **L'écriture d'EXIF**, dans le fichier source comme dans un sidecar (§6).

## Conséquences

* **Un JPEG importé a enfin une date, un boîtier et une exposition** : il se
  trie avec les autres, se cherche comme les autres, et affiche la bande de
  prise de vue de develop.
* **Une dépendance de plus** dans `leyline-engine`, en lecture seule et sans
  `unsafe` (`docs/architecture.md` §Briques externes est mis à jour).
* **`docs/catalog.md` gagne une phrase** : `capture_offset_minutes` a
  désormais un producteur.
* **Aucun changement de schéma, de rendu, de pipeline ni de version d'étage.**
  Un fichier importé avant et après cette décision se *développe* identiquement.

## Alternatives écartées

* **Faire lire les JPEG par LibRaw.** LibRaw ouvre certains fichiers non-RAW,
  mais son identification y est partielle et son coût est celui d'un décodeur
  RAW complet pour lire quatre nombres.
* **Écrire notre propre lecteur EXIF.** Le format est un marécage de cas
  particuliers par constructeur ; c'est exactement le genre de brique qu'ADR
  0037 a accepté d'écrire à la main *parce qu'elle était minuscule et
  cadrée* (quelques tags DCP), ce qui n'est pas le cas ici.
* **Lire les EXIF à l'affichage plutôt qu'à l'import.** Le catalogue existe
  pour ne pas rouvrir 60 000 fichiers à chaque tri.
* **Remplacer LibRaw par le lecteur EXIF partout**, pour n'avoir qu'un chemin.
  L'identification du décodeur qui rend l'image est celle qui doit s'afficher
  à côté d'elle (§1).
