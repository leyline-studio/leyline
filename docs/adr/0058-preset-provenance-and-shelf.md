# ADR 0058 — Les préréglages : rangés, lisibles avant d'être appliqués, et traçables jusqu'à la photo

**Statut :** Accepté — 2026-08

## Contexte

Un préréglage de développement, chez Leyline aujourd'hui : un nom, un JSON de
réglages partiels, une date. Une liste plate, dans un menu. On peut en créer
un, l'appliquer, le supprimer — et c'est tout. Ni dossier, ni favori, ni
modification : un préréglage qu'on veut corriger se supprime et se recrée.

Lightroom fait mieux sur le rangement : panneau de gauche, dossiers, favoris,
aperçu au survol. C'est une attente légitime dès qu'on dépasse une dizaine de
préréglages, et il n'y a aucune raison de faire moins bien.

**Mais Lightroom a un trou, et il est structurel.** Une fois le préréglage
appliqué, *le lien est perdu* : rien, dans le catalogue, ne dit qu'une photo
a été développée avec *Kodak Gold*. La conséquence est un scénario que tout
photographe connaît et qu'aucun outil ne résout :

> j'ai amélioré mon préréglage après avoir traité 340 photos d'un mariage.
> Lesquelles ?

Chez Leyline, la réponse est à portée de main pour une raison d'architecture,
pas de fonctionnalité : **appliquer un préréglage produit déjà une révision
ordinaire** ([ADR 0014](0014-develop-presets.md), `docs/presets.md` §2). Il
manque seulement que la révision dise *d'où elle vient*.

## Décision

### 1. Les préréglages ont un domicile : un panneau gauche dans develop

[ADR 0054](0054-first-run-and-basic-mode.md) §2 les a sortis du panneau de
droite avec une raison qui tient toujours : **la droite est la modification en
cours, pas la bibliothèque des modifications**. Leur place est donc à gauche,
là où [ADR 0055](0055-library-navigation.md) §2 a déjà mis ce qu'on *choisit*
— dossiers, collections. Develop gagne le même panneau, portant une seule
chose : les préréglages.

Il se replie avec `Tab` comme les autres (ADR 0055 §6).

### 2. Rangés : un niveau de dossiers, et des favoris

Les dossiers sont **une table**, pas un préfixe dans le nom : ils se
renomment, se suppriment (leurs préréglages remontent à la racine, rien n'est
perdu) et peuvent être vides. Un favori est un drapeau, et les favoris
s'affichent en tête.

**Un seul niveau.** Lightroom n'en propose pas davantage, personne ne s'en
plaint, et une profondeur arbitraire ici ne rangerait rien de plus qu'elle ne
compliquerait.

### 3. Lisibles avant d'être appliqués

Le panneau dit **ce que le préréglage change**, en clair — « Exposition +0,35 ·
Contraste +12 · Température 5200 K » — et non pas seulement son nom.

C'est possible parce que nos réglages sont structurés, et c'est exactement ce
que Lightroom ne montre pas : là-bas, un préréglage est une boîte fermée qu'on
ne comprend qu'en l'appliquant, puis en annulant.

### 4. Essayables sans être appliqués

Survoler un préréglage montre la photo courante **avec**, sans rien écrire.
Trois garde-fous, parce qu'un rendu n'est pas gratuit :

* le survol doit durer (un quart de seconde) avant de déclencher quoi que ce
  soit — parcourir une liste ne déclenche rien ;
* **un seul rendu en vol** : le suivant attend, et un résultat qui arrive pour
  un préréglage qu'on ne survole plus est jeté ;
* c'est un aperçu, jamais une révision. Quitter le survol rend la photo telle
  qu'elle est vraiment.

### 5. Traçables : la révision dit de quel préréglage elle vient

Une révision produite par l'application d'un préréglage enregistre **lequel**,
et **dans quelle version** de ce préréglage.

**Dans le catalogue, pas dans `settings_json`.** C'est le point sensible de
cette décision : `settings_json` est le contrat de *rendu*, et
[ADR 0043](0043-collapse-prerelease-render-history.md) a montré ce que coûte
un champ étranger dans ce document. Une provenance n'est pas une entrée du
pipeline : deux photos aux mêmes réglages doivent rendre les mêmes pixels,
qu'elles viennent d'un préréglage ou de douze curseurs déplacés à la main.
Elle vit donc dans deux colonnes de `develop_revisions`, nullables, que le
moteur de rendu ne lit jamais.

### 6. Un préréglage a une version, et se met à jour

