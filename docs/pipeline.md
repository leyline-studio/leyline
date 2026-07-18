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

Correction d'objectif

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

Vibrance / Saturation

↓

Réduction du bruit

↓

Netteté

↓

Rotation / Recadrage

↓

Sortie (aperçu ou export)
```

L'utilisateur règle des **valeurs**, jamais l'ordre.

Cet ordre fait partie du contrat de rendu : le modifier change les pixels produits, donc impose une nouvelle *process version* (§3.3).

---

## 3.2 settings_json

Chaque révision (`develop_revisions.settings_json`, catalogue §17) contient un **état complet et autonome** du développement.

Jamais un delta.

```json
{
    "schema": 1,
    "process": 1,

    "white_balance": { "temperature": 5400, "tint": 4 },
    "exposure": 0.35,
    "contrast": 12,
    "highlights": -40,
    "shadows": 25,
    "whites": 0,
    "blacks": -5,
    "vibrance": 18,
    "saturation": 0,

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
| `process` | Version du **rendu** (algorithmes produisant les pixels) |

Les deux évoluent indépendamment : on peut renommer un champ sans changer le rendu, et corriger un algorithme sans changer la structure.

### Valeurs omises

Un paramètre absent vaut sa **valeur neutre**, définie par le schéma.

Les valeurs neutres sont **gelées par version de schéma** : elles ne changent jamais rétroactivement.

`{}` avec `schema: 1` produira toujours le rendu neutre du schéma 1.

### Unités et conventions

* `temperature` : Kelvin ;
* `exposure` : EV ;
* `rotation` : degrés, sens horaire ;
* `crop` : coordonnées normalisées [0, 1] relatives à l'image **après** rotation ;
* les curseurs sans unité physique (`contrast`, `vibrance`...) : entiers dans [-100, +100], 0 = neutre.

---

## 3.3 Process Version

Le champ `process` joue le rôle des *process versions* de Lightroom (2003, 2010, 2012...).

Règle fondamentale :

> **Un moteur donné doit savoir rendre toutes les process versions passées.**

* Une révision est toujours rendue avec la process version qu'elle déclare.
* Une correction d'algorithme qui change les pixels produits = nouvelle process version.
* Une optimisation qui produit des pixels identiques = pas de nouvelle version.
* L'utilisateur peut migrer une photo vers une process version récente : cela crée une **nouvelle révision** (le graphe Git du catalogue s'en charge naturellement) — l'ancienne reste rendable à l'identique.

Le code des anciennes process versions est conservé dans le moteur : c'est le prix de la promesse « mêmes pixels dans dix ans ».

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

Un moteur qui rencontre un `schema` ou un `process` **plus récent** que ce qu'il connaît :

* ne modifie jamais la révision ;
* n'édite pas l'asset (lecture seule) ;
* affiche la meilleure préversion disponible (dernière preview en cache) avec un avertissement.

Un vieux moteur ne doit jamais détruire le travail d'un moteur récent.

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

Lorsqu'un pipeline évolue, les assets peuvent être retraités.

Le retraitement :

* crée une nouvelle exécution ;
* conserve les anciennes ;
* ne remplace jamais les résultats historiques.

Les anciens résultats restent consultables tant que l'utilisateur ne les supprime pas explicitement.

---

# 5. Reproductibilité

Deux exécutions sont identiques si et seulement si :

* le pipeline est identique ;
* la version est identique ;
* les paramètres enregistrés sont strictement identiques ;
* la ressource d'entrée est identique (même `checksum`).

Dans ces conditions, le résultat doit être identique — au pixel près pour le développement RAW.

Conséquence pratique : les traitements doivent être **déterministes**. Tout élément non déterministe (seed aléatoire, ordre de threads affectant le résultat) doit être fixé et enregistré dans les paramètres.

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
| Version de rendu | Champ `process` du JSON |
| Recalcul | Nouvelle révision dans le graphe |
| Résultat matérialisé | `previews`, invalidées par comparaison de `revision_id` (§20) |
| Coalescence | Une révision = une intention (§17) — la granularité des exécutions suit la même règle |

La seule exception à l'immuabilité des révisions est la fenêtre d'amendement du catalogue (§17), strictement bornée aux révisions non référencées.
