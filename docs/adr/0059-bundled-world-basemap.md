# ADR 0059 — Fond de carte mondial embarqué

**Statut :** Accepté — 2026-08

## Contexte

[ADR 0040](0040-gps-map-view.md) a écarté l'idée de livrer un pack de tuiles
par défaut, au motif qu'« une seule région à résolution utile pèse des
centaines de Mo à plusieurs Go, incompatible avec un installateur léger ».
Le raisonnement était juste, mais il portait sur un pack **régional à
résolution utile**. Il n'a jamais pesé le cas d'un **fond mondial à zoom
faible**, qui est un objet d'une autre nature : on n'y situe pas une photo à
la rue près, on y voit les continents, les côtes et les reliefs, ce qui
suffit à donner un contexte à des punaises GPS.

Conséquence de cette absence : la vue Carte s'ouvre sur un écran vide tant
que l'utilisateur n'a pas trouvé, téléchargé et importé un `.mbtiles`. Une
fonctionnalité livrée que personne ne voit fonctionner au premier lancement.

Le poids a été mesuré avant de décider, et non estimé — tuiles Web Mercator
rendues depuis le raster Natural Earth I (21600 × 10800), par GDAL :

| Profondeur | Tuiles | JPEG q80 | PNG 32 bits | PNG 8 bits |
|---|---|---|---|---|
| z0–5 | 1 365 | **9,0 Mo** | 26,5 Mo | 73,0 Mo |
| z0–6 | 5 461 | 29,8 Mo | 90,7 Mo | 253,9 Mo |

À titre de comparaison, l'AppImage pèse 16 Mo et l'installateur Windows
22 Mo ; darktable, le comparable direct, en pèse 108.

## Décision

**Un fond de carte mondial z0–5 est embarqué dans Leyline Studio, en JPEG,
et sert dès qu'aucun pack utilisateur n'est actif.**

* **Données** : Natural Earth I avec relief ombré et eaux
  (`NE1_HR_LC_SR_W`), **domaine public** — pas de licence à propager, pas
  d'attribution juridiquement exigée, et surtout pas de redistribution
  interdite. Les tuiles rendues par le serveur public OpenStreetMap ne
  peuvent pas être embarquées : leur politique d'usage l'interdit. C'est ce
  qui écarte OSM comme source d'un pack *livré*, pas comme source d'un pack
  que l'utilisateur apporte.
* **Profondeur et format** : z0–5, tuiles JPEG qualité 80, 1 365 tuiles,
  9,0 Mo. Le z6 quadruplerait le poids pour un cran de détail dont la vue
  Carte n'a pas besoin, et la source 10m plafonne de toute façon vers z6.
* **Emplacement** : `assets/basemap/world-z0-5.mbtiles`, versionné dans le
  dépôt. Un binaire de 9 Mo dans Git est un coût assumé : le pack est
  immuable, il ne sera pas réédité à chaque version, et l'alternative
  (le régénérer au build) imposerait GDAL et 309 Mo de source à quiconque
  compile le projet.
* **Embarquement** : `include_bytes!`, et ouverture **sans extraction** via
  `sqlite3_deserialize` en lecture seule (`Connection::deserialize_bytes`,
  rusqlite 0.37). Pas de copie dans la bibliothèque de l'utilisateur, pas de
  fichier temporaire à nettoyer, pas de chemin d'écriture sur une
  bibliothèque ouverte en lecture seule. `leyline-map` gagne un
  `TilePack::from_static`, à côté de `TilePack::open` : même lecteur, même
  requêtes, seule la façon d'attacher la base change.
* **Portée** : derrière une *feature* Cargo `bundled-basemap` de
  `leyline-engine`, désactivée par défaut et réactivée par Leyline Studio —
  exactement le montage de la feature `tether`. La CLI et le SDK ne
  rendent pas de carte ([ADR 0040](0040-gps-map-view.md)) : ils n'ont aucune
  raison de porter 9 Mo de tuiles.
