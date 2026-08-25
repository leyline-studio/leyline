# Configuration requise

**Document :** `docs/system-requirements.md`
**Version :** 1.0
**Statut :** Référence

---

# 1. Objectif

Ce document répond à : **sur quelle machine Leyline tourne-t-il, et à partir de quel plancher ?**

Toutes les valeurs qui suivent sont **mesurées**, jamais estimées à partir d'un ordre de grandeur plausible. Les conditions de mesure sont en §2.1 et la procédure pour les refaire en §7 : un chiffre de ce document qui ne se reproduit plus est un chiffre à corriger, pas à conserver.

Hors sujet ici : les prérequis de **compilation** (chaîne Rust épinglée, LibRaw, Lensfun, LittleCMS, nasm), qui sont dans [`contributing.md`](contributing.md), et le contenu des installateurs, qui est dans [ADR 0019](adr/0019-distribution-i18n.md).

---

# 2. Le facteur limitant est la mémoire

Ce n'est pas le processeur. Le pipeline d'**une** photo ne sait pas remplir une machine moderne — un lot de 12 fichiers 30 Mpx mesurait 280 % de 1 600 % sur seize threads ([ADR 0068](adr/0068-concurrent-export-batch.md)) — mais chaque photo en vol immobilise plusieurs centaines de mégaoctets de tampons flottants. Ajouter des cœurs accélère peu ; manquer de mémoire fait paginer, et la pagination coûte bien plus que tout ce que la concurrence rapporte.

D'où la règle de dimensionnement :

```
RAM ≈ 300 Mo (Studio et son catalogue)
    + concurrence × (Mpx / 30) × 700 Mo
```

