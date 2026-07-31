# ADR 0055 — Navigation de la bibliothèque : les repères qu'un utilisateur de Lightroom cherche en arrivant

**Statut :** Accepté — 2026-08

## Contexte

[ADR 0054](0054-first-run-and-basic-mode.md) a traité ce que develop **montre
d'abord**. Le même relevé d'utilisateurs portait sur un second manque, qui est
d'orientation et non de fonctionnalité : savoir *où l'on est* et *où cliquer*.

Une comparaison poste à poste avec la vue Bibliothèque de Lightroom Classic
donne l'état exact de l'écart. Ce que Leyline a déjà, et qui n'est pas en
cause : la barre de menus complète, les menus contextuels
([ADR 0021](0021-context-menus.md)), les raccourcis de classement identiques
(1-5, 6-9, P/X/U), `G`/`D`/`M` pour changer de vue, les filtres (étoiles,
labels, drapeaux, recherche, tri), les collections, le double-clic vers
develop, le filmstrip en develop.

Ce qui manque, dans l'ordre où cela désoriente :

1. **Aucun sélecteur de module.** Passer de la bibliothèque à develop se fait
   par un menu ou une touche. Le coin haut-droit, là où un utilisateur de
   Lightroom pose les yeux et le curseur en arrivant, est vide.
2. **Le panneau gauche ne porte que les collections.** Pas d'arbre de dossiers
   — or c'est la façon dont la plupart des gens se représentent leurs photos,
   et le catalogue a déjà tout ce qu'il faut pour l'afficher (table `folders`
   avec son `parent_id`, `GridQuery.folder`).
3. **Aucune barre d'outils sous la grille.** Les vignettes sont figées à
   176 px (`cell-size`, `panels/browser.slint`), et voir une photo en grand
   oblige à entrer dans develop, donc à ouvrir une session d'édition.
4. **Les cellules ne disent presque rien** : nom, pastille de couleur,
   étoiles. Ni numéro, ni drapeau, ni « celle-ci a déjà été travaillée ».

**Ce qui n'est pas en cause.** Aucune fonctionnalité ne manque : tout ce qui
précède est de la mise en place de ce qui existe déjà. Aucun réglage, aucun
rendu, aucun format stocké n'est touché par cette décision.

## Décision

### 1. Un sélecteur de module à droite de la barre de menus

**Bibliothèque | Develop | Carte**, aligné à droite dans la barre de menus
existante — pas une seconde barre : ce côté-là est vide aujourd'hui, et une
navigation ne mérite pas qu'on lui donne une rangée de pixels de plus. Le
module courant est mis en évidence ; *Develop* est inactif sans photo
sélectionnée, exactement comme l'est déjà l'entrée de menu du même nom.

**Imprimer n'y figure pas.** Un sélecteur nomme des *lieux* où l'on reste ;
imprimer est une *action* qui se termine — elle reste un dialogue, sous
`Ctrl+P` et dans le menu Fichier, où un utilisateur de Lightroom la trouve
aussi.

### 2. Le panneau gauche porte les dossiers

De haut en bas : **Toutes les photos**, l'**arbre des dossiers** avec le
nombre de photos de chacun, puis les **collections**. Cliquer un dossier
filtre la grille (`GridQuery.folder`, qui existe déjà) ; cliquer *Toutes les
photos* enlève le filtre.

L'arbre est **en lecture seule** : on n'y renomme, ne déplace ni ne supprime
rien. Déplacer un dossier est de la gestion de fichiers, pas du catalogue, et
Leyline ne modifie jamais ce qu'il n'a pas écrit (`docs/vision.md`).

Le catalogue gagne pour cela une seule lecture nouvelle : lister les dossiers
avec leur parent et leur compte. Aucun changement de schéma.

En bas du panneau, les deux boutons **Importer…** et **Exporter…**, à
l'endroit où Lightroom les met, ouvrant les dialogues existants. La ligne
grise de raccourcis (`N: new · B: add photo`) disparaît : ADR 0054 §1 disait
qu'un raccourci ne remplace pas une porte d'entrée ; c'est vrai aussi quand la
bibliothèque n'est pas vide.

**« Import précédent » n'est pas repris.** Le catalogue n'a pas d'identité de
lot d'import — seulement un `imported_at` par photo — et déduire un lot d'un
horodatage serait une devinette qui se tromperait le jour où deux imports se
suivent. Le tri « par date d'import », qui existe, couvre le besoin réel. Lui
donner une vraie identité serait une décision de catalogue, à prendre pour
elle-même.

### 3. Une barre d'outils sous la grille

Elle porte deux choses, et pas une de plus :

* un **curseur de taille de vignette**, parce qu'une planche-contact de 400
  photos et une relecture de trois cadrages ne se regardent pas à la même
  taille ;
* deux **modes de vue** : *Grille* et *Loupe*. La loupe montre la photo
  sélectionnée en grand **sans ouvrir de session d'édition** : c'est la
  preview, pas develop. La différence est réelle et vaut d'être tenue — aucune
  révision créée, aucun historique, rien à écrire, et donc un affichage
  immédiat. Les flèches y naviguent, `G` revient à la grille.

