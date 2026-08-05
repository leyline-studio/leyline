# ADR 0078 — Préférences : une règle d'admission, puis deux réglages

**Statut :** Accepté — 2026-08

## Contexte

L'entrée **Fichier ▸ Préférences…** existe depuis [ADR 0020](0020-menu-bar.md),
visible et **désactivée**, parce qu'[ADR 0019](0019-distribution-i18n.md) avait
mis le choix de langue « hors scope immédiat ». Cinquante-huit ADR plus tard,
elle est toujours le seul élément inerte de la barre de menus.

Deux décisions attendent maintenant cet endroit :

* le **choix de langue** d'[ADR 0019](0019-distribution-i18n.md), reporté ;
* le **consentement à la vérification de mise à jour** d'[ADR 0077](0077-application-updates.md) §2,
  qui déclare explicitement dépendre de ce panneau pour être livrable.

### Le vrai problème n'est pas les deux réglages

Écrire un dialogue à deux lignes ne demande pas d'ADR. Ce qui en demande un,
c'est qu'un panneau de préférences est une **pièce qui se remplit** : chaque
décision future qui hésitera entre « trancher » et « laisser choisir » aura
désormais un endroit où déposer sa question, et une préférence est le moyen le
plus courant de ne pas décider. Le dépôt a jusqu'ici refusé ce réflexe trois
fois, sans jamais l'écrire comme une règle :

* [ADR 0022](0022-default-library-fallback.md) — l'emplacement de la
  bibliothèque par défaut est affiché dans *À propos*, « pas de nouveau
  réglage, pas de fenêtre dédiée » ;
* [ADR 0075](0075-preview-cache-retention.md) §2 — la fenêtre de rétention est
  « une constante nommée, pas un réglage : un utilisateur n'a pas à arbitrer une
  taille de cache » ;
* [ADR 0077](0077-application-updates.md) — l'URL de mise à jour est compilée en
  dur, « une URL de mise à jour configurable est une surface de détournement,
  pas une commodité ».

Ces trois refus ont été argumentés chacun pour soi. Le jour où le panneau
existe, ils ont besoin d'une raison commune, sinon le quatrième cas se décidera
par la facilité.

### Ce que la brique donne déjà, et qui tranche la moitié des questions

Trois faits, vérifiés dans `slint` 1.13.1 et dans le dépôt, et non supposés :

* les traductions groupées passent par `translate_from_bundle`, qui lit une
  **propriété** (`translations_dirty`) et enregistre donc une dépendance :
  changer de langue **réévalue tout ce qui est affiché**, sans reconstruire
  quoi que ce soit ;
* `select_bundled_translation` doit être appelée **après** la création du
  premier composant — c'est déjà ce que fait `main.rs` ;
* le dépôt ne contient **aucun `slint::tr!()` côté Rust** : toutes les chaînes
  visibles vivent dans les `.slint`, en `@tr(...)`. Rien n'échappe donc à la
  réévaluation.

Un changement de langue à chaud est possible, et il n'a pas fallu le concevoir :
il fallait le constater.

## Décision

### 1. Une règle d'admission, écrite avant le premier réglage

Un réglage entre dans les Préférences si les **trois** conditions tiennent :

1. il porte sur l'**installation** — ni sur une photo, ni sur une
   bibliothèque, ni sur une vue ;
2. il doit **survivre à un relancement**, sans quoi il appartient à l'écran où
   il agit ;
3. il n'a **pas d'endroit naturel** dans la surface qu'il gouverne.

Et une interdiction qui ne se négocie pas, quelles que soient les trois
conditions : **aucune préférence ne peut changer un pixel.**
`pipeline.md` §5.1 promet qu'un rendu est fonction de la révision seule ; un
réglage d'application qui entrerait dans le rendu ferait produire à la même
révision deux images sur deux machines, et la promesse deviendrait fausse sans
qu'aucune ligne du pipeline n'ait bougé. C'est ce qui exclut définitivement d'ici
les défauts d'export, l'algorithme de dématriçage ([ADR 0061](0061-demosaic-algorithm.md),
qui est un champ de la révision, précisément pour cette raison), et tout ce qui
leur ressemblera.

La règle rejette aussi, tout de suite, un candidat évident : le mode
**Basique/Complet** d'[ADR 0054](0054-first-run-and-basic-mode.md) échoue à la
condition (3). Son interrupteur est en tête du panneau qu'il gouverne, où on le
trouve en le cherchant ; le déplacer dans les Préférences le rendrait **moins**
découvrable, pas plus configurable.

