# Presets Specification

**Document:** `docs/presets.md`
**Version:** 1.0
**Status:** Draft

---

# 1. Objectif

Un preset de développement est un jeu de réglages nommé et réutilisable, applicable en un geste à une photo, à une sélection, ou à toute une bibliothèque.

C'est une attente de base d'un outil de développement RAW (parité Lightroom/Darktable) : ce document décrit ce que Leyline offre, avant toute question d'implémentation.

Hors sujet : les **export presets** (`catalog.md` §27, `engine-api.md` §12) existent déjà et concernent l'encodage de sortie (format, qualité, dimensions), pas le développement. Ce document couvre exclusivement les **presets de développement**.

---

# 2. Principes

* Un preset est **partiel par nature** : il touche les catégories de réglages que l'auteur a choisi d'y inclure, jamais les autres.
* Appliquer un preset ne détruit rien : conformément au pipeline (`pipeline.md` §2), chaque application produit une **nouvelle révision**, jamais une écrasement des réglages courants.
* Un preset s'applique aussi bien à une seule photo qu'à un lot arbitraire (sélection dans la grille, résultat de recherche, collection entière, toute la bibliothèque) : le traitement par lots est un cas nominal (`engine-api.md` §8), pas une option pour les presets non plus.
* Un preset est indépendant des photos auxquelles il a déjà été appliqué : le modifier n'affecte jamais les révisions passées (même contrat d'immuabilité que `pipeline.md` §2).
* Un preset est portable : son format est un JSON autonome, sans référence à une bibliothèque particulière — condition nécessaire pour l'export/import ou le partage volontaire évoqués dans `readme.md`.

---

# 3. Modèle

## 3.1 Catégories de réglages

Un preset ne capture jamais l'état complet d'un développement (`settings_json`, `pipeline.md` §3.2). Un preset qui capturerait tout écraserait aussi le cadrage et la rotation à l'application — un preset « Noir & Blanc contrasté » appliqué à toute une série ne doit pas recadrer chaque photo de la même façon.

Les réglages du schéma 1 (`pipeline.md` §3.2) se répartissent en **catégories**, à la granularité d'une case à cocher (à la Lightroom) — jamais au champ individuel :

| Catégorie | Champs couverts |
|---|---|
| `white_balance` | `white_balance` (temperature, tint) |
| `tone` | `exposure`, `contrast`, `highlights`, `shadows`, `whites`, `blacks` |
| `presence` | `vibrance`, `saturation` |
| `lens_correction` | `lens_correction` |
| `detail` | `noise_reduction`, `sharpening` |
| `geometry` | `rotation`, `crop` |

Une catégorie est **atomique** : l'inclure capture (ou applique) tous ses champs ensemble. On ne peut pas inclure `temperature` sans `tint`. Cette granularité correspond à ce que l'utilisateur choisit réellement (« je veux ce rendu de couleur et ce contraste, mais pas ce cadrage »), sans complexité superflue au niveau du champ.

`geometry` n'est **jamais incluse par défaut** à la création d'un preset : le cadrage et la rotation sont des jugements par photo, pas un style reproductible en série. L'utilisateur peut l'inclure explicitement (ex. un preset « carré centré » pour une série Instagram).

## 3.2 Format (`preset_json`)

```json
{
    "schema": 1,
    "groups": ["white_balance", "tone", "presence"],

    "white_balance": { "temperature": 5400, "tint": 4 },
    "exposure": 0.35,
    "contrast": 12,
    "highlights": -40,
    "shadows": 25,
    "whites": 0,
    "blacks": -5,
    "vibrance": 18,
    "saturation": 0
}
```

* `schema` référence la **même** numérotation que `settings_json.schema` (`pipeline.md` §3.2) : un preset utilise le vocabulaire de champs d'un schéma de développement donné, ce n'est pas un espace de versionnement séparé. Un preset schéma 1 se comprend avec les définitions de champs du schéma 1.
* `groups` est la **source de vérité** de ce que le preset touche. C'est nécessaire parce qu'un preset a le droit de vouloir remettre une catégorie à sa valeur neutre (ex. « désactive la réduction de bruit ») — un `noise_reduction` absent du JSON ne veut *pas* dire « n'y touche pas », contrairement à la règle des valeurs omises de `settings_json` (`pipeline.md` §3.2). C'est une divergence assumée : `preset_json` répond à « quoi appliquer », `settings_json` répond à « quel est l'état ».
* Seuls les champs des catégories listées dans `groups` apparaissent dans le JSON.
* Pas de champ `process` : un preset ne fixe jamais de process version. Une révision créée par application d'un preset hérite du process de sa révision parente, comme toute révision éditée (`pipeline.md` §3.3).

## 3.3 Évolution du schéma

Un preset suit la compatibilité de `pipeline.md` §3.4, sur le même schéma que `settings_json` :

