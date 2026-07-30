# ADR 0053 — LUT créative : fichiers `.cube` fournis par l'utilisateur, appliqués sur l'axe d'affichage, dosables

**Statut :** Accepté — 2026-07

## Contexte

Leyline sait re-colorer une photo par ses propres opérateurs (courbe, mélangeur
TSL, roues de color grading, [ADR 0030](0030-tone-curve.md),
[ADR 0031](0031-hsl-color-grading.md)). Il ne sait pas appliquer une **LUT 3D**,
c'est-à-dire un look distribué comme un fichier : simulations de film, rendus de
maison de production, conversions de log vers un rendu d'affichage.

Les trois concurrents libres le font (darktable *lut 3D*, RawTherapee *film
simulation*, ART), le format `.cube` d'Adobe/Iridas est l'échange de fait, et
des milliers de ces fichiers circulent — gratuits ou vendus. C'est le dernier des
cinq écarts fonctionnels relevés face à eux, et le moins coûteux à combler :
tout est déjà en place sauf la lecture du fichier et l'interpolation.

**Ce qui n'est pas en cause.** Les opérateurs de couleur existants, qui restent
la voie normale : une LUT n'est pas un réglage, c'est un look qu'on a choisi
ailleurs.

## Décision

### 1. Un fichier référencé comme un profil DCP, jamais copié dans la révision

Le modèle d'[ADR 0035](0035-camera-profile-dcp.md) s'applique mot pour mot, et
il a été conçu pour ce cas de figure :

```rust
pub struct Lut {
    pub enabled: bool,
    /// Chemin relatif à la bibliothèque, `Profiles/LUT/<nom>.cube`.
    pub path: String,
    /// `blake3:` + 64 hexadécimaux des octets importés.
    pub checksum: String,
    /// Dosage, curseur dans [0, 100]. 100 = la LUT telle quelle.
    pub strength: i32,
}
```

* le fichier est **importé** dans `Profiles/LUT/` par le moteur, jamais
  référencé là où l'utilisateur l'a trouvé — sans quoi la bibliothèque cesserait
  d'être déplaçable ([ADR 0010](0010-relative-paths.md)) ;
* la révision porte le **checksum** des octets importés, donc un remplacement
  silencieux du fichier est détectable ;
* un import qui écraserait un nom déjà pris est **refusé**, comme pour un `.dcp`.

Aucune table de catalogue, aucun mécanisme nouveau : c'est le même chemin, pour
la même raison.

### 2. Le dosage fait partie du réglage

`strength` n'est pas un ornement : une LUT de simulation de film est presque
toujours trop forte à 100 %, et tous les concurrents exposent ce curseur. Le
mélange est linéaire entre l'entrée et la sortie de la LUT, sur l'axe
d'affichage (§3) — le seul endroit où « 50 % de ce look » veut dire ce que
l'utilisateur voit.

### 3. Appliquée sur l'axe d'affichage, pas en lumière linéaire

Un `.cube` est écrit pour des valeurs **display-referred** dans `[0, 1]` :
son auteur l'a réglé en regardant une image, pas un tampon linéaire non borné.
L'appliquer sur nos valeurs linéaires donnerait un résultat qui n'a aucun
rapport avec ce que le fichier décrit.

L'étage encode donc chaque échantillon vers l'axe d'affichage
(`kernel::v1::display`, ADR 0044), applique la LUT, et **revient** en linéaire.
C'est le même raisonnement que les masques par plage
([ADR 0048](0048-range-masks.md) §3), et la même fonction.

Conséquence assumée : ce qui dépasse le blanc est **écrêté à 1 avant la LUT**,
puisque la LUT n'a pas de valeur définie au-delà. Une LUT est un look de sortie ;
la marge au-dessus du blanc est le domaine de `output_rendering`, qui vient
après.

### 4. Rang 160 : après le color grading, avant le détail