### 2. Un dialogue modal, pas une fenêtre et pas une vue

`ui/dialogs/preferences.slint` et `wiring/dialogs/preferences.rs`, montés par le
`DialogOverlay` existant ([ADR 0045](0045-studio-ui-modularisation.md) §3),
comme les dix autres dialogues.

Une seconde fenêtre serait la première chose à casser : Studio n'en a qu'une, et
son changement de bibliothèque **relance le processus** (`relaunch_into`,
`src/library.rs`) précisément pour ne pas avoir à démonter et remonter l'état
d'une fenêtre. Une vue au même rang que Bibliothèque, Develop et Carte serait
l'autre erreur : une vue est un endroit où l'on travaille, les Préférences sont
un endroit où l'on passe.

L'entrée **Fichier ▸ Préférences…** est activée. **Aucun raccourci clavier** :
on ouvre ce panneau deux fois dans une vie d'installation, et une touche vaut
mieux pour un outil de develop.

**Ni OK ni Annuler.** Chaque changement s'applique et s'écrit au moment où il est
fait ; le seul bouton est *Fermer*. Un bouton Annuler annonce une transaction, et
il n'y a ici rien qui puisse être à moitié appliqué. C'est aussi ce que fait déjà
le reste de Studio : un curseur de develop ne se valide pas.

**Ni onglets, ni recherche, ni « rétablir les valeurs par défaut ».** Un panneau
de deux réglages qui se déguise en suite de réglages est pire qu'un panneau de
deux réglages. Le jour où le contenu demande des groupes, c'est une décision de
ce jour-là.

### 3. La langue change à chaud, et le défaut reste le système

Trois choix : **Langue du système** (défaut), **English**, **Français**.

Le premier n'est pas un synonyme du deuxième, et le réglage stocké les
distingue : *absent* signifie « suivre le système », une valeur explicite
signifie « celle-ci, quoi que dise le système ». Confondre les deux
fonctionnerait aujourd'hui et se tromperait le jour où une troisième traduction
arrive, ou celui où quelqu'un change la langue de son système d'exploitation.

Le changement est **immédiat** : toute l'interface bascule sous le curseur, sans
relancer, pour les raisons constatées en Contexte. Deux limites, qui se disent
plutôt qu'elles ne se réparent :

* un message **déjà affiché** dans la ligne de statut garde les mots avec
  lesquels il a été produit. C'est correct : c'est le compte rendu d'un
  événement passé, pas un libellé d'interface ;
* le texte des erreurs remontées par le moteur n'est traduit dans **aucune**
  langue aujourd'hui — le basculement ne régresse rien, il rend seulement
  visible ce qui était déjà vrai.

**Ordre au démarrage**, contraint par la brique : lire les préférences, créer la
fenêtre, puis appliquer la langue — `select_bundled_translation` exige un
composant existant. Une langue stockée l'emporte sur la locale système ; en son
absence, le comportement d'[ADR 0019](0019-distribution-i18n.md) est inchangé.

**Le nom des langues coûte une ligne de Rust, et [ADR 0019](0019-distribution-i18n.md)
est corrigée d'autant.** Cet ADR promettait qu'ajouter une langue ne toucherait
« ni au code Rust ni aux fichiers `.slint` ». C'était vrai tant qu'aucun menu ne
les nommait. La liste que la brique connaît est `["", "fr"]` : un tag vide pour
la langue source, et pas un seul nom lisible — un menu de langues affiche
*Français*, pas `fr`, et surtout pas une chaîne vide. Le nom natif de chaque
langue vit donc dans une petite table Rust, et ajouter une langue coûte
désormais un `.po` **et** une ligne. Inventer un en-tête `.po` privé pour porter
ce nom serait se doter d'une extension de format pour deux entrées.

Le garde-fou tient dans un test unitaire : les dossiers de `translations/` et la
table des noms doivent se correspondre exactement. Un `.po` ajouté sans son nom
échoue au test au lieu de produire une entrée de menu morte.

### 4. Le consentement de mise à jour : posé au second lancement, et toute sortie vaut « non »

[ADR 0077](0077-application-updates.md) §2 fixe trois choses que celle-ci ne peut
pas changer : la question est posée **une fois**, le défaut avant réponse est
**non**, et elle ne s'interpose **jamais** devant un premier lancement — cet
écran appartient à [ADR 0054](0054-first-run-and-basic-mode.md). Reste à décider
*quand* elle est posée, et ce que vaut une non-réponse.