* **Priorité** : un pack importé par l'utilisateur gagne toujours. Le fond
  embarqué est un *repli*, jamais un mélange : Leyline ne compose pas deux
  packs dans une même vue, il en sert un.
* **Attribution** : le pack déclare la sienne dans sa table `metadata`
  (`Natural Earth (public domain)`), et l'interface affiche déjà celle du
  pack actif. Le repli codé en dur « © OpenStreetMap contributors » reste
  pour les packs qui ne déclarent rien — la plupart des packs apportés par
  un utilisateur sont dérivés d'OSM, où l'attribution est obligatoire — mais
  il ne s'applique plus au fond embarqué, qui serait alors crédité à tort.
  Aucune API nouvelle pour distinguer les deux packs : ce que l'interface a
  besoin de dire, la métadonnée `name` du pack le dit déjà.

## Conséquences

* L'installateur Windows passe d'environ 22 à 31 Mo, l'AppImage de 16 à
  25 Mo. On reste sous le quart de darktable.
* La vue Carte n'a plus d'état vide au premier lancement : elle montre le
  monde, et les punaises GPS dessus. Le bouton « Importer un pack… » ne
  disparaît pas pour autant — il devient ce qu'il aurait toujours dû être,
  le moyen d'**affiner**, pas le prérequis pour voir quoi que ce soit.
* `docs/specification.md` §1 : la ligne « Vue carte GPS » cesse de dire
  « tuiles fournies par l'utilisateur » sans nuance.
* [ADR 0040](0040-gps-map-view.md) voit son alternative « Bundler un pack de
  tuiles par défaut » corrigée sur place : elle reste écartée pour un pack
  régional, elle ne l'est plus pour un fond mondial. Cette réécriture est
  permise tant que le projet n'est pas publié (`docs/adr/README.md`).
* Regénérer le pack un jour demande GDAL et le raster Natural Earth ; la
  recette exacte est dans `assets/basemap/README.md`, pour que personne
  n'ait à la redécouvrir.

## Alternatives écartées

* **Télécharger le pack depuis l'application** (un menu « choisir sa
  région », le client va chercher les tuiles) : c'est la demande initiale,
  et elle est écartée pour cette tranche. Elle contredit frontalement
  « jamais d'appel réseau depuis Leyline lui-même » ([ADR 0040](0040-gps-map-view.md)),
  ce qui demanderait sa propre décision ; elle suppose d'héberger et de
  maintenir des packs, donc de la bande passante et une disponibilité que
  le projet ne s'engage pas encore à tenir ; et elle ne remplace pas le
  fond embarqué, qui est justement ce qui rend la carte utile **sans**
  réseau. Rien n'empêche de la reprendre plus tard, par-dessus.
* **Un choix de précision dans l'installateur** : impossible à tenir sur
  les trois plateformes. L'AppImage n'a aucune étape d'installation, c'est
  un fichier qu'on lance ; le `.dmg` est un glisser-déposer. Seul NSIS
  saurait poser une page de composants, au prix d'un template `.nsi`
  maison. Un réglage qui n'existe que sur un système sur trois n'est pas un
  réglage, c'est une asymétrie à expliquer.
* **z0–6 embarqué** (29,8 Mo) : trois fois le poids pour un niveau de zoom
  supplémentaire, alors que la vue Carte sert à situer des photos, pas à
  naviguer. Le pack utilisateur reste la réponse dès qu'on veut du détail.
* **PNG 8 bits** : le format « léger » évident se révèle huit fois plus
  lourd que le JPEG sur ce contenu (73 Mo contre 9), la quantification
  s'accommodant mal d'un dégradé de relief. Mesuré, pas supposé.
* **Extraire le pack embarqué dans un fichier au premier lancement** :
  ajouterait un cache à gérer, à invalider entre versions, et un chemin
  d'écriture là où il n'en faut aucun. `sqlite3_deserialize` rend
  l'extraction inutile.