* un moteur qui connaît le `schema` déclaré sait lire et appliquer le preset ;
* un moteur qui rencontre un `schema` plus récent que ce qu'il connaît refuse l'application (même famille d'erreur que `NewerSettings`, `engine-api.md` §4) plutôt que d'appliquer un sous-ensemble mal interprété ;
* le renommage ou la suppression d'un champ dans une nouvelle version de schéma rend les presets de l'ancien schéma toujours lisibles (aucune migration automatique), au même titre que les révisions historiques.

Un preset ne référence jamais une version d'étage : il reste valide à travers les évolutions de rendu, seul le `schema` des champs le concerne. Les versions d'étages viennent de la révision à laquelle il est appliqué (`pipeline.md` §3.3).

---

# 4. Catalogue

```sql
CREATE TABLE develop_presets (

    id INTEGER PRIMARY KEY,

    uuid TEXT NOT NULL UNIQUE,

    name TEXT NOT NULL,

    preset_json TEXT NOT NULL,

    created_at INTEGER NOT NULL

);
```

Même forme que `export_presets` (`catalog.md` §27) mais table distincte : ce sont deux domaines indépendants (réglages de développement vs. encodage de sortie), qui n'ont pas vocation à partager une clé étrangère ou des règles de suppression communes (voir ADR 0014).

Les presets sont indépendants des révisions qu'ils ont produites : supprimer un preset ne touche à aucune révision existante (pas de `FOREIGN KEY` depuis `develop_revisions`). L'historique reste lisible même si le preset qui l'a généré a depuis été renommé ou supprimé — cohérent avec `export_history` qui garde `preset_id` nullable en `ON DELETE SET NULL` (`catalog.md` §28) : ici on va plus loin, `develop_revisions` ne référence même pas le preset d'origine, parce qu'une révision est un état de réglages, pas la trace d'une action (`catalog.md` §17 : « une révision représente une intention utilisateur, jamais un événement d'interface »).

---

# 5. Application

## 5.1 Sur une version

Appliquer un preset à une version fusionne les champs des catégories incluses par-dessus les réglages courants de la tête, puis **commite** — exactement le geste d'un utilisateur qui ajusterait les curseurs des catégories concernées puis relâcherait (`pipeline.md` « points de commit »).

Conséquences directes de la réutilisation du modèle de révision existant, sans nouvel invariant :

* une nouvelle révision est toujours créée (jamais un amendement — « une action explicite l'exige » couvre déjà les snapshots et l'export, l'application d'un preset s'y range) ;
* le résultat est immédiatement undo-able comme n'importe quelle révision (`pipeline.md` §2, catalogue §16) ;
* le process de la révision créée est hérité de son parent (§3.2 ci-dessus) ;
* les catégories non incluses (typiquement `geometry`) restent strictement inchangées.

## 5.2 Sur un lot

Un preset s'applique à un ensemble de versions désigné comme n'importe quel autre traitement par lots de Leyline : sélection dans la grille, résultat de recherche, collection, ou la bibliothèque entière (même sélection que `GridQuery`, catalogue §30, déjà utilisée pour les collections intelligentes).

* Chaque version du lot reçoit sa propre révision, indépendante des autres — c'est une conséquence de « la version est l'unité de bibliothèque » (ADR 0008) : il n'existe pas de transaction unique couvrant tout le lot.
* Un échec sur une version (ex. `NewerSettings` si le preset référence un schéma que le moteur ne connaît plus) n'empêche pas les autres d'être traitées ; le résultat est rapporté par version, comme `export_batch` et l'import (`engine-api.md` §3.2, §12).
* Le lot est un travail long (potentiellement toute une bibliothèque) : il suit la catégorie « travaux » de `engine-api.md` §3.1, pas « requêtes ».

## 5.3 Non-destructivité

Aucune règle nouvelle : appliquer un preset, un par un ou en lot, c'est écrire des révisions par le mécanisme déjà spécifié (`pipeline.md` §2, §6). Rien n'est perdu, tout reste reproductible, l'historique complet de chaque photo (y compris les révisions pré-preset) reste consultable et restaurable.

---

# 6. Cycle de vie d'un preset

* **Créer** : depuis l'état courant d'une session d'édition (`engine-api.md` §10.1), en choisissant les catégories à capturer. Un preset peut aussi être créé « à blanc » (les valeurs neutres du schéma) puis édité.
* **Renommer**, **dupliquer**, **supprimer** : opérations simples sur `develop_presets`, sans effet rétroactif (§4).
* **Organiser** : une liste plate suffit au périmètre V1 (pas de dossiers de presets) — extension réservée si le besoin se confirme à l'usage, sur le modèle des mots-clés hiérarchiques (catalogue §22) si nécessaire.

---

# 7. Portabilité

`preset_json` ne référence rien de spécifique à une bibliothèque (pas d'ID interne, pas de chemin) : un preset peut être sérialisé seul (nom + `preset_json`) dans un fichier et rechargé dans une autre bibliothèque ou partagé, sans traduction. Ce n'est pas une fonctionnalité V1 (pas d'UI d'export/import de fichier preset prévue ici), mais c'est une propriété gratuite du format choisi, alignée avec l'intention déjà actée dans `readme.md` (« éventuellement partager volontairement des presets »).

---

# 8. Suite

Ce document fixe le contrat produit et le modèle de données. Intégré dans `engine-api.md` (surface `Library`, catégorie requête vs. travail, événements) et dans le moteur (`leyline-engine`, `leyline-catalog`) — voir `adr/0014-develop-presets.md`.