**Au second lancement**, à l'ouverture de la fenêtre. Le premier lancement a déjà
son propos ; le second est le premier moment où l'application n'a rien d'autre à
dire. Le savoir demande un état, et c'est un compteur de lancements qui
**sature à 2** : le fichier n'apprend jamais rien de plus que « ce n'est pas la
première fois », ce qui est exactement ce dont la décision a besoin.

**Toute façon de quitter le dialogue vaut « non », et est stockée comme telle.**
Échap, un clic sur le fond, le bouton *Ne pas vérifier* : trois gestes, un seul
résultat, et la question n'est jamais reposée. C'est ce qui empêche
structurellement ce dialogue de devenir du harcèlement — il n'a pas de second
essai. Il le dit lui-même, en une ligne sous les boutons : *vous pourrez changer
d'avis dans les Préférences*. Sans cette phrase, un silence interprété comme un
refus serait un piège ; avec elle, c'est une valeur par défaut réversible.

Si la réponse est oui, la vérification de ce lancement a lieu — elle suit la
cadence d'[ADR 0077](0077-application-updates.md) §2 comme n'importe quelle
autre, il n'y a pas de premier cas particulier.

**Pourquoi poser la question, plutôt que se contenter de la case à cocher.**
Parce qu'[ADR 0077](0077-application-updates.md) a déjà écarté cette option sous
son vrai nom : « ne vérifier que manuellement » échoue parce que presque personne
ne clique. Une case à cocher que personne n'ouvre est la même chose, avec une
case en plus.

### 5. Un fichier, à côté de ceux qui existent déjà

`preferences.json`, dans le répertoire de configuration que
`directories::ProjectDirs::from("", "", "Leyline")` désigne — celui où vivent
déjà `recent_libraries.json` (la liste des bibliothèques récentes, `src/library.rs`)
et le dossier `detectors/` ([ADR 0073](0073-external-mask-detectors.md) §3, qui
avait choisi cet emplacement pour la même raison). Troisième occupant, aucune
convention nouvelle.

Quatre champs, tous facultatifs :

| Champ | Sens |
| :--- | :--- |
| `language` | tag de langue ; **absent** = suivre le système |
| `update_check` | `true` / `false` ; **absent** = jamais posée |
| `last_update_check` | horodatage de la dernière vérification réussie ([ADR 0077](0077-application-updates.md) §2, plafond de 24 h) |
| `launches` | compteur de lancements, saturé à 2 (§4) |

Les deux derniers ne sont pas des réglages mais de l'état écrit par
l'application ; ils sont nommés ici parce que **cet ADR possède le fichier**, et
qu'un état de même portée et de même durée de vie ne mérite pas un second
fichier pour la seule beauté du classement.

Deux propriétés à tenir :

* **écriture atomique** (fichier temporaire puis renommage) : un fichier tronqué
  par une coupure ne doit pas effacer un consentement déjà donné ;
* **toute lecture qui échoue retombe sur les défauts, et tous les défauts sont
  hors ligne.** Un fichier absent, illisible ou corrompu ne peut donc *jamais*
  activer une vérification réseau — au pire il repose la question, dont la
  réponse avant réponse est « non ». La direction de cette dégradation est le
  seul point de sécurité de tout cet ADR.

### 6. Ce que cet ADR ne fait pas

* **Aucune migration, aucun changement de schéma du catalogue** : les
  préférences ne l'approchent pas.
* **Aucun pixel, aucune version d'étage** — par construction (§1).
* **Ni la CLI ni le SDK ne lisent ce fichier.** Une commande prend ses arguments ;
  un script dont le comportement dépendrait d'une case cochée un jour dans une
  interface graphique serait irreproductible pour une raison invisible depuis sa
  ligne de commande.
* **Le mode Basique/Complet n'est pas déplacé ici**, et sa *persistance* — qu'il
  n'a pas aujourd'hui — reste une question ouverte qui appartient à
  [ADR 0054](0054-first-run-and-basic-mode.md), pas à celle-ci.
* **macOS n'est pas traité** : la convention y est un `Cmd+,` dans le menu de
  l'application, pas une entrée du menu Fichier. Tant que la distribution macOS
  est en retrait ([ADR 0019](0019-distribution-i18n.md),
  [ADR 0077](0077-application-updates.md)), c'est une différence sans porteur.

## Conséquences

* **[ADR 0077](0077-application-updates.md) devient livrable.** C'était sa seule
  dépendance non satisfaite.