*Comparaison* (C) et *mosaïque* (N) sont hors périmètre : ce sont deux
dispositions de plus, à décider quand quelqu'un les réclamera.

Le tri et les filtres restent en haut, où ils sont : les répéter en bas serait
deux endroits pour un réglage.

### 4. `E` ouvre la loupe, l'export passe à `Ctrl+E`

C'est le seul raccourci existant que cette décision déplace, et elle le fait
délibérément. `G`, `E` et `D` sont les trois touches qu'un utilisateur de
Lightroom presse sans y penser ; `G` et `D` font déjà chez nous ce qu'il
attend, `E` est la dernière qui manque. L'export, lui, est une action
délibérée qu'on atteint par le menu Fichier, par le bouton du panneau gauche
(§2) et par `Ctrl+E` — trois portes plutôt qu'une lettre.

Le dialogue des raccourcis, le menu Fichier et `docs/` sont mis à jour en même
temps : un raccourci qui change sans que l'aide le dise est un bug.

### 5. Le filmstrip est partagé, et les cellules disent l'essentiel

Le filmstrip devient un composant unique, affiché en **loupe** et en
**develop**. Pas en grille : il y répéterait la grille elle-même.

Les cellules gagnent trois repères, tous déjà connus du catalogue ou à une
lecture près :

* le **numéro d'index** dans la grille, comme Lightroom — c'est ce qui permet
  de dire « la 47 » à quelqu'un ;
* le **drapeau** (retenue / rejetée), qui est déjà dans `GridItem` et
  n'était simplement pas affiché ;
* un badge **« déjà développée »** : la version porte plus que sa révision
  initiale. C'est le seul champ nouveau, en lecture, calculé par la même
  requête que la grille.

Étoiles et pastille de couleur restent où elles sont.

### 6. Trois propriétés à tenir

* **rien de tout cela ne se stocke** : module courant, mode de vue, taille de
  vignette et dossier sélectionné sont de l'état d'interface, ils vivent le
  temps de la fenêtre et Rust ne les lit jamais
  ([ADR 0045](0045-studio-ui-modularisation.md) §2) ;
* **aucun réglage, aucun rendu, aucun format stocké ne change** : la seule
  évolution de surface du catalogue est en lecture ;
* **rien n'est retiré** : chaque geste qui existait avant existe après, au même
  endroit ou avec une porte de plus.

## Hors périmètre

* **Le repli des panneaux** (`Tab` / `Maj+Tab` chez Lightroom). Utile, sans
  rapport avec l'orientation d'un nouvel arrivant, et à décider avec le reste
  d'une éventuelle gestion des espaces de travail.
* **Le Navigateur** (l'aperçu en haut du panneau gauche). Il ne sert vraiment
  qu'à se déplacer dans une image zoomée au-delà de 100 % ; la grille et la
  loupe montrent déjà la photo.
* **Renommer, déplacer ou supprimer des dossiers** depuis l'arbre (§2).
* **Une identité de lot d'import** (§2).
* **Cloner l'interface au pixel.** L'objectif est qu'un utilisateur de
  Lightroom sache où cliquer, pas qu'il croie avoir lancé Lightroom.

## Conséquences

* **Le premier écran d'un migrant a ses trois repères** : où changer de
  module, où sont ses dossiers, comment grossir les vignettes.
* **Le catalogue gagne deux lectures** — la liste des dossiers avec leurs
  comptes, et un booléen « développée » par ligne de grille —, aucune
  écriture, aucun changement de schéma. `docs/catalog.md` est mis à jour.
* **Un raccourci change** (`E`), avec sa documentation dans le même geste (§4).
* **La grille gagne trois états d'interface** (mode de vue, taille de
  vignette, dossier courant) qui restent du côté UI, et le test mécanique
  d'ADR 0045 §2 continue de passer.
* **La ligne de raccourcis grise du panneau gauche disparaît**, remplacée par
  deux boutons.

## Alternatives écartées

* **Ne rien changer et écrire un guide.** Même réponse qu'ADR 0054 : c'est ce
  qu'on reproche aux autres.
* **Un mode « raccourcis Lightroom » optionnel.** Deux jeux à maintenir, et un
  choix à faire avant d'avoir de quoi choisir.
* **Mettre *Imprimer* dans le sélecteur de module** pour coller à Lightroom.
  Chez eux, Imprimer *est* un module avec ses panneaux ; chez nous c'est un
  dialogue ([ADR 0036](0036-print-module.md)). Un sélecteur qui ouvre une
  fenêtre modale ment sur ce qu'il est.
* **Garder `E` pour l'export et poser la loupe ailleurs.** Toute autre touche
  est une touche qu'il faut apprendre — ce que cette décision cherche
  précisément à éviter.
* **Afficher le filmstrip aussi en grille**, comme Lightroom. Il y montrerait
  les mêmes vignettes que la grille, deux fois, dans deux tailles.
