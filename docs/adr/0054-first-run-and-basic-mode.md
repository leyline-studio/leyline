# ADR 0054 — Prise en main : une bibliothèque vide qui explique, et un mode Basique par défaut dans develop

**Statut :** Accepté — 2026-07

## Contexte

Un relevé de ce que les utilisateurs reprochent réellement aux dérawtiseurs
libres, sur un fil de discussion de juillet 2026 comparant huit d'entre eux
(darktable, RawTherapee, ART, vkdt, Filmulator, LightZone, RapidRAW, Safelight) :

* « courbe d'apprentissage abrupte » ;
* « je n'arrive tout simplement pas à comprendre » ;
* « beaucoup plus compliqué que Lightroom » ;
* un guide de démarrage darktable posté deux fois dans le même fil.

**Aucun message ne réclame une fonctionnalité manquante.** Le manque du marché
n'est pas fonctionnel, il est d'accès. Et Leyline vient d'aller dans la
direction opposée : la série ADR 0049–0053 a porté la barre d'outils de develop
de trois à six outils et le panneau de réglages à quinze groupes.

Deux moments concrets où Leyline est aujourd'hui muet :

1. **Une bibliothèque vide** affiche une grille vide. Rien ne dit qu'il faut
   importer, ni comment ; la seule indication est un `N: new · B: add photo` en
   gris à dix pixels, en bas du panneau latéral.
2. **La vue develop** ouvre quinze groupes repliés dont les noms — *HSL Mixer*,
   *Color Grading*, *Creative LUT*, *Soft Proof* — ne veulent rien dire pour qui
   débute. Les deux groupes ouverts par défaut (balance des blancs, tonalité)
   sont les bons, mais ils sont noyés dans la liste des treize autres.

**Ce qui n'est pas en cause.** Aucune fonctionnalité n'est retirée, aucun
raccourci ne change, aucun réglage stocké n'est touché. Cette décision ne porte
que sur ce qui est **montré d'abord**.

## Décision

### 1. Une bibliothèque vide dit quoi faire, à l'endroit où on la regarde

Quand la grille ne contient aucune photo, elle affiche à sa place le nom de
l'application, une phrase, et les deux gestes qui font entrer des photos :
importer un dossier, ou surveiller un dossier. Les deux boutons ouvrent les
dialogues qui existent déjà — rien de nouveau derrière.

Le raccourci reste affiché où il est. Un raccourci se découvre après coup ; il
ne remplace pas une porte d'entrée.

### 2. Develop s'ouvre en mode **Basique**, et le mode **Complet** est à un clic

Deux niveaux de dévoilement, choisis par un interrupteur en tête du panneau :

| | Basique (défaut) | Complet |
| :--- | :--- | :--- |
| Groupes | Balance des blancs, Tonalité, Présence, Détail, Géométrie, Historique | les quinze |
| Outils sur l'image | Sélection, Recadrage | les six |

Le partage n'est pas arbitraire : **Basique contient ce qui a un équivalent
évident dans n'importe quel outil photo** — une température, une exposition, un
recadrage. Complet contient ce qui suppose de savoir ce qu'on cherche : masques
locaux, LUT, épreuvage, profil DCP, courbe, TSL, color grading, suppression de
tache, reconstruction des hautes lumières.

Trois propriétés à tenir :

* **le mode ne change aucun rendu.** Un réglage posé en Complet reste actif et
  visible dans son groupe, même si le groupe est masqué en Basique — masquer un
  panneau ne remet rien à zéro. C'est ce qui distingue un dévoilement progressif
  d'un mode dégradé ;
* **il ne se stocke nulle part.** Ni dans une révision, ni dans un preset, ni
  dans un fichier de configuration : c'est de l'état d'interface, il vit le
  temps de la fenêtre, et Rust ne le lit jamais ([ADR 0045](0045-studio-ui-modularisation.md) §2) ;
* **le passage en Complet est réversible et immédiat**, sans dialogue ni
  redémarrage.

### 3. Basique est le défaut, y compris pour qui connaît déjà l'application

Le contraire — se souvenir du dernier mode — demanderait de stocker une
préférence, donc un fichier de configuration que le projet n'a pas, et cette
décision ne justifie pas d'en créer un. Un utilisateur expérimenté clique une
fois par session ; un débutant, lui, n'a pas de « une fois » à donner.

### 4. Hors périmètre

* **Une visite guidée, des bulles d'aide, un assistant de première ouverture.**
  Un logiciel qui a besoin d'être expliqué par-dessus son interface a un
  problème dans son interface.
* **Réorganiser les quinze groupes** ou en fusionner. Peut-être justifié, mais
  c'est une refonte, et elle se déciderait avec des utilisateurs réels plutôt
  qu'avec des suppositions.
* **Un jeu de préréglages livrés** pour démarrer. Ce serait un choix esthétique
  de l'éditeur, que `docs/vision.md` refuse.
* **Traduire la documentation.** Réel, et sans rapport avec l'interface.

## Conséquences

* **Le premier écran cesse d'être vide**, et le deuxième cesse de présenter
  quinze portes fermées à quelqu'un qui en cherche deux.
* **Rien n'est perdu pour l'utilisateur avancé** : un clic, et l'interface est
  exactement celle d'avant cette décision.
* **Aucun réglage, aucun rendu, aucun format stocké ne change.** Cette ADR
  n'ajoute pas une ligne à `settings_json` et ne touche à aucun étage.
* **Le panneau develop gagne un état privé de plus** (`basic-mode`), qui reste
  du côté UI conformément à ADR 0045 §2 — et le test mécanique de cette règle
  (aucun `get_*`/`set_*` correspondant côté Rust) continue de passer.
* **La barre d'outils devient dépendante du mode**, donc l'outil actif doit
  retomber sur Sélection quand on quitte Complet avec un outil que Basique ne
  montre pas — sans quoi un clic sur l'image tracerait un masque invisible.

## Alternatives écartées

* **Ne rien faire, et écrire un guide de démarrage.** C'est la réponse que le
  fil de discussion donne pour darktable, deux fois, et elle prouve le
  problème plutôt qu'elle ne le résout.
* **Masquer les groupes avancés jusqu'à ce qu'ils soient utilisés** (dévoilement
  automatique). Une interface qui change toute seule est plus difficile à
  apprendre qu'une interface stable : on ne retrouve plus ce qu'on a vu hier.
* **Se souvenir du dernier mode dans un fichier de configuration.** §3 : cela
  créerait le premier fichier de préférences du projet pour un interrupteur.
* **Un troisième mode intermédiaire.** Trois niveaux demandent de comprendre le
  découpage avant de choisir, ce qui est exactement le problème traité.
* **Retirer des fonctionnalités de Studio.** Le fil ne se plaint pas de ce que
  les outils font, mais de ce qu'ils montrent d'emblée.
