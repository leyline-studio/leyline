# ADR 0064 — Filtrer la grille par métadonnée de prise de vue

**Statut :** Accepté — 2026-08

## Contexte

La grille se filtre aujourd'hui par dossier, collection, note, libellé, statut
de sélection, mots-clés, texte et date. **Rien sur les conditions de prise de
vue** : ni boîtier, ni objectif, ni sensibilité, ni ouverture, ni focale.

C'est un manque d'autant plus net que **la donnée est déjà là, et déjà
indexée**. La table `metadata` porte `camera_id`, `lens_id`, `iso`, et trois
colonnes générées `aperture_f`, `focal_length_mm`, `shutter_speed_s` ; six
index les couvrent (`docs/catalog.md` §32). Le schéma a été conçu pour cet
usage et rien ne l'a jamais exposé.

L'observation qui l'a fait remonter vient de RawTherapee, dont le panneau de
filtres liste boîtiers et objectifs **construits depuis le contenu réel** du
dossier ouvert. C'est un terrain où un catalogue devrait battre un navigateur
de fichiers : RawTherapee reconstruit ces listes en parcourant un dossier à
chaque ouverture, là où une requête indexée les donne sur la bibliothèque
entière, à n'importe quelle taille.

Un point de vigilance : **un filtre par boîtier existe déjà**, dans les
collections dynamiques (`SmartRules::camera`), avec sa propre sémantique de
correspondance — le modèle seul, ou `fabricant modèle`. Ajouter un second
filtre boîtier ailleurs, avec d'autres règles, ferait diverger deux réponses à
la même question.

## Décision

**Six filtres de prise de vue s'ajoutent à `GridQuery`, sans toucher au
schéma.**

### 1. Ce qui est filtrable, et comment

| Filtre | Forme | Colonne |
|---|---|---|
| Boîtier | valeur exacte, choisie dans une liste | `cameras` via `camera_id` |
| Objectif | idem | `lenses` via `lens_id` |
| Sensibilité | intervalle `[min, max]` | `iso` |
| Ouverture | intervalle | `aperture_f` |
| Focale | intervalle | `focal_length_mm` |
| Vitesse | intervalle | `shutter_speed_s` |

Les deux premiers sont **discrets** : on choisit un boîtier dans une liste, on
n'en tape pas le nom. Les quatre autres sont **continus** et se donnent en
intervalle, les deux bornes étant facultatives — « ISO ≥ 3200 » est une demande
plus fréquente que « ISO entre 3200 et 6400 ».

Chaque filtre est indépendant, et ils se combinent par **et**. Une photo sans
métadonnée pour un critère filtré n'apparaît pas : elle ne satisfait pas le
critère, et la faire apparaître « par défaut » rendrait tout filtre menteur.

### 2. Le filtre boîtier réutilise la sémantique existante

La correspondance est celle de `SmartRules::camera` — modèle seul ou
`fabricant modèle` — et le code de construction de la clause est **partagé**,
pas recopié. Deux implémentations de la même question finiraient par répondre
différemment, et c'est le genre d'écart qu'on ne découvre que sur un cas
bizarre, longtemps après.

### 3. Les listes de valeurs viennent de la bibliothèque entière

`Catalog::shot_facets()` rend les boîtiers et objectifs présents, ainsi que
les bornes observées pour les quatre grandeurs continues — un `SELECT
DISTINCT` et quelques `MIN`/`MAX` sur des colonnes indexées.

**Sur la bibliothèque entière, pas sur la sélection filtrée en cours.** Un
affinement progressif — où choisir « Canon 60D » retirerait de la liste les
objectifs jamais montés dessus — est plus malin et coûte plus cher : il faut
recalculer chaque facette à chaque changement, en excluant le filtre dont on
calcule la liste. Le gain est réel mais mince à cette échelle, et le
comportement est plus dur à prévoir pour qui l'utilise. À reprendre si l'usage
le réclame ; ce serait une évolution de cette décision, pas une contradiction.

### 4. Ni migration, ni index

Rien à ajouter au schéma. C'est ce qui rend cette tranche petite, et c'est
aussi ce qui explique qu'elle ait attendu si longtemps : rien ne manquait, il
n'y avait donc rien qui la réclamât.

### 5. Les trois clients

La CLI gagne des options sur `ls` (`--camera`, `--lens`, `--iso`,
`--aperture`, `--focal`, `--shutter`, les intervalles s'écrivant `min-max`,
`min-` ou `-max`), le SDK expose les champs de `GridQuery`, et Studio y met un
panneau repliable sous la barre de filtres — pas un onglet latéral : ces
filtres se combinent avec la note et le libellé déjà présents, et les séparer
en deux endroits ferait chercher.

Trois précisions venues de l'implémentation :

* La forme écrite d'un intervalle est **lue par le moteur**
  (`ShotRange::parse`), pas par chaque client : la CLI et Studio prennent la
  même chaîne, donc `1/200-` ne peut pas vouloir dire deux choses. Les bornes
  acceptent les fractions, parce qu'une vitesse s'écrit `1/200` partout
  ailleurs. Une valeur seule vaut pour les deux bornes.
* Un intervalle inversé (`3200-400`) est **refusé**, jamais répondu par une
  grille vide : zéro photo se lit « la bibliothèque n'en contient pas », ce
  qui serait faux.
* La CLI gagne aussi `leyline facets <library>`, qui imprime ce que
  `shot_facets` rend. Sans elle, les listes de §3 n'existeraient que dans
  Studio et un utilisateur de la CLI devrait deviner l'orthographe exacte
  d'un boîtier pour s'en servir — l'inverse de ce que §3 promet.

Dans Studio, boîtiers et objectifs sont des **puces**, comme les libellés et
les drapeaux de la même barre, plutôt qu'une liste déroulante : le nombre de
boîtiers d'une bibliothèque se compte sur les doigts, et une puce montre à la
fois ce qui existe et ce qui est actif. Les quatre grandeurs continues sont
des champs de saisie : les deux bornes étant facultatives, un curseur à deux
poignées devrait inventer une façon de dire « pas de borne du tout ». La puce
qui déplie le panneau porte le nombre de critères actifs, pour qu'une grille
filtrée ne paraisse jamais entière quand le panneau est replié.

## Conséquences

* `GridQuery` gagne six champs. Sa construction reste un chaînage de `AND`
  optionnels, et une requête sans filtre produit exactement le SQL
  d'aujourd'hui.
* Les collections dynamiques ne changent pas, mais partagent désormais leur
  clause boîtier avec la grille.
* Les listes de facettes se recalculent quand la bibliothèque change
  (`AssetsAdded`, `AssetsRemoved`), pas à chaque frappe.
* `docs/catalog.md` gagne la description de `shot_facets`.

## Alternatives écartées

* **Étendre la recherche plein texte** au lieu d'ajouter des filtres : taper
  « 60D » trouverait des photos, mais « ISO entre 800 et 3200 » n'est pas une
  question qu'un index plein texte sait poser, et mélanger les deux rendrait
  imprévisible ce que la barre de recherche fait.
* **Un filtre unique par expression** (`iso>800 AND camera="60D"`) : puissant,
  et il faut l'apprendre. Les listes construites depuis le contenu réel n'ont
  rien à apprendre — on voit ce qu'on a.
* **Des facettes progressives** dès cette version : voir §3.
* **Filtrer côté client, sur les lignes chargées** : la grille est virtuelle,
  seule une fenêtre est en mémoire, et un filtre qui ne verrait que cette
  fenêtre serait faux dès la première photo hors écran.
