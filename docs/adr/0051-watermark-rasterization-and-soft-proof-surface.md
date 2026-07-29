# ADR 0051 — Rasterisation du filigrane (`ab_glyph` + police embarquée) et surface d'épreuvage écran

**Statut :** Accepté — 2026-07

## Contexte

[ADR 0034](0034-softproofing-watermark-print.md) a tranché *quoi* : un
filigrane **texte** rangé dans `ExportSettings`, composité en toute dernière
étape avant l'encodage ; un épreuvage écran **vue seule**, paramètre optionnel
d'un appel de preview, jamais persisté. Le module d'impression, troisième tiers
de l'item, a depuis eu son propre [ADR 0036](0036-print-module.md).

Ce qu'ADR 0034 a explicitement laissé « à la PR » et qui se révèle être une
décision structurelle plutôt qu'un détail :

1. **avec quoi dessiner du texte.** Aucune brique du dépôt ne rastérise des
   glyphes. `leyline-export` ne dépend ni de Slint ni d'un moteur de texte, et
   il ne doit pas en dépendre : c'est un encodeur d'images sans interface ;
2. **quelle police**, et où elle vit. Une police système ferait dépendre le
   rendu d'un filigrane de la machine — donc du poste, donc de la
   reproductibilité — et contredirait le Local First : deux exports du même
   preset sur deux postes ne se ressembleraient pas ;
3. **par où passe l'épreuvage** côté API, ADR 0034 n'ayant donné qu'une esquisse
   de structure.

## Décision

### 1. `ab_glyph` pour la rasterisation, et rien de plus

`leyline-export` gagne une dépendance : **`ab_glyph`** — Rust pur, sans
dépendance système, déterministe, et déjà présente dans l'arbre de compilation
par les dépendances de Slint (donc pas un téléchargement de plus pour qui
construit Studio).

Elle fait exactement une chose : transformer un contour de glyphe en couverture
de pixels. La mise en page — largeur du texte, position d'ancrage, composition
alpha — reste du code Leyline, une trentaine de lignes, parce que c'est du
placement de rectangles et non de la typographie. Aucun moteur de mise en forme
(HarfBuzz, `rustybuzz`, `cosmic-text`) : un filigrane est une ligne de texte
sans ligature ni bidi à négocier.

### 2. Une police embarquée : DejaVu Sans

Le fichier `crates/leyline-export/assets/DejaVuSans.ttf` est **embarqué dans le
binaire** (`include_bytes!`), avec sa licence à côté.

* **Embarquée**, parce qu'un filigrane doit se dessiner à l'identique partout :
  une police système ferait du rendu une propriété du poste.
* **DejaVu Sans**, parce que sa licence (Bitstream Vera + DejaVu) est
  permissive, donc compatible avec le GPL-3.0-only du projet — ce que la
  Liberation installée sur la plupart des distributions, sous GPLv2 avec
  exception police, n'est pas.
* **Le coût est assumé** : 757 Ko dans chaque binaire. C'est le prix d'un
  filigrane identique sur deux machines, et le seul poste de dépense de cette
  décision.

`ExportSettings.watermark.font` est une énumération, aujourd'hui à une seule
valeur (`"sans"`). Une seconde fonte s'ajoutera comme une valeur de plus, sans
changer la forme du document.

### 3. Le filigrane est dessiné dans `encode`, sur une copie

ADR 0034 §Filigrane place le composite « immédiatement avant l'encodage ».
Concrètement, c'est `leyline_export::encode` qui l'applique, sur une **copie**
du tampon reçu : la fonction prend `&[u8]` et ne doit pas graver le filigrane
dans le tampon de l'appelant, qui est le rendu de la révision.

L'effet secondaire utile : tout chemin de sortie passant par `encode` — export
simple, lot, preset — l'obtient sans le savoir, et aucun ne peut l'oublier.

Unités, arrêtées ici parce qu'ADR 0034 n'en donnait qu'un exemple :

| Champ | Unité |
| :--- | :--- |
| `text` | la chaîne, non vide |
| `font` | `"sans"` |
| `size` | pourcentage de la **hauteur** de l'image, dans `(0, 50]` — un filigrane suit la taille de l'export, il ne se mesure pas en pixels |
| `color` | `"#RRGGBB"` |
| `opacity` | `[0, 1]` |
| `anchor` | `bottom-right` (défaut), `bottom-left`, `top-right`, `top-left`, `center` |