Aujourd'hui on ne peut que créer et supprimer. Un préréglage devient
modifiable, et chaque modification **incrémente un compteur**.

Dès lors, deux questions ont une réponse — les deux que Lightroom ne sait pas
poser :

* « quelles photos ont été développées avec *Kodak Gold* ? » ;
* « lesquelles l'ont été avec une version antérieure à l'actuelle ? »

Et la réponse à « repasse-le sur celles-là » est **un lot de révisions
ordinaires** : annulables une par une, visibles dans l'historique, comme tout
le reste.

### 7. Rien ne se met à jour tout seul

Modifier un préréglage ne touche **aucune** révision existante — c'est déjà la
règle de `docs/presets.md` §2, et cette décision ne l'écorne pas. Les photos
déjà développées gardent leurs pixels ; le catalogue sait seulement dire
qu'elles ont été faites avec une version antérieure, et l'utilisateur décide.
Un préréglage qui changerait une photo sans qu'on l'ait demandé serait
l'inverse exact de la promesse du projet.

### 8. Le catalogue migre, il ne se réimporte pas

Deux colonnes et deux tables en plus : c'est **additif**, et le mécanisme de
migration incrémentale (`docs/catalog.md` §34) est fait pour ça. Rien à voir
avec ADR 0043, qui changeait la forme d'un *rendu stocké* et n'avait donc rien
à migrer de sensé. Ici une bibliothèque existante s'ouvre et continue.

## Hors périmètre

* **Des préréglages livrés avec l'application.** `docs/vision.md` refuse que
  l'éditeur impose un goût, et ADR 0054 §4 l'avait déjà écarté.
* **Le dosage d'un préréglage** (le curseur *Amount* de Lightroom).
  Interpoler une carte de réglages partiels n'a pas de sens pour un booléen,
  un chemin de fichier ou un masque ; il faudrait décider *quoi* se dose, ce
  qui est une décision à part entière.
* **Importer les préréglages Lightroom** (`.xmp`). C'est le levier d'adoption
  le plus évident de tout ce document, et c'est précisément pour ça qu'il ne
  se traite pas en passant : traduire les noms de paramètres d'Adobe vers les
  nôtres est une promesse de compatibilité, avec ses cas où l'équivalent
  n'existe pas. Sa propre ADR.
* **Les dossiers imbriqués** (§2).
* **Partager un préréglage entre bibliothèques** : le format JSON est déjà
  autonome (`docs/presets.md` §2), mais l'import/export de fichiers est un
  sujet de distribution, pas de rangement.

## Conséquences

* **Le catalogue passe en schéma 2** : `preset_folders`, trois colonnes sur
  `develop_presets` (dossier, favori, version) et deux sur
  `develop_revisions` (préréglage d'origine, version de ce préréglage).
  `docs/catalog.md` est mis à jour.
* **Develop gagne un panneau gauche**, qui n'existait pas — et avec lui la
  place où poser, plus tard, ce qui se *choisit* plutôt que ce qui se règle.
* **La CLI gagne deux commandes** pour ne pas rester en retrait de Studio
  (ADR 0011) : mettre à jour un préréglage depuis une photo, et le repasser
  sur les photos qui en portent une version antérieure.
* **Aucun pixel ne change.** Aucun étage, aucune version d'étage, aucune ligne
  de `settings_json` : la promesse de reproductibilité est intacte, et c'est
  la condition qui rendait cette décision acceptable.

## Alternatives écartées

* **Mettre la provenance dans `settings_json`.** Le document du rendu doit
  rester le document du rendu (§5). Un champ de plus, et l'on retrouve
  exactement la situation qu'ADR 0043 a dû nettoyer.
* **Une table de liaison `révision ↔ préréglage`.** Plus normalisée, mais une
  révision est *un* commit : appliquer deux préréglages fait deux révisions.
  Deux colonnes disent la même chose sans jointure.
* **Ré-appliquer automatiquement un préréglage modifié** à toutes ses photos
  (« presets dynamiques »). Séduisant et faux : la révision d'hier deviendrait
  différente aujourd'hui sans que personne ne l'ait demandé (§7).
* **Ranger par préfixe dans le nom** (`Film / Kodak Gold`), au lieu d'une
  table de dossiers. Gratuit à écrire, et le renommage d'un dossier devient
  une réécriture de N noms, l'ordre dépend de la ponctuation, et un dossier
  vide n'existe pas.
* **Aperçu au survol sans délai ni file.** Une liste de cinquante préréglages
  parcourue au curseur lancerait cinquante rendus (§4).
