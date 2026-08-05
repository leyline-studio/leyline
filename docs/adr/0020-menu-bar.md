# ADR 0020 — Barre de menu comme surface de commandes

**Statut :** Accepté — 2026-07

## Contexte

Leyline Studio est aujourd'hui entièrement piloté par souris et raccourcis clavier : `G`/`D` (bibliothèque/développement), `I`/`E`/`N`/`B`/`Shift+B` (import, export, collection), `R`/`Shift+R` (retraitement, ADR 0016–0018 §10.4), `S` (sauver un preset), `Ctrl+Z`/`Ctrl+Y` (undo/redo), chiffres/`P`/`X`/`U`/lettres de couleur (classement). Aucune fenêtre ni menu n'expose cette liste ailleurs qu'un fragment en une ligne visible seulement en mode développement (`studio.slint`) — rien en mode bibliothèque. Un nouvel utilisateur n'a aucun moyen de découvrir `Shift+B` ou `Shift+R` sans lire la documentation.

ADR 0019 ajoute par ailleurs des Préférences (choix de langue) et n'a pas d'endroit naturel où les loger : pas de menu, pas de fenêtre de réglages existante.

## Décision

Leyline Studio adopte une **barre de menu** (`File` / `Library` / `Photo` / `Develop` / `View` / `Help`) via le composant `MenuBar` natif de Slint — rendue comme la vraie barre système sur macOS, comme une barre dans la fenêtre sur Windows/Linux. Chaque entrée affiche son raccourci existant à côté du libellé ; la barre de menu **n'ajoute aucune fonctionnalité nouvelle**, elle rend visible et cliquable ce qui n'existe aujourd'hui que par raccourci — mêmes callbacks Rust déjà câblés (`on_run_import`, `on_develop_undo`, `on_reprocess_library`, etc.), pas de nouveau chemin de code.

> **Correction, 2026-08-05.** Le moyen a changé, la décision non. Le `MenuBar`
> natif de Slint entraîne l'intégration de menus au niveau OS (`muda`), qui
> **casse l'ordonnancement des repaints** de la grille et de la liste des
> collections : la fenêtre cessait de se redessiner tant qu'on ne la
> survolait pas. La barre est donc **écrite à la main** en Slint
> (`ui/panels/menubar.slint` + `ui/widgets/menu.slint`), avec la même
> structure, les mêmes libellés et les mêmes raccourcis.
>
> Ce que cela coûte, et qui est assumé : sur macOS la barre est **dans la
> fenêtre** comme sur Windows et Linux, et non dans la barre système. Le reste
> de ce document — la structure des six menus, la règle « aucune capacité
> nouvelle », le contrat de chaque entrée — décrit l'implémentation telle
> quelle, à une entrée près : **Préférences…** est présente mais **désactivée**.
> C'est cohérent avec [ADR 0019](0019-distribution-i18n.md), qui met le réglage
> explicite de langue « hors scope immédiat » — l'entrée tient la place que
> cette carte de menus lui donne, et s'activera avec le panneau qui la remplit
> (voir [ADR 0077](0077-application-updates.md) §2, qui y adosse une seconde
> attente). **[ADR 0078](0078-preferences-panel.md) construit ce panneau et
> active l'entrée** : la carte ci-dessous n'a plus d'exception.

Structure retenue (reflète l'existant, `docs/engine-api.md` fait foi pour le comportement réel de chaque action) :

* **File** — Import…, Export…, ———, Preferences… *(nouveau, ADR 0019 : langue)*, ———, Quit
* **Library** — New Collection…, Add to Collection, Remove from Collection, ———, Reprocess Library…
* **Photo** — Rate ▸, Label ▸, Flag ▸ (Pick/Reject/Clear), ———, Develop, Reprocess
* **Develop** *(actif en mode développement)* — Undo, Redo, ———, Save Preset…, Apply Preset ▸, ———, Back to Library
* **View** — Library, Develop
* **Help** — Keyboard Shortcuts…, About Leyline

Les entrées sensibles au contexte (ex. `Reprocess` sous Photo, qui aujourd'hui n'agit que sur la photo ouverte en développement) restent grisées hors de ce contexte plutôt que de changer silencieusement de comportement — même contrat que le raccourci clavier correspondant.

## Conséquences

* Aucun changement du modèle d'événements ou de l'API moteur : la barre de menu est une couche UI pure côté `leyline-studio`, elle n'ajoute rien à `leyline-engine`/`leyline-sdk`.
* Les raccourcis clavier existants restent inchangés et prioritaires ; la barre de menu est un second chemin d'accès, pas un remplacement.
* Devient l'emplacement naturel des Préférences (ADR 0019) et d'un futur panneau « About » — pas d'emplacement concurrent à inventer ailleurs.
* `Help ▸ Keyboard Shortcuts…` documente ce que la barre elle-même ne peut pas montrer en permanence (les raccourcis mono-touche du mode classement, ex. chiffres pour la notation) — reste à concevoir comme un panneau, hors scope de cet ADR.

## Alternatives écartées

* **Menu unique en surcouche (icône avatar/kebab)** : plus léger, mais ne résout pas la découvrabilité — encore faut-il savoir qu'une action existe pour aller la chercher dans un menu générique plutôt que dans une hiérarchie thématique.
* **Aucun menu, uniquement une fiche de raccourcis (`?`)** : cohérent avec le style actuel « tout au clavier », mais demande à un nouvel utilisateur d'apprendre la fiche entière avant de savoir ce que l'app sait faire, plutôt que de le découvrir en parcourant des menus familiers.