La marge entre le texte et le bord est la moitié de `size`, jamais réglable :
c'est du placement, pas une décision de l'utilisateur.

### 4. L'épreuvage est une méthode de bibliothèque qui rend une image, non un fichier

```rust
Library::preview_soft_proofed(asset, kind, &SoftProof) -> Result<Rgb8>
```

Trois propriétés, qui sont la traduction directe du « vue seule » d'ADR 0034 :

* elle rend **en mémoire** et n'écrit rien — ni cache de previews, ni catalogue.
  C'est exactement la forme de `preview_before` (comparaison avant/après), pour
  la même raison : un tampon d'affichage n'est pas un livrable ;
* le `SoftProof` (profil ICC de destination, intention, alerte de gamut) est un
  argument d'appel, jamais un champ de révision ni de preset ;
* la transformation réutilise la primitive ICC d'[ADR 0027](0027-color-management-beyond-srgb.md)
  (`leyline_color::OutputTransform`), comme ADR 0034 l'exigeait.

**L'alerte de gamut** utilise l'épreuvage de LittleCMS lui-même — transformation
de *proofing* avec `gamut check` et couleur d'alarme — et non un aller-retour
maison comparé à l'original : c'est la même bibliothèque qui décide ce qui est
hors gamut et qui le signale, donc une seule définition de « hors gamut » dans
le projet.

### 5. Ce qui n'entre pas

* **L'épreuvage dans la CLI.** Un épreuvage est un mode d'affichage ; une
  commande sans écran n'en a pas l'usage, et l'exposer inviterait à écrire son
  résultat dans un fichier — soit exactement l'export vers un profil de
  destination, qui est une autre fonction (ADR 0027).
* **Le filigrane image/logo**, coupé par ADR 0034 et pour la raison qu'il
  donnait : le problème de référence de ressource n'est pas tranché.
* **Le filigrane sur la sortie d'impression.** L'impression a son propre chemin
  de sortie (ADR 0036) et ses propres réglages ; y porter le filigrane est un
  changement à part, pas un effet de bord de celui-ci.
* **Une police par langue, ou le choix d'une police système.** §2.

## Conséquences

* **Deux des trois manques d'ADR 0034 passent de « décidé » à « livré »**, cinq
  mois après la décision, et le dernier (le logo) reste coupé pour la raison
  d'origine.
* **`leyline-export` gagne une dépendance et un actif binaire.** C'est la
  première police du dépôt et le premier `include_bytes!` d'un fichier de
  données ; `architecture.md` en tient la liste.
* **Tout chemin d'export hérite du filigrane** (§3), y compris les lots et les
  presets, sans ligne de câblage supplémentaire.
* **L'épreuvage ne peut pas polluer le cache** : il rend en mémoire, comme la
  comparaison avant/après. Un utilisateur qui épreuve puis exporte obtient un
  export non épreuvé, ce qui est le comportement correct — l'épreuvage montre ce
  que *donnerait* une destination, il ne la produit pas.
* **Une seule définition de « hors gamut »** dans le projet (§4), celle de
  LittleCMS.

## Alternatives écartées

* **Utiliser une police système.** Le filigrane deviendrait une propriété du
  poste : nom du photographe rendu en Helvetica ici, en Arial là, absent
  ailleurs. Inacceptable pour une décoration destinée à des fichiers publiés.
* **Écrire notre propre rastériseur de glyphes**, sur le modèle du lecteur DCP
  maison d'[ADR 0037](0037-dcp-parsing-dependency.md). Le parallèle ne tient
  pas : lire quelques tags TIFF est borné et vérifiable, rastériser des
  contours TrueType correctement (hinting, anti-aliasing, kerning) ne l'est pas,
  et le résultat serait visiblement moins bon pour un gain nul.
* **Une police bitmap maison**, sans dépendance ni actif. Un filigrane crénelé
  sur un export 6000 px : la fonction perdrait sa raison d'être.
* **Dessiner le filigrane dans le moteur plutôt que dans l'encodeur.** Il
  faudrait le faire dans chaque chemin de sortie, et un chemin oublié
  n'afficherait rien sans erreur. `encode` est le point de passage obligé.
* **Un aller-retour ICC maison pour l'alerte de gamut** (transformer, retransformer,
  comparer). Deux définitions de « hors gamut » dans le projet, dont une à nous,
  pour une information que LittleCMS donne déjà.
* **Persister l'épreuvage choisi dans la révision.** Écarté par ADR 0034 ; rien
  n'a changé.
