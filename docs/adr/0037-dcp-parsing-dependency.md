# ADR 0037 — Dépendance de parsing DCP : un lecteur de tags maison minimal au-dessus du crate `tiff` déjà lié, pas de nouvelle dépendance

**Statut :** Accepté — 2026-07

## Contexte

ADR 0035 a tranché le placement pipeline, le crate propriétaire, la source des
profils et la reproductibilité du profil caméra DCP — mais a laissé
**explicitement ouverte** une seule question, renvoyée « à la PR
d'implémentation » : la **dépendance de parsing DCP**. Elle a posé que Leyline
écrive « un **parseur DCP maison minimal** (les seuls tags nécessaires à
l'application) ou **intègre un crate Rust existant** (s'il en existe un
convenable et licenciable au moment venu) », sans trancher, « cela dépend de ce
qui est disponible et licenciable à ce moment-là ».

Ce document résout ce seul point laissé blanc par ADR 0035. Il ne rouvre
**rien** d'autre : ni le placement `leyline-color` (ADR 0035), ni la source des
profils, ni la reproductibilité par checksum BLAKE3, ni la process version. Il
remplit une case, il ne re-litige pas l'ADR précédent.

Faits vérifiés dans le dépôt avant décision :

* Le crate **`tiff` est déjà une dépendance de l'espace de travail**, pinné
  `tiff = "0.10"` (`Cargo.toml`, résolu à `0.10.3` dans `Cargo.lock`), consommé
  par `leyline-export` (`crates/leyline-export/Cargo.toml : tiff.workspace =
  true`). Il est utilisé aujourd'hui pour l'**écriture** TIFF, avec embarquement
  du profil ICC (`crates/leyline-export/src/lib.rs` :
  `tiff::encoder::TiffEncoder`, `tiff::tags::Tag::IccProfile`) — l'adoption
  décidée par ADR 0015. Sa licence a donc **déjà passé la barre** de vérification
  du projet quand il a été adopté pour l'export.

## Décision

### Écrire un lecteur de tags DCP maison minimal au-dessus du crate `tiff` déjà lié

Leyline **écrit un lecteur de tags DCP maison minimal** en s'appuyant sur les
primitives de lecture d'IFD/tags du crate `tiff` **déjà lié** — plutôt que de
dépendre d'un nouveau crate externe DCP-spécifique non vérifié, et plutôt que
d'écrire un lecteur TIFF/IFD de zéro. Trois raisons :

**1. DCP est un conteneur fondé sur les tags TIFF/EP — comme le DNG lui-même.**
ADR 0035 l'a déjà établi : « DCP est le format d'Adobe, fondé sur les **tags
TIFF/EP**, **pas** de l'ICC ». Leyline lie **déjà** `tiff` pour l'**écriture**
TIFF (embarquement ICC, ADR 0015). Réutiliser ses primitives de lecture
d'IFD/tags pour un chemin de **lecture** DCP est une **extension naturelle
d'une dépendance déjà vérifiée et déjà liée**, pas une surface de dépendance
inédite. C'est le même conteneur, lu au lieu d'être écrit.

**2. L'ensemble de tags réellement nécessaire est petit et intégralement
documenté.** Le rendu DCP n'a besoin que d'un sous-ensemble borné de familles
de tags, toutes publiées dans la **DNG Specification d'Adobe** — un document
**publiquement disponible et librement distribué**, pas un format
rétro-ingénieré ou non documenté :

* les **matrices colorimétriques** (color matrices) et **matrices de
  calibration** (calibration matrices),
* les **illuminants de calibration** (les deux illuminants de référence),
* les **matrices *forward*** (forward matrices, espace de connexion),
* la **courbe tonale du profil** (profile tone curve),
* les **tables de déformation teinte/saturation** (hue/sat map, les tables 3D
  HSV du profil),
* la **look table** (table de rendu esthétique du profil).

*(Les familles de tags sont nommées au niveau de détail que la DNG
Specification garantit ; les identifiants numériques exacts de chaque tag sont
laissés à la PR, qui les lira dans la spec plutôt que de les inventer ici —
même prudence de non-invention de constantes que le reste de cette série d'ADR.)*

C'est une situation **matériellement différente** d'un format propriétaire non
documenté : écrire un lecteur minimal contre une spec publiée est une tâche
**bornée et bien cadrée**, pas un effort de rétro-ingénierie ouvert.

**3. L'histoire de dépendance/licence reste propre.** Aucun nouveau crate
externe à vérifier : la contrainte de licence de dépendance d'ADR 0009 (code
sous GPL-3.0, dépendances compatibles) n'est pas re-sollicitée, puisque la
licence de `tiff` **a déjà franchi la barre** quand il a été adopté pour
l'export (ADR 0015). Réutiliser un crate déjà vérifié évite d'introduire — et de
devoir re-vérifier — une dépendance DCP-spécifique tierce.

### Portée du parseur — lecture seule, le petit ensemble de tags de rendu

Le parseur est **en lecture seule** et se limite au petit ensemble de tags
nécessaires au rendu (ci-dessus). **Aucun support d'*authoring*/écriture DCP** :
la V2 ne fait que **lire** des fichiers `.dcp` fournis par l'utilisateur
(ADR 0035), elle n'en écrit jamais. Cette portée étroite est énoncée
explicitement : elle **garde le parseur petit** et **évite tout glissement de
périmètre** vers une boîte à outils DCP générale (édition, ré-encodage,
conversion) que rien dans la V2 ne réclame.

### La correctness colorimétrique n'est PAS résolue par cet ADR

