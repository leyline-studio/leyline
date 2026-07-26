# ADR 0015 — Gestion des couleurs V1 : pipeline et export figés en sRGB

**Statut :** Accepté — 2026-07
**Suite :** l'espace de travail *interne* qu'il fige est remplacé par
[ADR 0044](0044-linear-wide-gamut-working-space.md) (Rec. 2020 linéaire non
borné) ; ce qui concerne la **sortie** a d'abord été élargi par
[ADR 0027](0027-color-management-beyond-srgb.md).

## Contexte

`specification.md` inclut « Gestion des couleurs (LittleCMS) » et ADR 0005 a choisi LittleCMS pour `leyline-color`, sans jamais préciser ce que la V1 en fait concrètement. En pratique le pipeline est déjà entièrement sRGB de bout en bout et fonctionne : `leyline-raw` demande à LibRaw une sortie sRGB 8 bits (`crates/leyline-raw/src/lib.rs`), et `process1`/`process2` (`pipeline.md` §3.3, ADR 0013) appliquent la fonction de transfert sRGB au rendu tonal. Il manquait la pièce qui rend cette hypothèse vérifiable en dehors du code : aucun fichier exporté ne porte de profil ICC, donc un visualiseur géré en couleur n'a que la convention pour deviner l'espace des pixels.

## Décision

**V1 ne gère qu'un seul espace, du décodage à l'export : sRGB.** Pas de sélection d'espace de travail, pas de profil d'entrée par appareil, pas de conversion d'espace de sortie configurable — c'est le pipeline existant, documenté comme décision plutôt que comme hasard d'implémentation.

**`leyline-color` expose un profil, pas une bibliothèque de transformations.** `srgb_icc_profile()` génère le profil ICC sRGB canonique de LittleCMS (`lcms2::Profile::new_srgb`) une seule fois (`OnceLock`) et rend ses octets ICC bruts. Pas de `cmsTransform`, pas de gestion de profils appareil : ce que LittleCMS apporte en V1, c'est un profil de référence correct plutôt qu'un profil maison encodé en dur, rien de plus.

**`leyline-export` embarque ce profil dans les formats qui le supportent.** JPEG (segment APP2 `ICC_PROFILE`, via `jpeg-encoder`), PNG (bloc `iCCP`, via `png`) et TIFF (tag `ICCProfile` 34675, via `tiff`) le portent nativement. WebP (`image-webp`) et AVIF (`ravif`) n'exposent aucune API d'embarquement ICC dans leur version actuelle : ils sortent sans profil, ce qui est la convention acceptée du web pour ces formats (sRGB implicite).

**Liaison LittleCMS.** Le crate `lcms2` (MIT) embarque `lcms2-sys`, qui lie dynamiquement à `liblcms2` si `pkg-config` la trouve, et se rabat sinon sur une compilation vendue via `cc` — LittleCMS étant MIT (contrairement à LibRaw, ADR 0004), aucune des deux options ne crée d'obligation de linkage dynamique.

## Conséquences

* Le contrat implicite « tout est sRGB » devient un fait testé : `leyline-color` vérifie que le profil généré a un en-tête ICC valide et est déterministe ; `leyline-export` vérifie que les octets du profil se retrouvent bien dans les fichiers JPEG/PNG/TIFF encodés.
* Un visualiseur géré en couleur (navigateur, Preview macOS, etc.) affiche les JPEG/PNG/TIFF de Leyline correctement même sur un écran à gamut large, sans dépendre de la convention « pas de profil = sRGB ».
* Aucun changement au format des pixels ni à `process1`/`process2` : ADR 0012 (parallélisme, rendu bit-pour-bit) n'est pas engagé, ceci n'est qu'un embarquement de métadonnées à l'export.
* Un futur espace de travail plus large (ProPhoto, Adobe RGB en interne) resterait un changement structurant à part entière — cet ADR ne le prépare pas et ne l'exclut pas.

## Alternatives écartées

* **Profil ICC statique embarqué en binaire** : aurait évité la dépendance à LittleCMS pour ce seul usage, mais aurait réintroduit exactement ce que ADR 0005 a écarté (« profils maison ») et un fichier qu'il faut faire confiance sans pouvoir le régénérer ; générer via LittleCMS coûte une poignée de lignes et documente que la bibliothèque choisie sert à quelque chose dès la V1.
* **Attendre la correction d'objectif et livrer les deux features Lensfun/LittleCMS ensemble** : la correction d'objectif (`leyline-lens`) exige en plus des métadonnées objectif/boîtier à l'import (EXIF `LensModel`, non extraites aujourd'hui) et une nouvelle version de process (ADR 0012 interdit de changer l'ordre des opérations par échantillon d'un process existant) — une portée nettement plus large, à traiter dans un ADR séparé.
* **Transformation ICC complète (profil d'entrée appareil → sRGB via `cmsTransform`)** : LibRaw produit déjà du sRGB 8 bits directement ; ajouter une conversion ICC par-dessus doublerait un travail déjà fait sans bénéfice mesurable pour la V1.
