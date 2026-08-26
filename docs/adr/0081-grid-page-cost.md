# ADR 0081 — La page de grille ne trie plus la bibliothèque entière

**Statut :** Accepté — 2026-08

## Contexte

`catalog.md` §35 promet, sous « Navigation », un **défilement instantané sur
plusieurs centaines de milliers d'assets**, et ajoute qu'« au-delà de cette
jointure, les requêtes courantes évitent toute jointure supplémentaire ».
Mesuré le 2026-08-26 sur une bibliothèque synthétique de **50 000 assets**
(l'ordre de grandeur du corpus de test réel : 15 000 CR2 et 29 000 JPEG),
compilée en `--release` : **aucune des deux affirmations n'est vraie**.

Ce n'est pas un détail de confort. `load_window`
(`crates/leyline-studio/src/wiring/grid.rs`) appelle `Catalog::grid` **sur le
fil de l'interface**, une fois par pas de défilement, avec une fenêtre de la
taille du viewport plus la marge d'overscan — une centaine de lignes.

### Ce que la mesure impose à la conception

Trois faits, dont deux qu'aucune lecture du code ne donnait :

* **SQLite matérialise la ligne de sortie complète *avant* de trier.** Les deux
  sous-requêtes corrélées de la liste de sélection — la pastille « déjà
  développée » ([ADR 0055](0055-library-navigation.md) §5) et la pastille
  `RAW+J` ([ADR 0079](0079-raw-jpeg-pairing.md) §6) — s'exécutent donc **50 000
  fois pour afficher 100 vignettes**. Une liste de sélection allégée de ces
  deux colonnes tombe de 65 ms à 32 ms : elles pèsent la moitié du temps, et
  elles le pèsent sur des lignes que personne ne verra.
* **La taille de la fenêtre demandée ne change presque rien.** 100 lignes
  coûtent 10,4 ms, 1 000 lignes 17,0 ms. Le coût n'est pas la lecture des
  lignes rendues, c'est le tri de l'ensemble filtré tout entier —
  `EXPLAIN QUERY PLAN` répond `USE TEMP B-TREE FOR ORDER BY` dans tous les cas.
* **Le terme d'expression en tête du `ORDER BY` interdit à tout index de servir
  le tri.** `ORDER BY a.capture_date IS NULL, a.capture_date DESC` ne peut être
  satisfait par aucun index sur `capture_date`. Et en descendant il est
  **redondant** : SQLite trie déjà les NULL en dernier quand l'ordre est
  `DESC`. Il n'est nécessaire qu'en ascendant, où `NULLS LAST` le dit sans
  interdire l'index.

## Décision

### 1. La page se choisit sur des lignes maigres, puis se décore

`Catalog::grid` émet désormais deux niveaux au lieu d'un. Le niveau intérieur
porte les filtres, l'ordre et la fenêtre, et ne sélectionne que **deux
entiers** — l'identifiant de version et celui d'asset. Le niveau extérieur
rejoint `develop_versions` et `assets` sur la centaine de lignes survivantes
et y calcule les onze colonnes de `GridItem`, sous-requêtes de pastilles
comprises :

```sql
WITH page AS (
    SELECT c.version_id AS vid, a.id AS aid
    FROM ...  WHERE ...  ORDER BY ...  LIMIT ? OFFSET ?
)
SELECT <les onze colonnes>
FROM page
JOIN develop_versions v ON v.id = page.vid
JOIN assets a ON a.id = page.aid
ORDER BY ...
```

Les deux pastilles s'évaluent alors cent fois et non cinquante mille. Le tri,
lui, ne porte plus que sur des paires d'entiers.

C'est aussi ce qui rend §35 littéralement vrai pour la première fois : les
jointures supplémentaires existent toujours, mais elles ne s'appliquent qu'à
la page.

### 2. L'ordre perd son terme d'expression

`ORDER BY a.capture_date IS NULL, a.capture_date DESC, a.id` devient :

* `ORDER BY a.capture_date DESC, a.id` en descendant — les NULL tombent déjà
  en dernier, le terme retiré ne décidait rien ;
* `ORDER BY a.capture_date ASC NULLS LAST, a.id` en ascendant — même ordre
  qu'avant, dit d'une façon qu'un index peut servir.

**L'ordre observable est identique**, et il est vérifié comme tel : un test
compare les 300 premières lignes de l'ancienne forme et de la nouvelle sur les
quatre tris. `NULLS LAST` existe depuis SQLite 3.30 (2019), très en deçà de la
version embarquée.

### 3. Un index couvrant qui commence par la clause que toute grille porte

```sql
CREATE INDEX idx_assets_grid ON assets(companion_of, capture_date, id);
```

`companion_of` d'abord parce que **toute** requête de grille porte
`a.companion_of IS NULL` ([ADR 0079](0079-raw-jpeg-pairing.md) §5) ;
`capture_date` ensuite parce que c'est le tri par défaut et de loin le plus
utilisé ; `id` enfin, qui est la rupture d'égalité.

`idx_assets_companion` **reste**, bien que ce nouvel index l'ait pour préfixe
strict. C'était la décision inverse jusqu'à ce qu'elle soit mesurée :
`count()` parcourt cet index d'un bout à l'autre pour dimensionner l'ascenseur
de la grille, et le parcourir dans sa version large coûte **28,7 ms contre
3,3 ms**. La finesse des entrées est exactement ce qui rend ce parcours bon
marché ; le même index couvrant sert aussi le test d'existence du compagnon
([ADR 0079](0079-raw-jpeg-pairing.md) §6). Le prix est un b-tree de plus par
asset enregistré, sur un chemin d'écriture qui n'est pas là où les imports
passent leur temps.

C'est le seul endroit de cet ADR où la mesure a renversé la conception, et
elle l'a fait après l'écriture du code : sans elle, le comptage aurait été
huit fois plus lent pour économiser un index.

### 4. `count()` ne change pas de forme

Le comptage ne trie pas et ne rend aucune colonne : la jointure différée n'a
rien à lui apporter. Il continue d'émettre une seule requête, sur le même
tronc `FROM`/`WHERE` partagé — ce tronc devient une fonction, et c'est la
seule raison pour laquelle `build` est découpé.

## Conséquences

Mesures sur **une même base** de 50 000 assets, dates de capture mélangées,
fenêtre de 100 lignes, `--release` : l'ancienne forme exécutée à la main et la
nouvelle par `Catalog::grid`, l'une après l'autre.

| | avant | après | rapport |
|---|---|---|---|
| tête, tri par défaut (date décroissante) | 10,32 ms | **0,46 ms** | ×22 |
| tête, date croissante | 10,38 ms | **0,42 ms** | ×25 |
| fenêtre de 1 000 lignes | 15,99 ms | 3,30 ms | ×4,8 |
| tri par date d'import | 58,87 ms | 16,47 ms | ×3,6 |
| offset 25 000 | 69,97 ms | 20,83 ms | ×3,4 |
| offset 49 000 | 77,01 ms | 42,01 ms | ×1,8 |
| `count()` | 3,47 ms | 3,47 ms | inchangé |

Le gain est le plus grand là où il compte le plus — l'ouverture d'un dossier
et le début du défilement — et il croît avec la taille de la bibliothèque,
puisque c'est un `O(N log N)` par page qui devient un parcours d'index borné
par la fenêtre.

Deux effets à connaître :

* **La pagination par `OFFSET` reste linéaire en l'offset.** Descendre au fond
  d'une bibliothèque de 50 000 photos coûte encore 41 ms. Le différé y gagne
  toujours (77,5 ms aujourd'hui), mais l'index n'y peut presque rien : à
  l'offset 49 000, parcourir l'index coûte même ~10 ms de plus qu'un tri de
  lignes maigres. C'est le prix assumé d'un facteur 36 en tête.
* **Les tris autres que par date de capture ne gagnent que la jointure
  différée**, faute d'index qui les serve : la date d'import passe de 58,9 ms
  à 16,5 ms, `filename` et la note gagnent moins de 5 %. Aucun ne régresse.
* **Un filtre qui ne ramène rien coûte toujours un parcours complet** : pour
  prouver qu'aucune photo n'est notée 3 étoiles, il faut les avoir toutes
  regardées — 29,9 ms, index ou pas. Dès que le filtre ramène une page, on
  retombe à 0,31 ms. C'est une propriété de la question, pas de la requête.

## Alternatives écartées

* **Pagination par clé (`WHERE (capture_date, id) < (?, ?)`)** — c'est la vraie
  réponse au coût de l'offset, et elle est incompatible avec l'interface :
  `desired_window` demande une fenêtre **par indice absolu**, parce que
  l'ascenseur de la grille doit pouvoir sauter n'importe où. Changer cela est
  une décision d'interface, pas de catalogue.
* **Dénormaliser les deux pastilles en colonnes d'`assets`** — supprimerait les
  sous-requêtes, mais introduirait deux colonnes à tenir à jour à chaque
  révision et à chaque appairage, donc deux façons de mentir. La jointure
  différée les rend assez rares pour que la question ne se pose plus.
* **Mémoriser la page dans Studio** — déplacerait le coût sans le supprimer, et
  le premier défilement le paierait quand même. Un cache est une réponse à un
  coût irréductible ; celui-ci ne l'était pas.
* **Retirer les pastilles** — elles sont décidées par
  [ADR 0055](0055-library-navigation.md) §5 et
  [ADR 0079](0079-raw-jpeg-pairing.md) §6, et elles ne coûtaient cher que par
  accident de forme.

## Ce que cet ADR ne fait pas

Aucun pixel ne bouge : ni étage, ni version d'étage, ni révision. `pipeline.md`
§5.1 est hors de cause. Le schéma ne gagne ni colonne ni table — un index
remplace un index. Et le contrat de `Catalog::grid` est inchangé : mêmes
paramètres, mêmes lignes, dans le même ordre.
