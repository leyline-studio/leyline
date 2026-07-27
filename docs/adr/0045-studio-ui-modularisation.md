# ADR 0045 — Modularisation de l'interface Studio : globals par domaine, un fichier par panneau

**Statut :** Accepté — 2026-07

## Contexte

`crates/leyline-studio` est le seul endroit du dépôt où la règle « une
responsabilité par unité » de `docs/contributing.md` n'a jamais été tenue.
Deux fichiers portent toute l'interface :

* `ui/studio.slint` — 4425 lignes, dont ~4000 pour le seul composant
  `StudioWindow`, qui déclare **111 propriétés et 73 callbacks à plat** avant
  d'imbriquer, dans un unique `FocusScope`, la vue Develop, la vue Carte, la
  grille, le panneau de détails, dix dialogues modaux et la barre de menus.
* `src/main.rs` — 3820 lignes : douze fonctions `wire_*` (dont `wire_dialogs`
  à elle seule 472 lignes), le pompage d'événements, le fenêtrage de la
  grille, la construction des modèles et les helpers de formatage.

Ce n'est pas un problème esthétique. Trois coûts sont mesurables :

1. **Aucune frontière ne dit qui a le droit d'écrire quoi.** Les 111
   propriétés sont visibles depuis n'importe quel point de l'arbre ; rien
   n'empêche le dialogue d'export de lire l'état du pinceau de retouche.
   Chaque tranche livrée depuis Phase 5 a donc élargi la même surface plate.
2. **Le coût d'une modification croît avec le fichier, pas avec la
   modification.** Ajouter un réglage Develop impose de traverser un fichier
   de 4425 lignes pour trouver ses trois points d'ancrage, et la moindre
   erreur d'accolade se diagnostique sur l'ensemble.
3. **La contribution externe est bloquée par la taille.** C'est le point qui
   force la décision *maintenant* : l'ouverture publique de la V1 invite des
   contributeurs qui n'ont pas l'historique du projet, et un premier patch
   d'interface ne devrait pas demander de lire 8000 lignes.

Le gel des ADR démarre à la publication ([[adr-editable-before-release]]) :
c'est donc la dernière fenêtre où réorganiser librement la surface UI sans
devoir superséder quoi que ce soit.

## Décision

**L'état de l'interface est porté par des `global` Slint découpés par
domaine ; chaque panneau et chaque dialogue devient un fichier autonome qui
lit ces globals directement ; les modules Rust de câblage sont le miroir
exact de cette découpe.**

### 1. Un `global` Slint par domaine, pas de propriétés sur `StudioWindow`

Slint offre deux façons de faire descendre l'état vers un composant extrait :
le plomber en `in`/`out`/`callback` sur chaque composant, ou l'exposer dans un
`global` que le composant lit sans intermédiaire. **Le projet choisit les
globals**, pour une raison qui tient à la nature du programme : Studio n'est
pas une bibliothèque de composants réutilisables, c'est une application unique
dont les panneaux sont singletons. Un `DevelopPanel` n'existera jamais deux
fois, avec deux états distincts, dans deux fenêtres. Payer 600 à 800 lignes de
plomberie de bindings pour une réutilisabilité qui n'arrivera pas serait un
coût sans contrepartie — et cette plomberie vivrait précisément dans
`StudioWindow`, c'est-à-dire que le monolithe qu'on cherche à démonter
resterait gros.

Les domaines, et leur contenu :

| Global | Porte |
|---|---|
| `LibraryState` | nom/chemin de la bibliothèque, version applicative, ligne de statut, bibliothèques récentes, ouverture/relance, quitter |
| `GridState` | fenêtre de cellules chargée, total, index sélectionné, clic cellule, viewport, recherche, tri, classement (`classify`) |
| `FilterState` | filtres note/label/pick/mot-clé et leurs bascules |
| `DetailState` | métadonnées de la photo sélectionnée, mots-clés et leur ajout/retrait |
| `DevelopState` | mode develop, images (rendu et « avant »), `DevSettings`, histogramme, courbe tonale, taches, presets, historique de révisions, tous les `develop-*` |
| `MapState` | mode carte, image rendue, épingles, pack de tuiles, pan/zoom |
| `DialogState` | dialogue ouvert et son résultat, et l'état de saisie des dix dialogues (import, export, impression, tethering, surveillance, collection, preset) |
| `CollectionState` | collections, collection active, appartenance |

`StudioWindow` ne conserve **aucune propriété métier**. Il lui reste ses
attributs de fenêtre (titre, icône, tailles minimales), le `FocusScope` des
raccourcis clavier, et l'assemblage des panneaux.