Cet ADR résout **comment des octets deviennent des données structurées** — le
container. Il **ne résout pas** si les matrices/tables ainsi lues sont
**appliquées correctement** au sens colorimétrique. Ce sont **deux risques
distincts** : parser le conteneur correctement, et appliquer sa science des
couleurs correctement.

L'exigence de validation d'ADR 0035 **reste inchangée et n'est en rien
affaiblie** : le chemin d'application DCP doit être **validé contre de vrais
fichiers `.dcp` générés par Adobe et leurs rendus de référence avant toute
sortie** — la même barre qu'ADR 0016 (« non triviaux à valider sans images de
référence sous la main »). Cet ADR ne touche **que** le premier risque (le
parsing du conteneur, documenté et à faible risque) ; le second (la correctness
de la math colorimétrique face au rendu d'Adobe) demeure exactement le risque
qu'ADR 0035 a nommé, et cet ADR ne prétend pas le refermer.

### Placement — inchangé, dans `leyline-color`

Le parseur et sa logique d'interprétation des tags vivent dans
**`leyline-color`**, cohérent avec le placement décidé par ADR 0035 (le parseur
DCP et l'application de ses matrices/tables y vivent déjà). Cet ADR ne rouvre
pas ce choix ; il remplit le seul détail qu'ADR 0035 avait laissé blanc.

## Conséquences

* **La question de dépendance ouverte par ADR 0035 est refermée** : lecteur de
  tags maison minimal au-dessus de `tiff` (déjà lié, déjà vérifié), pas de
  nouveau crate DCP-spécifique, pas de lecteur TIFF/IFD de zéro.
* **Zéro nouvelle dépendance à vérifier** : la contrainte de licence d'ADR 0009
  n'est pas re-sollicitée, la licence de `tiff` ayant déjà franchi la barre pour
  l'export (ADR 0015). L'histoire de dépendance reste propre.
* **La portée reste petite et gelée** : lecture seule, sous-ensemble de tags de
  rendu, aucun *authoring*. Pas de glissement vers une boîte à outils DCP
  générale.
* **La correctness colorimétrique reste un risque ouvert, inchangé depuis
  ADR 0035** : cet ADR résout le parsing du conteneur (documenté, borné, faible
  risque), **pas** la fidélité de la math couleur au rendu d'Adobe. La validation
  contre de vrais DCP Adobe et leurs rendus de référence avant sortie (barre
  d'ADR 0016) demeure exigée telle quelle.
* **`leyline-color` reste le foyer** du parseur et de l'application DCP
  (ADR 0035), aux côtés du chemin de transformation ICC de sortie (ADR 0027) —
  ce document ne déplace rien.
* **Ne préjuge pas d'un futur système de plugins.** `docs/roadmap.md` liste
  « Plugins, SDK stable » en Long terme, hors V2. Le lecteur DCP maison décidé
  ici est un choix d'implémentation interne (où vit le code, aujourd'hui) —
  il ne ferme pas la porte à un futur mécanisme d'extension (un plugin
  fournissant un autre parseur de profil, un format supplémentaire) qui
  s'ajouterait par-dessus, le jour où `docs/roadmap.md` aborde ce chantier.
  Rien ici n'engage la forme de ce futur système ; ce n'est simplement pas ce
  que cet ADR ferme.

## Alternatives écartées

* **Dépendre d'un crate externe DCP-spécifique.** Écarté : à ce jour, aucun
  crate DCP-spécifique établi n'est connu ayant la maturité et le niveau
  d'adoption des autres dépendances du projet (LibRaw, Lensfun, LittleCMS via
  `lcms2`, `tiff`) — sans prétendre avoir mené un relevé exhaustif de
  l'écosystème, aucun candidat de ce calibre ne s'impose. Introduire un tel
  crate imposerait une nouvelle vérification de licence (ADR 0009) et une
  nouvelle surface de dépendance non éprouvée, pour parser un conteneur dont le
  sous-ensemble utile est petit et documenté — coût disproportionné face au
  gain. Réutiliser `tiff`, déjà lié et déjà vérifié, évite les deux.
* **Écrire un lecteur TIFF/IFD de zéro** plutôt que de réutiliser le crate
  `tiff` déjà lié. Écarté : réécrirait exactement les primitives de lecture
  d'IFD/tags que `tiff` fournit déjà et que `leyline-export` emploie déjà pour
  l'écriture TIFF (ADR 0015). C'est du travail redondant sur un composant
  (parsing d'IFD/tags) où une dépendance vérifiée existe déjà dans l'arbre —
  aucune raison de le refaire à la main. Le lecteur DCP maison se limite à la
  **couche d'interprétation des tags DCP**, au-dessus de la lecture d'IFD que
  `tiff` porte déjà.
* **Traiter le DCP comme un format entièrement non documenté exigeant une
  prudence de rétro-ingénierie équivalente au report du vignettage/TCA d'ADR
  0016.** Écarté — et il faut distinguer explicitement : le **format conteneur**
  de DCP est **documenté** (tags TIFF/EP, DNG Specification publiée d'Adobe) et
  **à faible risque à parser**, ce n'est pas un format rétro-ingénieré. Seule la
  **correctness colorimétrique de son application** porte le risque de niveau
  ADR 0016 — et ce risque-là n'est **pas** ce que cet ADR résout : il reste
  ouvert et validé contre de vrais rendus Adobe, exactement comme ADR 0035
  l'exige. Confondre les deux mènerait à sur-cadrer le parsing (traiter une
  tâche bornée comme un effort ouvert) tout en sous-estimant qu'il faut encore
  valider la math couleur séparément. Les deux risques sont distincts ; cet ADR
  n'en referme qu'un.