La LUT est la **dernière décision de couleur**, donc après la courbe, le
mélangeur TSL et le color grading — un look s'applique sur l'image étalonnée,
pas avant elle. Et **avant** le débruitage et l'accentuation : ceux-ci
travaillent sur des structures locales, qu'une LUT à fort contraste amplifierait
si elle passait après.

Rang 160, libre entre `color_grading` (150) et `local_adjustments` (170).

### 5. Interpolation trilinéaire, `.cube` 1D et 3D

* le lecteur accepte `LUT_3D_SIZE` (le cas courant) et `LUT_1D_SIZE`, les
  directives `DOMAIN_MIN`/`DOMAIN_MAX`, les commentaires `#` et les titres ;
* l'interpolation est **trilinéaire**. La tétraédrique est légèrement plus
  fidèle aux arêtes du cube et significativement plus longue à écrire ; elle sera
  une `v2` si une différence visible se présente, exactement comme n'importe
  quelle correction de rendu ;
* une taille hors de `[2, 128]`, une ligne mal formée, un fichier tronqué : une
  **erreur nommée**, jamais un rendu approximatif. Le lecteur vit dans
  `leyline-color`, à côté du lecteur DCP et pour la même raison
  ([ADR 0037](0037-dcp-parsing-dependency.md)) : lire un format de données
  tabulaires bien spécifié n'est pas un problème qui mérite une dépendance.

### 6. Hors périmètre

* **Les formats `.3dl`, `.look`, `.icc` de type link, et les HaldCLUT en PNG.**
  `.cube` couvre l'échange réel ; les autres s'ajouteront comme des variantes du
  même étage si le besoin apparaît, sans nouvelle décision de fond.
* **Les LUT livrées avec l'application.** Leyline n'embarque aucun look : ce
  serait un choix esthétique de l'éditeur, et le projet n'en fait pas
  (`docs/vision.md`). L'utilisateur apporte les siennes.
* **L'interpolation tétraédrique** (§5).
* **Une LUT par masque local.** `LocalAdjustmentValues` re-paramètre des
  curseurs, pas des références de fichier (ADR 0029) ; l'y faire entrer est une
  autre décision.

## Conséquences

* **Le dernier des cinq écarts face aux concurrents libres se ferme.** Les
  fichiers que l'utilisateur possède déjà fonctionnent.
* **Aucun mécanisme nouveau** : import de ressource et référence checksummée
  d'ADR 0035, axe d'affichage d'ADR 0048 §3, étage versionné d'ADR 0042.
* **Un nouvel étage neutre par défaut**, donc aucune révision existante ne
  change de rendu.
* **`leyline-color` gagne un second lecteur de format**, et le même argument que
  pour le DCP : dépendance nulle, surface de lecture seule, erreurs nommées.
* **Deux conversions d'axe par pixel quand la LUT est active** (aller-retour
  linéaire ↔ affichage), ce qui est le coût de l'appliquer là où elle a un sens.

## Alternatives écartées

* **Appliquer la LUT en lumière linéaire**, sans conversion. Moins de calcul, et
  un rendu qui n'a rien à voir avec ce que le fichier décrit : le contenu d'un
  `.cube` est défini sur des valeurs d'affichage (§3).
* **La placer après `output_rendering`**, dans l'espace de sortie. Ce serait la
  place la plus fidèle à l'intention d'un coloriste, mais elle mettrait un
  opérateur *après* la conversion vers l'espace de sortie, donc hors du tampon
  de travail — et rendrait le résultat dépendant du profil de sortie choisi à
  l'export. Un look ne doit pas changer selon qu'on exporte en sRGB ou en
  Adobe RGB.
* **Copier les octets de la LUT dans `settings_json`.** Une révision autonome
  jusqu'au bout, et un `settings_json` de plusieurs mégaoctets par photo. ADR
  0035 a déjà tranché ce compromis dans l'autre sens.
* **Un chemin absolu vers le fichier de l'utilisateur.** Interdit par
  ADR 0010 : la bibliothèque cesserait d'être déplaçable.
* **Embarquer un jeu de simulations de film.** §6.