### 2. L'état purement local reste dans le panneau, jamais dans un global

La contrepartie du choix ci-dessus est que le global est une variable
partagée : tout ce qu'on y met devient visible de partout. La règle qui borne
cet effet est simple et non négociable :

> **Un global ne porte que l'état qui traverse la frontière Rust ↔ UI. L'état
> qui ne concerne qu'un panneau reste une propriété privée de ce panneau.**

Concrètement, restent locaux — et disparaissent donc de la surface partagée —
les onze booléens `expand-*` d'accordéon du panneau Develop, `active-tool`,
`dev-zoomed` (et le `changed develop-image` qui le remet à zéro),
`spot-pending*`, `hsl-band-names`, ainsi que `open-menu` et les cinq
`*-submenu-open` de la barre de menus. C'est un tiers des propriétés
actuelles de `StudioWindow` qui cesse d'être global au lieu de le devenir.

Quand `StudioWindow` a malgré tout besoin d'agir sur cet état — le
`FocusScope` ferme les menus sur Échap, et les flèches doivent faire défiler
la grille — il passe par la surface publique du panneau (`menubar.open-menu`,
`public function select-cell(int)` exposée par le panneau grille) plutôt que
par un global. La lecture d'une propriété d'enfant est du Slint standard ; ce
qui change, c'est que le panneau choisit ce qu'il expose.

### 3. Arborescence

```
ui/
  studio.slint            assemblage + FocusScope, ~250 lignes
  types.slint             structs (Cell, DevSettings, …) + global Tr
  state/                  un fichier par global du §1
  widgets/                DetailRow, GroupHeader, FilterChip, EditSlider,
                          TopMenuLabel, MenuDropdown, MenuRow, MenuSep, …
  panels/                 develop, map, browser (grille + détails), menubar
  dialogs/                import, export, print, tether, watch, collection,
                          preset, shortcuts, about
```

`types.slint` reste séparé de `state/` parce que les structs sont importées
par le Rust généré autant que par l'UI, alors qu'un global est un point
d'accès à l'exécution : ce ne sont pas les mêmes objets, et les mélanger
recréerait un fichier fourre-tout.

### 4. Les modules Rust sont le miroir des panneaux

`main.rs` est réduit à `main()`/`run()` et au bootstrap. Le reste se répartit
en `src/wiring/{grid,filters,develop,dialogs,collections,keywords,presets,map}.rs`
— un module par global, câblant ce global et lui seul via
`Global::<T>::get(&window)` (le mécanisme déjà utilisé pour `Tr`) — plus
`src/{app,library,events,models,details}.rs` pour l'état applicatif, les
chemins de bibliothèque, le pompage d'événements, la conversion vers les
modèles Slint et le panneau de détails.

Les modules de logique pure existants (`develop.rs`, `map_view.rs`,
`format.rs`, `classify.rs`) ne bougent pas : ils sont déjà à une
responsabilité, et leur qualité première — ne connaître aucun type Slint,
donc être testables unitairement — est exactement ce que `wiring/` ne peut
pas avoir. La proximité de nom entre `src/develop.rs` (règles de réglage,
sans Slint) et `src/wiring/develop.rs` (branchement du global) est
volontaire : elle rend visible la séparation logique/câblage.

### 5. Refactorisation à comportement constant, par tranches livrables

Aucun changement de comportement, aucun changement de rendu, aucune nouvelle
fonctionnalité dans ce chantier — le seul livrable est la structure. L'ordre
est contraint par les dépendances : types et widgets d'abord (aucun appelant
à changer), puis les globals (c'est la tranche qui touche le Rust en masse),
puis les panneaux (qui deviennent alors de simples déplacements de blocs),
puis la découpe de `main.rs`.

Chaque tranche compile, passe `cargo fmt`/`clippy -D warnings`/`cargo test
--workspace`, et est committée séparément.

### 6. La frontière SDK ne bouge pas : Studio reste un client tiers parmi d'autres

Studio n'a aucun privilège d'accès au moteur. Il consomme `leyline-sdk`
exactement comme le ferait un produit tiers, au même titre que la CLI :
`crates/leyline-studio/Cargo.toml` ne déclare qu'une seule dépendance
Leyline, et tout le code de `src/` ne référence que `leyline_sdk`. C'est ce
qui rend la façade honnête — si Studio avait besoin d'une porte dérobée, ce
serait le signe qu'il manque quelque chose à l'API publique, et le correctif
serait d'élargir le SDK ([[sdk-is-a-pure-facade]]), pas de contourner.

Ce chantier est donc **strictement en amont de cette frontière** : il
réorganise l'interface et son câblage, il ne touche ni à `leyline-sdk` ni à
ce que Studio lui demande. En particulier, aucun module de `wiring/` ne peut
ajouter une dépendance sur `leyline-engine`, `leyline-catalog` ou tout autre
crate interne — la tentation existe au moment où l'on découpe (« ce module-là
n'aurait besoin que d'un type de `leyline-core` »), et elle transformerait un
refactor d'interface en régression d'architecture. Le `Cargo.toml` à une
seule dépendance Leyline est le garde-fou : l'enfreindre demande une ligne
visible en revue.

