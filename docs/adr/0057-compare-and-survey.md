# ADR 0057 — Départager deux photos : vue Comparaison à zoom lié, et vue Mosaïque

**Statut :** Accepté — 2026-08

## Contexte

[ADR 0055](0055-library-navigation.md) §3 a livré la grille et la loupe, et a
laissé de côté les deux autres dispositions de Lightroom en renvoyant leur
décision ici. La raison de les traiter maintenant est un geste précis, que
Leyline ne sait pas faire aujourd'hui :

> deux photos de la même scène, prises à une seconde d'écart. Laquelle est
> nette ? Sur quel œil ? À l'écran entier, aucune des deux ne le dit ; il faut
> les voir **au même endroit, au même agrandissement, en même temps**.

Aujourd'hui, cela demande d'entrer dans la loupe, de mémoriser, de revenir, de
naviguer, de re-agrandir. C'est exactement le travail que l'ordinateur devrait
faire. Et c'est le geste le plus fréquent d'une séance de tri, celle qui
précède tout développement.

**Ce qui est déjà là et qui n'est pas en cause** : la multi-sélection
(Ctrl/Maj-clic) existe et alimente déjà les actions par lot ; les drapeaux, les
étoiles et les libellés existent ; la loupe existe. Ce qui manque est une
manière de *regarder*, pas une manière de classer.

## Décision

### 1. Deux dispositions de plus, du même genre que la loupe

**Comparaison** (`C`) montre deux photos côte à côte ; **Mosaïque** (`N`)
montre toute la sélection à la fois. Comme la loupe, et pour la même raison :

* **aucune session d'édition n'est ouverte** — ce sont les previews en cache,
  celles que la loupe et develop utilisent déjà, donc rien n'est rendu deux
  fois ;
* **rien n'est écrit** : aucune révision, aucun historique, aucune préférence.
  L'état vit le temps de la fenêtre ([ADR 0045](0045-studio-ui-modularisation.md) §2) ;
* `G` ramène à la grille, les touches de classement (1-5, 6-9, P/X/U)
  continuent d'agir sur la photo courante.

### 2. Le zoom et le déplacement sont **liés**, en coordonnées d'image

C'est la décision qui fait exister cette ADR. Deux loupes indépendantes côte à
côte ne servent à rien : ce qu'on compare, c'est le *même endroit* des deux
images.

Le lien est exprimé en **coordonnées normalisées de l'image** — le point
regardé est « à 62 % de la largeur, 41 % de la hauteur » — et non en pixels
d'écran ni en pixels d'image. C'est ce qui fait que deux photos de dimensions
différentes (un recadrage et son original, un RAW et son JPEG) restent sur la
même zone. Un facteur d'agrandissement unique s'applique aux deux.

Deux niveaux, comme la loupe de develop : **ajusté** et **100 %**. Pas de zoom
continu : le geste qu'on sert ici est « montre-moi les pixels », et un curseur
de zoom en est une version plus lente.

### 3. Gauche = la retenue, droite = la candidate

En Comparaison, la photo de gauche est celle qui est sélectionnée ; celle de
droite est sa voisine, et les flèches ne déplacent **que la candidate**. Une
touche les échange : la candidate devient la retenue, et le tri avance.

C'est le modèle de Lightroom (*Select* / *Candidate*), et il vaut mieux que
« les deux photos sélectionnées » : il donne au geste une direction — on
défend un tenant du titre contre des challengers — au lieu de demander deux
sélections avant de pouvoir regarder quoi que ce soit.

### 4. La mosaïque montre la sélection, et sert à la réduire

`N` affiche côte à côte les photos **multi-sélectionnées**, ou la seule photo
sélectionnée s'il n'y en a qu'une. Cliquer la croix d'une vignette la **retire
de la sélection** sans rien supprimer : c'est un entonnoir, on part de douze
photos et on en garde deux.

C'est le seul endroit de cette décision qui écrit quelque chose — et il
n'écrit que dans la sélection, qui n'est pas stockée.

### 5. Ce que ces vues ne font pas

* **Elles ne développent pas.** Aucun réglage n'y est modifiable ; pour cela il
  y a develop, à une touche.
* **Elles ne comparent pas un avant/après.** Ça, c'est `\` dans develop
  (la vue Compare Before/After), et
  c'est une autre question : le même fichier à deux états, pas deux fichiers.
* **Elles ne montrent pas plus de deux photos en Comparaison.** Trois vues à
  zoom lié tiennent difficilement sur un écran, et la mosaïque couvre le cas
  « plusieurs ».

## Hors périmètre

* **Le zoom continu** (molette progressive, curseur) : §2.
* **Un mode plein écran sans barre de menus** : ADR 0055 §6 dit pourquoi la
  barre reste.
* **Comparer deux *versions* d'une même photo** côte à côte. Le modèle de
  données le permettrait (une version est l'unité, [ADR 0008](0008-version-as-library-unit.md)),
  mais c'est une entrée par le catalogue et non par la sélection ; à décider
  pour elle-même.
* **Synchroniser le classement entre les deux vues** (noter la gauche note
  aussi la droite). Non : c'est précisément ce qu'on cherche à distinguer.

## Conséquences

* **Le tri devient faisable dans Leyline** sans sortir de l'application ni
  ouvrir deux fenêtres.
* **Aucune écriture nouvelle**, aucun accès catalogue nouveau : les deux vues
  lisent les previews déjà en cache et la sélection déjà en mémoire.
* **Un composant d'affichage partagé** apparaît (image agrandissable et
  déplaçable, à zoom piloté de l'extérieur) ; la loupe d'ADR 0055 le reprend et
  gagne donc le zoom qu'elle n'avait pas, sans code en double.
* **Deux raccourcis de plus** (`C`, `N`), identiques à ceux de Lightroom. `N`
  n'était pas libre : il ouvrait « nouvelle collection », qui passe à `Ctrl+N`
  — le même arbitrage que `E`/`Ctrl+E` dans [ADR 0055](0055-library-navigation.md) §4,
  et pour la même raison : la touche de *vue* est celle qu'on presse sans y
  penser, l'action délibérée a déjà un menu et un bouton. Le dialogue des
  raccourcis, le menu Bibliothèque et le menu Affichage le disent.

## Alternatives écartées

* **Deux loupes indépendantes côte à côte.** C'est ce qu'on obtient sans §2, et
  ça ne répond pas à la question posée.
* **Lier le déplacement en pixels d'image.** Deux photos de tailles différentes
  se désalignent immédiatement ; les coordonnées normalisées sont ce qui rend
  la comparaison entre un original et son recadrage encore lisible.
* **Comparer les deux dernières photos sélectionnées**, sans notion de retenue
  ni de candidate. Il faut alors deux sélections avant de voir quoi que ce
  soit, et rien ne dit laquelle on est en train de défendre.
* **Faire de la mosaïque une grille filtrée** (« ne montrer que la sélection »
  dans la barre de filtres) plutôt qu'une vue. Ce serait un filtre de plus dans
  une barre qui en a déjà six, et il faudrait le retirer à la main après coup.