où `concurrence` est le nombre de photos qu'un lot d'export traite à la fois ([§3.1](#31-la-concurrence-dexport-suit-la-machine)).

## 2.1 Les mesures

Conditions : build `--release` du 2026-08-25, Intel i9-9900K (8 cœurs / 16 threads), 32 Gio disponibles, Linux. Corpus : trois CR2 de Canon 5D IV (30 Mpx, ~65 Mo pièce) et 199 JPEG de provenances variées.

| Action | Pic mémoire | Temps |
|---|---|---|
| Studio ouvert sur une bibliothèque, au repos | **178 Mo** | — |
| Import de 3 RAW (EXIF + miniatures) | 313 Mo | 3,2 s |
| Import de 199 JPEG | 133 Mo | 8,7 s |
| Aperçu `small` — 1024 px, la vue develop | 220 Mo | 1,3 s |
| Aperçu `large` | 665 Mo | 5,5 s |
| Aperçu `full` — pleine résolution | 990 Mo | 11,5 s |
| Export de 3 JPEG, `--concurrency 1` | 760 Mo | 7,1 s |
| Export de 3 JPEG, `--concurrency 2` | 1,26 Go | 5,0 s |
| Export de 3 JPEG, `--concurrency 4` | **1,80 Go** | 3,1 s |

Deux lectures de ce tableau :

* **Le pic d'un export est proportionnel à la concurrence**, linéairement : ~700 Mo par photo en vol à 30 Mpx, au-delà du gigaoctet à 45 Mpx. C'est la seule ligne qui puisse mettre une machine à genoux.
* **Un aperçu pleine résolution coûte presque autant qu'un export**, et c'est un geste que l'utilisateur fait sans y penser. Une machine dimensionnée au plus juste doit pouvoir absorber ce gigaoctet-là.

Le rendu interactif, lui, ne pèse rien : un curseur déplacé sur une photo déjà ouverte redessine en 11 à 15 ms depuis le proxy en cache ([ADR 0074](adr/0074-live-preview-while-dragging.md), [ADR 0076](adr/0076-proxy-cache.md)), et les caches qui le permettent sont bornés — 64 Mo pour les proxies, ~144 Mo pour les décodages.

---

# 3. Les configurations

| | **Minimale** | **Recommandée** |
|---|---|---|
| Processeur | x86-64 de base, 2 cœurs | 4 cœurs / 8 threads |
| Mémoire | **4 Go** | **16 Go** |
| Affichage | 1024 × 700 | 1920 × 1080 |
| Graphique | OpenGL ES 2.0, ou Mesa llvmpipe | GPU matériel |
| Disque | 100 Mo + le cache (§6) | SSD |

**Aucun jeu d'instructions récent n'est exigé.** Le projet ne fixe ni `target-cpu` ni `target-feature` : les binaires sont compilés pour le x86-64 de base (SSE2). Un processeur sans AVX2 fait tourner Leyline. Sur macOS, la cible est arm64.

**Le GPU ne calcule aucun pixel.** Il ne sert qu'à dessiner l'interface. La question d'un pipeline GPU a été posée deux fois et écartée deux fois : par [ADR 0012](adr/0012-rayon-data-parallelism.md) (déterminisme inter-GPU non garanti), puis à nouveau le 2026-08-03 ([`competitive-plan.md`](competitive-plan.md) §B3), une fois l'interaction rendue fluide côté CPU et le GPU interdit à l'export par la promesse de [`pipeline.md`](pipeline.md) §5.1. Une carte graphique plus puissante n'accélère donc **rien** du développement.

## 3.1 La concurrence d'export suit la machine

Le degré par défaut est `min(4, available_parallelism())` ([ADR 0068](adr/0068-concurrent-export-batch.md) §1) : une machine à deux cœurs traite deux photos à la fois, pas quatre. C'est la disposition qui rend les 4 Go de la colonne « minimale » tenables — deux photos 30 Mpx en vol demandent ~1,3 Go, quatre en demandent 1,8 à 2,4.

Sur une machine au plancher **et** un corpus au-delà de 30 Mpx, descendre explicitement à `--concurrency 1` reste le bon réflexe : un lot séquentiel est lent, un lot qui pagine l'est bien davantage.

À l'inverse, une machine large gagne à monter : 6 photos en vol prennent 3,4× contre 2,8×, pour ~3,3 Go de pointe.

---

# 4. Systèmes d'exploitation

| Plateforme | Plancher | Format livré |
|---|---|---|
| Linux | glibc **2.35** — Ubuntu 22.04, Debian 12 ou plus récent | AppImage |
| Windows | Windows 10 | installateur NSIS |
| macOS | arm64 | `.dmg` |

Le plancher Linux n'est pas une décision d'architecture : c'est la glibc de la machine qui a compilé l'AppImage. Le construire sur une distribution plus ancienne l'abaisse d'autant, sans rien changer au code.

---

# 5. Affichage et carte graphique

Studio déclare `min-width: 1024px` et `min-height: 700px` ; sa fenêtre s'ouvre par défaut aux deux tiers de l'écran détecté, jamais en dessous de ces bornes. Un 1024 × 768 fonctionne, un 1280 × 800 est confortable.

Le rendu de l'interface passe par Slint et son moteur femtovg, qui demande **OpenGL ES 2.0**. Sans GPU matériel, le chemin correct est le pilote logiciel OpenGL de Mesa :

```bash
LIBGL_ALWAYS_SOFTWARE=1 leyline-studio
```

**Ce qui n'est pas le chemin correct : `SLINT_BACKEND=winit-software`.** Le renderer logiciel de Slint démarre sans erreur et ne dessine **aucun élément `Path`** : les surimpressions de masque et les contours d'histogramme disparaissent, en silence, sans que rien ne signale que l'affichage est incomplet. Un défaut invisible est pire qu'un refus de démarrer ; ce backend ne doit pas être présenté comme un repli.

---

# 6. Disque

## 6.1 L'installation

L'AppImage pèse 26 Mio et l'installateur Windows 30 Mo, fond de carte mondial compris ([ADR 0059](adr/0059-bundled-world-basemap.md) — 9 Mo de tuiles z0–5, embarquées pour que la vue carte n'émette jamais une requête réseau). À titre de comparaison, darktable en pèse 108.

## 6.2 La bibliothèque

Les photos dominent tout le reste. Ce que Leyline ajoute par-dessus, mesuré :

| Poste | Coût | Pour 15 000 photos |
|---|---|---|
| Catalogue SQLite | 1,7 Ko/photo (+ 264 Ko de schéma) | ~26 Mo |
| Miniatures | 86 Kio/photo | ~1,3 Go |
| Aperçus d'une photo **retouchée** | ~2 Mo | proportionnel aux seules photos éditées |

Un aperçu 1024 px pèse 0,68 Mo, un 2048 px 2,47 Mo, un 4096 px 9,03 Mo. Le cache d'aperçus est borné par une fenêtre de trois révisions plus les têtes ([ADR 0075](adr/0075-preview-cache-retention.md)), donc il croît avec le nombre de photos **effectivement retouchées**, pas avec la taille de la bibliothèque.

Un aperçu `full` fait exception : il est gardé tel quel et pèse plusieurs dizaines de mégaoctets par photo. En générer sur des milliers de photos est le seul geste qui fasse exploser le cache.

## 6.3 Référencer au lieu de copier

`leyline import --reference` inscrit les fichiers sans les recopier dans `Photos/`, ce qui évite de doubler l'occupation disque. La contrainte : **les fichiers doivent déjà se trouver sous la racine de la bibliothèque**, puisque le catalogue ne stocke que des chemins relatifs à cette racine ([ADR 0010](adr/0010-relative-paths.md)). Un fichier situé ailleurs est ignoré, avec la raison `file is outside the library root`.

---

# 7. Refaire les mesures

Chaque chiffre de §2.1 se reproduit avec les binaires du dépôt, sans harnais particulier :

```bash
# Pic mémoire et temps de n'importe quelle commande
/usr/bin/time -f "%M Kio  %e s  %P CPU" leyline-cli export <lib> <dest> 1 2 3 --concurrency 4

# Studio au repos, sans écran physique
Xvfb :77 -screen 0 1920x1080x24 &
DISPLAY=:77 leyline-studio <lib> &
grep VmHWM /proc/$!/status

# Croissance du catalogue et des miniatures
ls -l <lib>/catalog.db && du -sb <lib>/Cache/thumbnails
```

La mesure faite sous `Xvfb` passe par llvmpipe : c'est donc aussi le chiffre d'une machine sans GPU matériel.

---

# 8. Documents liés

* [`architecture.md`](architecture.md) — les crates et les briques externes dont dépendent ces prérequis.
* [`contributing.md`](contributing.md) — les prérequis de compilation, distincts de ceux d'exécution.
* [ADR 0068](adr/0068-concurrent-export-batch.md) — la mesure d'origine du coût mémoire d'une photo en vol.
* [ADR 0075](adr/0075-preview-cache-retention.md), [ADR 0076](adr/0076-proxy-cache.md) — ce qui borne les caches.
* [ADR 0019](adr/0019-distribution-i18n.md) — les installateurs par plateforme.