Si le découpage révèle un manque réel dans l'API — un besoin auquel Studio
répondait par un détour — c'est une décision SDK à traiter séparément, avec
son propre commit, et pas dans une tranche de modularisation.

### 7. Le `.pot` est régénéré à la fin, pas au fil de l'eau

`slint-tr-extractor` inscrit dans le catalogue le fichier et la ligne de
chaque `@tr(...)`. Déplacer 4000 lignes invalide donc toutes les références,
en silence ([[i18n-extraction-workflow]]). L'extraction est relancée une fois
la nouvelle arborescence stabilisée ; les chaînes elles-mêmes étant
inchangées, aucune traduction n'est perdue.

## Conséquences

* `StudioWindow` passe de ~4000 à ~250 lignes ; aucun fichier d'interface ne
  dépasse quelques centaines de lignes. Un contributeur qui veut corriger le
  dialogue d'impression ouvre `ui/dialogs/print.slint`.
* La surface partagée devient explicite et bornée : ce qui est dans `state/`
  traverse la frontière Rust, ce qui n'y est pas ne la traverse pas. C'est
  une propriété vérifiable par lecture, ce que les 111 propriétés à plat ne
  permettaient pas.
* Le coût est réel et assumé : les panneaux ne sont pas réutilisables hors de
  Studio, puisqu'ils lisent des singletons. C'est acceptable parce qu'ils ne
  sont pas destinés à l'être (§1) ; si un composant devait un jour être
  instancié deux fois, il basculerait sur des propriétés `in`/`out` — la
  bascule est locale au composant concerné, pas structurelle.
* Le diff de la tranche « globals » est large et mécanique (chaque
  `window.set_x(…)` devient `window.global::<T>().set_x(…)`). Il est
  intégralement couvert par la compilation : un accès resté sur l'ancienne
  surface ne compile pas.
* La frontière SDK sort du chantier telle qu'elle y est entrée : une seule
  dépendance Leyline dans `crates/leyline-studio/Cargo.toml`, `leyline_sdk`
  comme seul chemin d'import dans `src/`. La propriété se vérifie en deux
  greps, avant et après.
* `docs/architecture.md` gagne la description de l'arborescence `ui/`, et
  `docs/contributing.md` la règle du §2, qui est celle qu'un contributeur
  peut enfreindre sans s'en rendre compte.

## Alternatives écartées

* **Plomber des propriétés `in`/`out` sur chaque panneau.** L'option
  orthodoxe, et la bonne pour une bibliothèque de composants. Ici elle coûte
  600 à 800 lignes de bindings, laisse `StudioWindow` volumineux, et achète
  une réutilisabilité dont l'application n'a aucun usage prévu. Retenue comme
  porte de sortie ponctuelle (§Conséquences), pas comme règle.
* **Un `global` unique `AppState`.** Un seul fichier au lieu de huit, mais
  c'est le monolithe déplacé plutôt que démonté : la surface reste plate et
  la question « qui a le droit d'écrire quoi » reste sans réponse.
* **Découper les fichiers sans introduire de globals**, en s'appuyant sur la
  visibilité de `root` depuis les composants imbriqués. Slint ne le permet
  pas au-delà d'un fichier : un composant importé n'a aucun accès à la portée
  de son instanciateur. L'option n'existe pas techniquement.
* **Ne découper que le Slint et laisser `main.rs` à 3820 lignes.** La moitié
  du couplage subsisterait, et la découpe ultérieure du Rust rouvrirait
  exactement les mêmes zones — deux passages là où un suffit.
* **Reporter après la publication V1.** C'est précisément l'inverse du besoin
  qui motive le chantier : la structure doit être en place *avant* que des
  contributeurs extérieurs écrivent du code dessus, sans quoi ils écriront
  dans le monolithe et la dette augmentera pendant la refonte.
* **En profiter pour retoucher l'ergonomie ou le style visuel.** Mélanger un
  déplacement massif de code avec des changements de comportement rendrait
  toute régression indiscernable d'un choix délibéré. Le §5 l'interdit
  explicitement.
