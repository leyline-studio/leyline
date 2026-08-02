# ADR 0040 — Vue carte GPS : tuiles MBTiles hors-ligne

**Statut :** Accepté — 2026-07

## Contexte

[[studio-workflow-gaps-progress]] laissait la vue carte GPS comme le seul
écart Lightroom/Darktable volontairement non tranché, faute de solution
compatible avec le principe Local-First (`docs/vision.md`) : pas de cloud,
pas de dépendance à un service tiers qui doit rester joignable.

Une carte a besoin de deux choses : des données géographiques (routes,
côtes, lieux) et un moyen de les afficher. Les solutions courantes
reposent presque toutes sur un serveur de tuiles interrogé en HTTP à
l'exécution (OpenStreetMap public, MapTiler, Mapbox...) — incompatible
avec Local-First telle quelle : Leyline ne doit jamais dépendre d'un
service réseau pour fonctionner.

## Décision

**Tuiles OpenStreetMap pré-téléchargées, au format MBTiles, fournies par
l'utilisateur — jamais d'appel réseau depuis Leyline lui-même.**

* **Données** : OpenStreetMap (licence ODbL, gratuite, attribution
  `© OpenStreetMap contributors` obligatoire, affichée sur la carte).
  L'utilisateur télécharge ou génère un pack de tuiles pour la région qui
  l'intéresse (ex. extraits [Geofabrik](https://download.geofabrik.de) +
  un rendeur comme `tilemaker`, ou un pack MBTiles déjà rendu) — **hors du
  périmètre de Leyline**, exactement le même traitement que les profils
  caméra DCP (ADR 0035) : un fichier que l'utilisateur apporte, jamais une
  dépendance réseau cachée dans le produit.
* **Format** : MBTiles — un fichier SQLite unique contenant les tuiles
  (`docs/catalog.md`-style : `tiles(zoom_level, tile_column, tile_row,
  tile_data)` + `metadata(name, value)`). `rusqlite` est déjà une
  dépendance du projet (`leyline-catalog`) ; lire un MBTiles est une
  poignée de requêtes SQL, aucune bibliothèque supplémentaire.
* **Nouveau crate `leyline-map`** : lecteur MBTiles seul, même
  responsabilité unique que `leyline-catalog`/`leyline-preview` — ouvre le
  fichier, sert une tuile `(z, x, y)` en bytes bruts (PNG/JPG selon le
  pack), lit les métadonnées (bornes, zoom min/max, attribution). Ne
  connaît rien du catalogue ni du rendu de carte — juste un accès aux
  tuiles, consommé par `leyline-engine`.
* **Emplacement du pack** : convention de fichier, pas une nouvelle colonne
  catalogue. `Library::import_map_pack(source)` copie le `.mbtiles` choisi
  vers `<root>/Map/pack.mbtiles`, au même rang que `Photos/`/`Cache/`
  /`Exports/`/`Backups/` (`docs/catalog.md` §3). Un seul pack actif à la
  fois en V1 — pas de table `library` à faire migrer, la présence du
  fichier fait foi.
* **Points GPS** : `docs/catalog.md` §13 (`metadata.gps_latitude`/
  `gps_longitude`/`gps_altitude`) existait déjà dans le schéma mais n'était
  jamais rempli — aucune extraction EXIF GPS n'existait. LibRaw expose les
  coordonnées déjà parsées (`other.parsed_gps`, degrés/minutes/secondes +
  référence N/S/E/W) : nouveaux accesseurs dans `leyline-raw/src/shim.c`,
  conversion DMS → degrés décimaux côté Rust (jamais côté C, même choix
  que le reste du shim : le C ne fait que lire les champs, toute la
  logique reste en Rust). Comme le reste des métadonnées EXIF
  aujourd'hui, l'extraction GPS ne couvre que les RAW identifiés par
  LibRaw — les JPEG/TIFF n'ont jamais eu de métadonnées caméra non plus,
  même limite préexistante, pas une régression introduite ici.
* **Rendu** : Slint n'a pas de canevas de tuiles ; le rendu est fait côté
  Rust, dans la même forme que le canevas de développement existant
  (mêmes réglages, même image composée en mémoire) — la fenêtre visible
  est composée en RGBA à partir des tuiles couvrant le viewport puis
  publiée comme `slint::Image`, le pan/zoom recompose l'image à chaque
  geste. Aucun rendu vectoriel : les tuiles MBTiles sont déjà des images
  matricielles pré-rendues.

## Conséquences

* `docs/specification.md` §Inclus gagne « Vue carte GPS (tuiles MBTiles
  hors-ligne fournies par l'utilisateur) ».
* Nouveau crate `leyline-map`, consommé par `leyline-engine`, même rang de
  dépendance que `leyline-catalog`/`leyline-preview` (`docs/architecture.md`).
* `docs/catalog.md` §13 : les colonnes GPS, présentes dans le schéma
  depuis l'origine mais jamais utilisées, sont désormais effectivement
  peuplées à l'import pour les fichiers RAW.
* Studio gagne une vue Carte ; **la CLI et le SDK n'exposent volontairement
  pas de rendu de carte** — c'est une surface visuelle, comme le canevas de
  développement, pas une opération scriptable. `Library::import_map_pack`/
  `map_pins`/`map_tile` restent accessibles au SDK pour un futur client,
  mais aucune commande CLI de rendu n'est ajoutée.
* Sans pack importé, la vue Carte affiche un état vide invitant à en
  importer un — jamais d'appel réseau de repli, jamais de carte
  placeholder qui donnerait l'illusion d'une connexion. *Amendé par
  [ADR 0059](0059-bundled-world-basemap.md)* : le repli est désormais un
  fond mondial embarqué, donc hors ligne comme le reste. L'état vide ne
  subsiste que dans une compilation sans la feature `bundled-basemap`.

## Alternatives écartées

* **Tuiles OSM via HTTP à l'exécution** (serveurs publics ou payants type
  MapTiler/Mapbox) : rejeté d'emblée — viole Local-First, et l'usage des
  serveurs de tuiles OSM publics est de toute façon soumis à une politique
  d'usage stricte incompatible avec un produit distribué.
* **MapLibre** (fork libre de Mapbox GL) : pas de binding Rust mûr
  compatible Slint ; `maplibre-rs` existe mais reste expérimental
  (WebGPU), gros risque d'intégration pour un gain de rendu (tuiles
  vectorielles stylées) que la V1 n'a pas besoin de payer — les tuiles
  matricielles MBTiles suffisent pour afficher des punaises sur une carte.
* **Bundler un pack de tuiles régional par défaut** : écarté — une région
  à résolution utile pèse des centaines de Mo à plusieurs Go, incompatible
  avec un installateur léger ; laisser l'utilisateur choisir sa propre
  région est aussi plus respectueux (pas de téléchargement imposé au
  premier lancement). **Cette formulation, d'origine, disait « un pack par
  défaut » sans qualifier sa résolution, et allait donc trop loin** : un
  fond *mondial* à zoom faible ne coûte que 9 Mo, et [ADR 0059](0059-bundled-world-basemap.md)
  l'a depuis embarqué. Ce qui reste écarté ici, et le reste, c'est de
  livrer du détail régional.
* **Stocker le chemin du pack dans le catalogue** (nouvelle colonne/table
  `library`) : écarté pour la V1 — une convention de fichier
  (`Map/pack.mbtiles`) suffit tant qu'un seul pack actif à la fois est
  supporté, évite toute migration de schéma pour ce ticket. À revisiter si
  le multi-pack (plusieurs régions actives) devient un besoin réel.
