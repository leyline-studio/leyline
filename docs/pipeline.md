# Pipeline Development Specification

**Document:** `docs/pipeline.md`
**Version:** 1.0
**Status:** Draft

---

# 1. Objectif

Le pipeline de développement garantit que tous les traitements de Leyline sont **reproductibles**, **versionnés** et **non destructifs**.

Les algorithmes évolueront au fil du temps : corrections, optimisations, nouveaux paramètres, nouveaux modèles.

Cette évolution ne doit **jamais** invalider les résultats déjà calculés.

Une révision de développement créée aujourd'hui doit produire exactement les mêmes pixels dans dix ans.

---

# 2. Principes

* chaque pipeline possède une identité stable ;
* chaque évolution incompatible crée une nouvelle version ;
* chaque exécution conserve une copie complète des paramètres utilisés ;
* les paramètres d'une exécution sont immuables ;
* les résultats historiques ne sont jamais modifiés ;
* un recalcul crée toujours une nouvelle exécution.

Ce document couvre deux niveaux :

1. le **pipeline de développement RAW** — le cœur de Leyline (V1) ;
2. les **pipelines de traitement génériques** — le cadre qui le généralise (miniatures, histogrammes aujourd'hui ; visages, OCR, IA demain, voir §38 du catalogue).

---

# 3. Le pipeline de développement RAW

## 3.1 Ordre des opérations

Le développement applique une chaîne d'opérations **dans un ordre fixe**, défini par le moteur — jamais par l'utilisateur.

```text
RAW décodé

↓

Entrée (`input` : configuration du décodeur, espace du tampon)

↓

Profil d'appareil (DCP)

↓

Correction d'objectif

↓

Suppression de tache

↓

Balance des blancs

↓

Exposition

↓

Contraste

↓

Hautes lumières / Ombres

↓

Blancs / Noirs

↓

Courbe tonale

↓

Clarté / Texture / Dehaze

↓

Vibrance / Saturation

↓

Mélangeur TSL (HSL)

↓

Color Grading (ombres/tons moyens/hautes lumières)

↓

Réglages locaux (masqués)

↓

Réduction du bruit

↓

Netteté

↓

Rotation / Recadrage

↓

Rendu de sortie (`output_rendering` : du tampon de travail au signal d'affichage)

↓

Sortie (aperçu ou export)
```

L'utilisateur règle des **valeurs**, jamais l'ordre.

Les deux étages d'extrémité, `input` et `output_rendering`, encadrent le pipeline depuis [ADR 0044](adr/0044-linear-wide-gamut-working-space.md). Ils n'ont pas de valeur neutre — il n'existe pas de rendu sans entrée ni sortie — et sont donc les seuls que **toute** révision inscrit dans sa carte `stages`. `input` porte notamment la configuration demandée au décodeur, qui change les pixels et n'était épinglée nulle part avant cet ADR. L'**espace de travail** du tampon est une propriété déclarée par chaque version d'étage : deux versions d'espaces différents ne composent pas, et un plan qui les mélange **échoue** (`MixedWorkingSpaces`) au lieu d'être rendu au mieux. Migrer une révision d'un espace à l'autre est un retraitement (§4.5), donc une nouvelle révision.

Cet ordre fait partie du contrat de rendu : le modifier change les pixels produits, donc impose une nouvelle *version d'étage* déclarant un autre rang (§3.3).

**Sources non-RAW.** Le catalogue accepte à l'import des fichiers JPEG, TIFF et PNG (catalogue §10). Ces fichiers entrent dans la même chaîne : ils sont décodés par des codecs natifs (orientation EXIF appliquée, échantillons normalisés en RGB 8 bits) et prennent la place de « RAW décodé » en tête de pipeline. Le décodage reste déterministe au même titre que LibRaw (§5). HEIF et PSD sont catalogués mais n'ont pas de décodeur en V1 : demander leurs pixels est une erreur explicite, pas un refus LibRaw.

---

## 3.2 settings_json

Chaque révision (`develop_revisions.settings_json`, catalogue §17) contient un **état complet et autonome** du développement.

Jamais un delta.

```json
{
    "schema": 1,
    "stages": {
        "input": 1,
        "camera_profile": 1, "gains": 1, "contrast": 1, "crop": 1,
        "output_rendering": 1
    },

    "camera_profile": {
        "enabled": true,
        "path": "Profiles/Camera/Canon EOS 60D.dcp",
        "checksum": "blake3:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
    },
    "white_balance": { "temperature": 5400, "tint": 4 },
    "exposure": 0.35,
    "contrast": 12,
    "highlights": -40,
    "shadows": 25,
    "whites": 0,
    "blacks": -5,
    "clarity": 25,
    "texture": 15,
    "dehaze": 30,
    "vibrance": 18,
    "saturation": 0,

    "tone_curve": {
        "points": [
            { "x": 0.0,  "y": 0.0 },
            { "x": 0.25, "y": 0.30 },
            { "x": 0.75, "y": 0.70 },
            { "x": 1.0,  "y": 1.0 }
        ]
    },
    "spot_removal": [
        {
            "target": { "x": 0.62, "y": 0.31 },
            "source": { "x": 0.55, "y": 0.29 },
            "radius": 0.03,
            "feather": 0.40,
            "opacity": 1.0
        }
    ],
    "hsl": [
        { "hue": 0,  "saturation": -20, "luminance": 0 },
        { "hue": 5,  "saturation": 0,   "luminance": 0 },
        { "hue": 0,  "saturation": 0,   "luminance": 0 },
        { "hue": -10, "saturation": 15, "luminance": 8 },
        { "hue": 0,  "saturation": 0,   "luminance": 0 },
        { "hue": 8,  "saturation": 25,  "luminance": 0 },
        { "hue": 0,  "saturation": 0,   "luminance": 0 },
        { "hue": 0,  "saturation": 0,   "luminance": 0 }
    ],
    "color_grading": {
        "shadows":    { "hue": 220, "saturation": 15, "luminance": 0 },
        "midtones":   { "hue": 0,   "saturation": 0,  "luminance": 0 },
        "highlights": { "hue": 45,  "saturation": 10, "luminance": 0 },
        "balance": 0,
        "blending": 50
    },
    "local_adjustments": [
        {
            "mask": {
                "type": "radial",
                "cx": 0.5, "cy": 0.42,
                "rx": 0.30, "ry": 0.22,
                "angle": 0.0,
                "feather": 0.40,
                "inverted": false
            },
            "opacity": 1.0,
            "adjustments": { "exposure": 0.6, "contrast": 15, "highlights": -20 }
        }
    ],

    "lens_correction": { "enabled": true, "profile": "auto" },
    "noise_reduction": { "luminance": 15, "color": 25 },
    "sharpening": { "amount": 40, "radius": 1.0 },

    "rotation": 0.0,
    "crop": { "x": 0.1, "y": 0.2, "width": 0.8, "height": 0.7 }
}
```

### Champs réservés

| Champ | Rôle |
|---|---|
| `schema` | Version du **format** des paramètres (structure du JSON) |
| `stages` | Version du **rendu**, étage par étage (algorithmes produisant les pixels) |

Les deux évoluent indépendamment : on peut renommer un champ sans changer le rendu, et corriger un algorithme sans changer la structure.

`stages` associe à chaque étage **actif** la version de cet étage qui rend cette révision. Un étage à sa valeur neutre ne s'exécute pas, n'a donc aucun comportement à épingler, et **n'y figure pas** — la carte est proportionnelle à l'édition réelle, pas au nombre d'étages du moteur. Une révision neutre n'inscrit donc que les deux étages d'encadrement (§3.1), qui n'ont pas de valeur neutre.

### Valeurs omises

Un paramètre absent vaut sa **valeur neutre**, définie par le schéma.

Les valeurs neutres sont **gelées par version de schéma** : elles ne changent jamais rétroactivement.

`{}` avec `schema: 1` produira toujours le rendu neutre du schéma 1.

Valeurs neutres du schéma 1 :

| Paramètre | Valeur neutre |
|---|---|
| `camera_profile` | absent — conversion sRGB propre au décodeur, aucun profil appliqué |
| `white_balance` | absent — balance des blancs « telle que prise » du boîtier |
| `exposure` | 0.0 EV |
| `contrast`, `highlights`, `shadows`, `whites`, `blacks`, `vibrance`, `saturation` | 0 |
| `clarity`, `texture`, `dehaze` | 0 |
| `hsl` | absent — 8 bandes à `{ "hue": 0, "saturation": 0, "luminance": 0 }` |
| `color_grading` | absent — chaque zone à `{ "hue": 0, "saturation": 0, "luminance": 0 }`, `balance`/`blending` à 0 |
| `lens_correction` | `{ "enabled": false, "profile": "auto" }` |
| `noise_reduction` | `{ "luminance": 0, "color": 0 }` |
| `sharpening` | `{ "amount": 0, "radius": 1.0 }` |
| `rotation` | 0.0 |
| `crop` | absent — image entière |

### Unités et conventions

* `temperature` : Kelvin ;
* `exposure` : EV ;
* `rotation` : degrés, sens horaire ;
* `crop` : coordonnées normalisées [0, 1] relatives à l'image **après** rotation ;
* les curseurs sans unité physique (`contrast`, `vibrance`...) : entiers dans [-100, +100], 0 = neutre ;
* `hsl[].hue`, `color_grading.{shadows,midtones,highlights}.luminance`, `color_grading.balance` : entiers dans [-100, +100], 0 = neutre ;
* `color_grading.{shadows,midtones,highlights}.hue` : degrés, entier dans [0, 360) ;
* `color_grading.{shadows,midtones,highlights}.saturation`, `color_grading.blending` : entiers dans [0, 100], 0 = neutre ;
* `camera_profile.path` : chemin relatif à la racine de la bibliothèque, séparateur `/`, par convention sous `Profiles/Camera/` ;
* `camera_profile.checksum` : `"blake3:"` suivi des 64 chiffres hexadécimaux du hachage BLAKE3 du fichier `.dcp` (ADR 0006, appliqué ici à une entrée référencée et non à une photo). Une empreinte qui ne correspond plus au fichier sur disque **échoue le rendu** (`CameraProfileFailed`) au lieu de rendre d'autres couleurs en silence — même posture que `NewerSettings` (§3.4).

---

## 3.3 Versions d'étages

Le champ `stages` joue le rôle des *process versions* de Lightroom (2003, 2010, 2012...), à une granularité près : ce n'est pas le pipeline entier qui porte un numéro, c'est **chaque opérateur** ([ADR 0042](adr/0042-versioned-stage-pipeline.md)).

Règle fondamentale :

> **Aucune version publiée du logiciel — correctif, mineure ou majeure — ne modifie le rendu d'une version d'étage déjà publiée.**

* Une révision est toujours rendue avec les versions d'étages qu'elle déclare.
* Une correction d'algorithme qui change les pixels produits = nouvelle version de cet étage, dans un nouveau module ; l'ancienne n'est jamais touchée.
* Une optimisation qui produit des pixels identiques = pas de nouvelle version.
* Le rang d'un étage dans le pipeline appartient à la version : déplacer un étage est une nouvelle version qui déclare un autre rang, jamais une modification de l'existante.
* L'utilisateur peut migrer une photo vers les versions courantes (§4.5) : cela crée une **nouvelle révision** — l'ancienne reste rendable à l'identique.

**Épinglage.** La carte est écrite par le moteur au moment où la révision est écrite, jamais déduite à la lecture :

* un étage déjà inscrit **garde** sa version — éditer en 2036 une photo de 2026 ne la re-rend pas à travers du code plus récent ;
* un étage qui vient de quitter sa valeur neutre reçoit la version courante du moteur **dans l'espace de travail que la révision déclare déjà** — jamais une version qui la ferait changer d'espace par effet de bord ([ADR 0044](adr/0044-linear-wide-gamut-working-space.md) §4) ;
* un étage redevenu neutre **perd** son entrée, puisqu'il ne rend plus rien ; `input` et `output_rendering` font exception, n'ayant pas de valeur neutre.

Un étage actif mais sans version inscrite rend à la version courante. Ce cas ne concerne que des réglages construits en mémoire (SDK, préréglage, test) : toute révision *stockée* reçoit ses entrées à l'écriture.

Le code de chaque version d'étage est conservé dans le moteur pour toujours : c'est le prix de la promesse « mêmes pixels dans dix ans », dont §5.1 énonce la portée exacte. Il se paie désormais par opérateur réellement corrigé — quelques dizaines de lignes — et non plus par copie intégrale du pipeline.

**Étages connus, et l'ordre dans lequel ils s'exécutent.** Tous sont en version 1 : l'historique de rendu antérieur à la publication a été effondré ([ADR 0043](adr/0043-collapse-prerelease-render-history.md)), puisqu'aucune révision au monde ne le citait.

| Rang | Étage | Version | Rôle |
|---|---|---|---|
| 0 | `input` | 1 | Configuration demandée au décodeur (sortie native capteur ou sRGB) et espace du tampon de travail ; toujours actif (ADR 0044) |
| 10 | `camera_profile` | 1 | Matrice DCP boîtier → sRGB linéaire, avant tout le reste : elle établit l'espace du tampon de travail (ADR 0035, conteneur lu selon ADR 0037) |
| 20 | `lens` | 1 | Distorsion, aberration chromatique transversale et vignettage via un profil Lensfun (ADR 0016–0018) |
| 30 | `spot_removal` | 1 | Clonage déterministe par copie bilinéaire adoucie, sans mode *heal* (ADR 0031) |
| 40 | `gains` | 1 | Balance des blancs et exposition, en lumière linéaire, via des tables de transfert de 4096 intervalles (ADR 0013) |
| 50 | `contrast` | 1 | Courbe en S autour du gris moyen |
| 60 | `highlights_shadows` | 1 | Hautes lumières et ombres, masquées par la luminance |
| 70 | `whites_blacks` | 1 | Remappage des extrémités |
| 80 | `tone_curve` | 1 | Courbe par points, spline cubique monotone précalculée en table (ADR 0030) |
| 90 | `clarity` | 1 | Contraste local à grand rayon (ADR 0033) |
| 100 | `texture` | 1 | Même opérateur à petit rayon (ADR 0033) |
| 110 | `dehaze` | 1 | Suppression de voile par *dark channel prior* (ADR 0033) |
| 120 | `vibrance` | 1 | Saturation pondérée par le chroma existant |
| 130 | `saturation` | 1 | Saturation uniforme |
| 140 | `hsl` | 1 | Mélangeur TSL à 8 bandes de teinte, fondu entre bandes adjacentes (ADR 0032) |
| 150 | `color_grading` | 1 | Trois zones ombres/tons moyens/hautes lumières pondérées par la luminance (ADR 0032) |
| 160 | `local_adjustments` | 1 | Réglages locaux masqués (brosse/radial/gradient), réutilisant les opérateurs globaux restreints à une couverture (ADR 0029) |
| 170 | `noise_luminance` | 1 | Réduction du bruit de luminance |
| 180 | `noise_color` | 1 | Réduction du bruit chromatique |
| 190 | `sharpen` | 1 | Masque flou sur le plan de luminance |
| 200 | `rotate` | 1 | Rotation d'angle arbitraire, échantillonnage bilinéaire |
| 210 | `crop` | 1 | Recadrage |
| 900 | `output_rendering` | 1 | Du tampon de travail au signal d'affichage ; toujours actif. Sans effet tant que le tampon est déjà sRGB affichable (ADR 0044 §3) |

Les rangs vont de dix en dix : un étage futur s'insère entre deux existants sans que personne ne renumérote quoi que ce soit.

Une révision éditée hérite des versions d'étages de son parent ; seules les nouvelles révisions par défaut (imports) épinglent les versions courantes.

---

## 3.4 Évolution du schéma

Le champ `schema` s'incrémente selon les mêmes règles que les pipelines génériques (§4.4) :

Compatible (pas d'incrément) :

* ajout d'un paramètre optionnel avec valeur neutre ;
* ajout de documentation ou de contraintes de validation.

Incompatible (incrément obligatoire) :

* suppression ou renommage d'un paramètre ;
* changement de type, d'unité ou de signification ;
* changement de plage de valeurs.

Aucune migration des révisions existantes n'est jamais effectuée : le moteur sait **lire** tous les schémas passés.

### Compatibilité ascendante

Un moteur qui rencontre un `schema` plus récent que ce qu'il connaît, ou une version d'étage qu'il n'implémente pas :

* ne modifie jamais la révision ;
* n'édite pas l'asset (lecture seule) ;
* affiche la meilleure préversion disponible (dernière preview en cache) avec un avertissement.

Un vieux moteur ne doit jamais détruire le travail d'un moteur récent. Un étage inconnu n'est jamais **sauté** : le rendu échoue (`UnknownStage`), car rendre la photo sans un opérateur que son auteur a vu serait lui montrer d'autres pixels sans le dire.

Même posture pour une révision dont les versions d'étages ne s'accordent pas sur un espace de travail : le rendu échoue (`MixedWorkingSpaces`) en nommant les deux étages en désaccord. Un opérateur écrit pour la lumière linéaire à qui l'on donne un tampon gamma-encodé produirait des pixels plausibles et faux — le seul cas pire qu'une erreur.

Le champ `process`, retiré par [ADR 0043](adr/0043-collapse-prerelease-render-history.md), fait exception à la règle de préservation des champs inconnus : un document qui le porte encore est **refusé**. Cette règle protège le travail d'un moteur *plus récent* ; un champ *supprimé* signale au contraire un document antérieur à la carte d'étages, qu'il serait faux de rendre comme s'il n'en portait pas.

---

# 4. Pipelines de traitement génériques

Le mécanisme du développement RAW se généralise à tout traitement automatique produisant des résultats à partir d'un asset.

Exemples :

Aujourd'hui (V1) :

* génération de miniatures et previews ;
* calcul d'histogrammes ;
* extraction EXIF.

Demain (§38 du catalogue, hors V1) :

* détection de visages ;
* OCR ;
* classification par IA locale ;
* vecteurs de recherche.

---

## 4.1 Identité d'un pipeline

Chaque pipeline possède :

| Champ | Description |
|---|---|
| `id` | Identifiant fonctionnel stable (ex. `face_detection`) |
| `version` | Version majeure du pipeline |
| `schema` | Schéma JSON décrivant les paramètres autorisés |
| `description` | Documentation optionnelle |

---

## 4.2 Exécutions

Chaque exécution enregistre une **copie complète** des paramètres réellement utilisés.

```json
{
    "model": "yolov12-face",
    "confidence": 0.55,
    "min_size": 48,
    "gpu": true,
    "merge_distance": 12
}
```

Ces paramètres sont :

* immuables ;
* indépendants du schéma courant ;
* conservés pendant toute la durée de vie de la bibliothèque.

Le traitement lit **exclusivement** les paramètres contenus dans cette copie — jamais la configuration courante de l'application.

### Déroulement

1. chargement du pipeline ;
2. sélection de la version ;
3. validation des paramètres via le schéma ;
4. copie complète des paramètres dans `settings_json` ;
5. lancement du traitement ;
6. enregistrement des résultats.

---

## 4.3 Schéma de validation

Chaque pipeline expose un schéma JSON :

```json
{
    "type": "object",
    "properties": {
        "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
        "gpu": { "type": "boolean" },
        "min_size": { "type": "integer", "minimum": 1 }
    },
    "required": ["confidence"]
}
```

Le schéma sert uniquement à :

* valider les paramètres avant exécution ;
* documenter le pipeline ;
* générer des interfaces de configuration.

Le schéma n'est **jamais** utilisé pour reconstruire ou réinterpréter une ancienne exécution.

---

## 4.4 Évolution et compatibilité

Évolutions compatibles :

* ajout d'un paramètre optionnel ;
* ajout de contraintes de validation ;
* ajout de documentation.

Évolutions incompatibles — nouvelle version obligatoire :

* suppression ou renommage d'un paramètre ;
* changement de signification ou de type ;
* changement de comportement produisant des résultats différents.

Une nouvelle version ne remplace jamais une ancienne. Plusieurs versions coexistent :

```text
face_detection v1
face_detection v2
face_detection v3
```

Chaque exécution référence explicitement la version utilisée.

Exemple de renommage entre versions :

```json
v2 : { "confidence": 0.50 }

v3 : { "score_threshold": 0.50 }
```

Les deux formats restent valides pour les exécutions qui les utilisent. Aucune migration des paramètres historiques n'est autorisée.

---

## 4.5 Reprocessing

Lorsqu'un pipeline évolue, les assets peuvent être retraités : le retraitement remonte chaque étage épinglé d'une révision à sa version courante, en conservant les valeurs des réglages.

Le retraitement :

* crée une nouvelle exécution ;
* conserve les anciennes ;
* ne remplace jamais les résultats historiques.

Les anciens résultats restent consultables tant que l'utilisateur ne les supprime pas explicitement.

---

# 5. Reproductibilité

## 5.1 Ce qui est garanti

Deux exécutions produisent le **même résultat, bit pour bit**, si et seulement si :

* les paramètres enregistrés sont strictement identiques ;
* les versions d'étages mises en jeu sont identiques (ADR 0042) — y compris celles des deux étages d'encadrement, qui épinglent l'espace de travail et la configuration du décodeur (ADR 0044) ; pour un pipeline générique, l'identité du pipeline et sa version (§4.1) ;
* la ressource d'entrée est identique (même `checksum`) ;
* la plateforme et la chaîne de compilation sont les mêmes (§5.2).

Cette garantie ne dépend **ni de la version de l'application, ni du profil de compilation**. Leyline 1.0.3 et Leyline 7.2.0 rendent `sharpen::v1` à l'identique, parce qu'il s'agit littéralement du même code gelé dans les deux binaires ; un build `debug` et un build `release` également, ce que les rendus de référence (`crates/leyline-engine/src/stages/golden.rs`, manifeste dans `tests/golden/renders.json`) vérifient dans les deux profils.

Ces rendus de référence épinglent, avec chaque empreinte, **la carte `stages` qui l'a produite**, et rejouent chaque entrée à travers cette carte-là. Une nouvelle version d'étage ne peut donc pas déplacer une empreinte existante : elle en ajoute une. Trois gardes tiennent ensemble — les entrées épinglées rendent toujours les mêmes pixels, ce que le moteur épingle *aujourd'hui* figure au manifeste, et aucune paire `(étage, version)` publiée n'échappe au manifeste.

C'est la bonne échelle pour énoncer la promesse : un utilisateur sait quelles versions d'étages sa photo cite — elles sont écrites dans sa révision — alors qu'il ignore quel build a produit ses pixels.

D'où la règle de publication, qui est la forme opérationnelle de la promesse :

> **Aucune version publiée — correctif, mineure ou majeure — ne modifie le rendu d'une version d'étage déjà publiée.** Si le rendu doit changer, c'est une nouvelle version d'étage ; les révisions existantes continuent de citer l'ancienne.

Un rendu qui change n'est donc jamais un incrément de version de l'application : c'est un nouvel étage. Et la règle est **vérifiée mécaniquement** plutôt que promise — la suite de rendus de référence tourne avant publication, et une empreinte qui bouge bloque la publication.

Conserver l'ancien code dans l'arbre est ce qui rend cette garantie réelle, et une étiquette Git n'y suffit pas : le binaire de 2036 est compilé depuis l'arbre de 2036, et une photo de 2026 n'est correctement rendue que si `sharpen::v1` s'y trouve encore. L'étiquette Git sert à *auditer* que l'étage n'a jamais bougé (`git log` vide depuis sa publication) — pas à le livrer.

## 5.2 Ce qui n'est pas garanti

**Changer de plateforme ou de chaîne de compilation.** Le pipeline appelle `powf`, `ln` et `exp` : ces fonctions viennent de la bibliothèque mathématique du système, dont les résultats ne sont pas identiques au dernier bit d'une plateforme, d'une version de libm ou d'une version de LLVM à l'autre. Entre deux plateformes, le rendu est donc **visuellement identique, à une dérive de dernier bit près** — pas bit pour bit.

Prétendre l'inverse serait promettre ce qu'aucun moteur ne tient. Lightroom ne le tient pas (ses chemins GPU et CPU ne donnent pas les mêmes pixels) ; darktable non plus (il migre les paramètres des anciens modules vers le code courant plutôt que de geler ce code). Leyline garantit strictement plus qu'eux dans le cadre de §5.1, et s'arrête exactement là où s'arrête la virgule flottante.

Conséquence pratique : la chaîne d'outils est **épinglée sur une version exacte** dans `rust-toolchain.toml`. En changer est un acte délibéré, qui impose de rejouer les rendus de référence et de consigner toute dérive constatée — jamais l'effet de bord d'un correctif.

## 5.3 Déterminisme

Les traitements doivent être **déterministes** : tout élément non déterministe (graine aléatoire, ordre des threads affectant le résultat) doit être fixé et enregistré dans les paramètres. Le parallélisme reste autorisé tant qu'il ne change ni la formule ni l'ordre des opérations pour un échantillon donné (ADR 0012).

---

# 6. Contrat de non-destructivité

Les règles suivantes sont invariantes :

* un pipeline peut évoluer ;
* un schéma peut évoluer ;
* les paramètres d'une exécution ne changent jamais ;
* les résultats historiques ne sont jamais modifiés ;
* un recalcul produit toujours une nouvelle exécution ;
* toute exécution historique reste reproductible ;
* un moteur récent lit tous les formats passés ; un moteur ancien ne modifie jamais un format qu'il ne connaît pas.

---

# 7. Articulation avec le catalogue

| Concept pipeline | Réalité catalogue (`docs/catalog.md`) |
|---|---|
| Exécution de développement | Ligne de `develop_revisions` |
| Paramètres immuables | `settings_json` (état complet) |
| Version de format | Champ `schema` du JSON |
| Version de rendu | Champ `stages` du JSON, une entrée par étage actif |
| Recalcul | Nouvelle révision dans le graphe |
| Résultat matérialisé | `previews`, invalidées par comparaison de `revision_id` (§20) |
| Coalescence | Une révision = une intention (§17) — la granularité des exécutions suit la même règle |

La seule exception à l'immuabilité des révisions est la fenêtre d'amendement du catalogue (§17), strictement bornée aux révisions non référencées.