* **Le dernier élément inerte de la barre de menus disparaît.**
  [ADR 0020](0020-menu-bar.md) avait posé une entrée désactivée « en attendant » ;
  elle aura attendu cinquante-huit ADR, ce qui est une leçon sur les entrées
  désactivées davantage que sur les préférences.
* **La règle d'admission (§1) est la partie qui servira.** Les deux réglages sont
  écrits une fois ; la règle sera citée à chaque fois qu'une décision future
  voudra se transformer en case à cocher. Elle rend aussi rétroactivement
  explicites les refus d'[ADR 0022](0022-default-library-fallback.md),
  [0075](0075-preview-cache-retention.md) et
  [0077](0077-application-updates.md), qui les avaient argumentés chacun pour
  soi.
* **Un basculement de langue à chaud est un outil d'audit de traduction gratuit.**
  Choisir *Français* et parcourir la fenêtre montre en quelques secondes toute
  chaîne restée hors de `@tr(...)` — ce qui est exactement le mode d'échec
  silencieux que le `.pot` obsolète produit à chaque tranche d'interface.
* **Un consentement peut se perdre avec le fichier de configuration**, et la
  question se reposera alors. C'est le prix assumé de faire dégrader tous les
  chemins d'échec vers « non ».
* **Le répertoire de configuration a désormais trois occupants** — la liste des
  bibliothèques récentes, les manifestes de détecteurs, les préférences. C'est
  encore un répertoire plat ; ce ne le sera pas indéfiniment.

## Alternatives écartées

* **Une fenêtre de préférences séparée.** La convention de la plupart des
  applications de bureau, et la seule chose qui obligerait Studio à savoir gérer
  deux fenêtres — alors que son changement de bibliothèque est bâti sur
  l'hypothèse inverse (relancer le processus plutôt que remonter un état). Un
  dialogue modal donne le même résultat sans toucher à cette hypothèse.
* **Une vue Préférences**, au rang de Bibliothèque, Develop et Carte. Une vue est
  un lieu de travail avec sa propre disposition et ses propres raccourcis ; deux
  réglages n'en font pas un.
* **Stocker les préférences dans le catalogue.** Mauvaise portée, et deux effets
  concrets : quelqu'un qui tient deux bibliothèques répondrait deux fois à la
  question de consentement, et une bibliothèque posée sur un disque externe
  transporterait la langue de l'application qui l'a créée. Le catalogue décrit
  des photos, pas une installation.
* **Un fichier TOML commenté, pensé pour l'édition à la main.** Plus aimable à
  lire, mais l'édition à la main n'est pas un objectif — et ce serait un second
  format dans un répertoire qui en a déjà un (`recent_libraries.json`, les
  manifestes de détecteurs). La cohérence vaut plus que le confort d'un fichier
  qu'on n'ouvrira pas.
* **Poser la question du consentement au premier lancement.** Interdit par
  [ADR 0077](0077-application-updates.md) §2, et pour une bonne raison :
  [ADR 0054](0054-first-run-and-basic-mode.md) a conçu ce moment pour expliquer
  comment faire entrer des photos. Une question sur le réseau y arriverait avant
  que quiconque sache ce qu'est le logiciel qui la pose.
* **Ne jamais poser la question, et se contenter de la case à cocher dans le
  panneau.** Le plus discret, et déjà écarté sous un autre nom par
  [ADR 0077](0077-application-updates.md) : les gens attentifs sont informés, les
  autres restent sur leur version indéfiniment, correctifs de sécurité compris.
* **Reposer la question si le dialogue est fermé sans réponse.** Attrapante d'une
  ou deux réponses de plus, et c'est exactement le mécanisme par lequel un
  logiciel devient désagréable. Une non-réponse est une réponse ; le panneau
  existe pour en changer.
* **Un redémarrage pour appliquer la langue**, comme le fait le changement de
  bibliothèque. Ç'aurait été le choix par défaut sans la vérification faite en
  Contexte — et il aurait été inutile : la brique réévalue les chaînes toute
  seule, et la seule raison de relancer aurait été de ne pas avoir lu son code.
* **Dériver la liste des langues de la brique seule**, pour tenir la promesse
  d'[ADR 0019](0019-distribution-i18n.md) à la lettre. La liste existe
  (`["", "fr"]`) mais n'est atteignable que dans le variant d'erreur de
  `select_bundled_translation` — il faudrait demander une langue impossible pour
  lire la réponse — et elle ne porte aucun nom affichable. Un menu construit
  ainsi proposerait une entrée vide.
