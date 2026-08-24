# ADR 0077 — Se mettre à jour : un manifeste signé, et une question posée une fois

**Statut :** Accepté — 2026-08

## Contexte

Le dépôt s'ouvre bientôt, et [ADR 0019](0019-distribution-i18n.md) a livré les
trois façons d'installer Leyline — AppImage, installeur NSIS, `.dmg`. Aucune ne
dit ce qui se passe **ensuite**. Une personne qui installe la 0.1.0 aujourd'hui
n'a aucun moyen d'apprendre que la 0.2.0 existe, sinon revenir d'elle-même sur
une page de téléchargement dont rien ne lui a donné l'adresse.

C'est le dernier manque de la mise en distribution, et il a été **reporté
explicitement** le 2026-08-02 à « avant l'ouverture » : sans binaires publiés,
un mécanisme de mise à jour pointe vers un vide.

### Ce que la vision impose, et qui n'est pas négociable ici

[`vision.md`](../vision.md) autorise le réseau pour exactement cet usage, et
pose la contrainte dans la même phrase :

> Le réseau ne sert qu'à des usages **optionnels et explicites** : vérifier
> qu'une mise à jour existe, la télécharger, éventuellement partager un preset.

« Optionnel et explicite » n'est pas une préférence de style : c'est ce qui
distingue Leyline d'un logiciel qui téléphone chez lui. Rien de ce qui suit ne
peut envoyer autre chose que **la version installée et la plateforme**, ni
partir sans que quelqu'un l'ait demandé.

### Ce que la brique de packaging donne déjà

`cargo packager` est déjà la chaîne d'empaquetage (ADR 0019). Sa brique
compagnon `cargo-packager-updater` lit un manifeste JSON, compare les
versions, télécharge le paquet de la plateforme courante et le remplace. Elle
apporte trois choses qu'on n'a pas à écrire :

* la **signature du paquet** (paire de clés minisign : la privée signe à la
  publication, la publique est compilée dans le binaire) — sans elle, une mise
  à jour est un téléchargement d'exécutable arbitraire.

  > **Ce qui est signé, précisément** — vérifié dans
  > `cargo-packager-updater` 0.2.3 : le manifeste JSON lui-même **n'est pas
  > signé**, il *porte* la signature minisign de chaque paquet, et cette
  > signature est vérifiée sur les octets téléchargés avant toute
  > installation (`verify_signature`, appelé par `Update::download`). La
  > propriété obtenue est celle qui compte : personne ne peut faire installer
  > un binaire qu'il n'a pas signé. Ce qu'elle ne couvre pas, et qui reste
  > tenu par HTTPS seul : rediriger vers une **version publiée plus
  > ancienne**, ou empêcher la découverte d'une mise à jour. Un dépôt qui
  > perdrait la maîtrise de ses releases aurait de toute façon un problème
  > plus grand que celui-là ;
* le remplacement **en place** par plateforme : une AppImage se réécrit
  elle-même, et l'installeur NSIS est déjà en `installer-mode = "currentUser"`
  (`crates/leyline-studio/Cargo.toml`), donc **aucune élévation UAC** n'est
  demandée ;
* le cas macOS, qui exige que le `.app` soit **notarisé** avant qu'un
  remplacement soit acceptable — contrainte de plateforme, pas de conception.

## Décision

### 1. Le manifeste vit sur GitHub Releases, à une URL qui ne bouge jamais

Le manifeste est un asset de release nommé `latest.json`, et le binaire
interroge la redirection stable que GitHub maintient :

```
https://github.com/leyline-studio/leyline/releases/latest/download/latest.json
```

Cette URL désigne toujours l'asset de la release la plus récente, sans que son
texte change d'une version à l'autre. C'est ce qui permet de la **compiler en
dur** — et il le faut : une URL de mise à jour configurable est une surface de
détournement, pas une commodité. En changer est une release, pas un réglage.

Le choix se fait sur une seule question : **qu'est-ce que le projet accepte de
devoir maintenir en vie ?** Un binaire installé interroge son URL pendant des
années. Un domaine, un certificat TLS et un hébergement sont trois choses à
renouveler indéfiniment sous peine de casser les copies déjà installées ; les
releases sont l'endroit où les binaires vont **déjà**, et le manifeste y voyage
avec eux, publié par le même geste. Un projet local-first qui refuse d'opérer un
service pour ses utilisateurs ne va pas en opérer un pour ses propres mises à
jour.

La clé privée de signature ne vit **ni dans le dépôt ni dans le CI** : elle
signe à la main au moment de publier. Le dépôt ne porte que la clé publique.

### 2. On demande une fois, puis on vérifie tout seul — mais on n'installe jamais tout seul

C'est la cadence de Firefox sans son silence, et les deux moitiés se séparent
proprement.

**Le consentement est demandé une fois, et vaut « non » tant qu'il n'a pas été
donné.** « Vérifier automatiquement si une mise à jour existe ? » — une
question, une réponse stockée, jamais reposée. C'est ce qui rend la suite
conforme à `vision.md` : la vérification reste **optionnelle et explicite** au
sens fort — quelqu'un l'a autorisée en connaissance de cause — plutôt qu'au
sens littéral d'un clic à chaque fois. Un défaut à « non » est ce qui distingue
cette lecture d'une permission arrachée.

**Si c'est oui : une vérification par lancement, et pas davantage.** Quelques
secondes après l'ouverture de la bibliothèque, jamais pendant — et sautée si la
dernière a réussi il y a moins de 24 h, pour que cinq lancements dans un
après-midi ne fassent pas cinq requêtes. **Aucun minuteur en cours de session :**
une séance de développement dure des heures, et un appel réseau qui part au
milieu est exactement la surprise que le principe local-first refuse.

**Rien trouvé, rien dit.** Pas de notification « vous êtes à jour », pas
d'entrée de journal. Le cas normal est le silence total.

**Quelque chose trouvé : une pastille, pas une fenêtre.** Le libellé **Aide**
de la barre de menus porte une marque discrète, et l'entrée
*Rechercher des mises à jour…* devient *Mettre à jour vers 0.2.0…*. Aucune
modale ne s'interpose, rien n'interrompt le travail en cours, et l'information
attend qu'on la regarde.

> Cette pastille est possible **parce que la barre de menus est écrite à la
> main** ([ADR 0020](0020-menu-bar.md), correction du 2026-08-05) : un
> `MenuBar` natif ne se décore pas. Le contournement d'un bug de repaint se
> révèle après coup être ce qui rend cette surface disponible.

**L'installation demande toujours un clic**, et c'est ce qui referme
gratuitement le danger du §4 : une migration de catalogue est irréversible, et
personne ne peut en subir une qu'il n'a pas déclenchée. C'est la moitié de
Firefox qu'on ne prend pas, et ce n'est pas seulement par prudence — la brique
ne sait pas la donner (voir §Alternatives écartées).

**Où vit le réglage.** Dans le panneau **Préférences** — l'entrée du menu
Fichier était désactivée depuis [ADR 0020](0020-menu-bar.md) parce
qu'[ADR 0019](0019-distribution-i18n.md) avait mis le choix de langue « hors
scope immédiat ». Ce panneau avait **deux locataires** et une raison d'être
construit ; c'est son propre ADR, et la présente décision en dépendait pour
être livrable. [ADR 0078](0078-preferences-panel.md) l'a écrit, et les deux
sont livrées. Ce qui est fixé ici et qu'il ne peut pas changer : la question
est posée une fois, le défaut avant réponse est « non », et elle ne s'interpose
jamais devant un premier lancement ([ADR 0054](0054-first-run-and-basic-mode.md)
possède cet écran).

> C'est [ADR 0078](0078-preferences-panel.md) qui l'écrit, en respectant ces
> trois contraintes : la question est posée au **second** lancement, **toute
> façon de fermer le dialogue vaut « non »** et est stockée comme telle, et le
> réglage vit dans `preferences.json` avec `last_update_check` — le plafond de
> 24 h du présent paragraphe lit ce champ.

### 3. Rien n'est envoyé, et rien n'est installé sans un second geste

* La requête ne porte que ce que l'URL contient — **aucun identifiant, aucun
  compteur, aucune donnée de bibliothèque**. La version installée et la
  plateforme sont connues du client, pas transmises comme télémétrie : elles
  servent à choisir une ligne du manifeste, localement.
* Un échec réseau n'est **pas** une erreur de l'application : hors ligne est
  l'état normal de Leyline. Le dialogue dit qu'il n'a pas pu joindre le serveur
  et se ferme ; rien ne réessaie.
* Trouver une version ne l'installe pas. Le dialogue montre le numéro et les
  notes de version, et attend un second clic.

### 4. Le catalogue est sauvegardé avant qu'une version plus récente y touche

C'est la partie de cette décision qui n'a rien à voir avec le réseau, et la
seule dont l'absence peut **perdre du travail**.

`Catalog::open` applique les migrations en attente, une transaction par
migration. Elles n'ont **pas de retour arrière** : il n'existe pas de script
descendant, et `Catalog::open` **refuse** un catalogue plus récent que le moteur
(`LeylineError::NewerCatalog`). Aujourd'hui c'est sans conséquence — on
n'installe une nouvelle version qu'en le voulant. Avec une mise à jour à un
clic, la séquence « je mets à jour, la migration tourne, je veux revenir en
arrière » devient atteignable par accident, et l'ancienne version ne peut plus
ouvrir la bibliothèque.

Donc : **avant d'appliquer une migration, `catalog.db` est copié dans
`Backups/`**, sous un nom qui porte la version de schéma quittée. Le dossier
`Backups/` existe dans le squelette d'une bibliothèque depuis
[ADR 0010](0010-relative-paths.md) et `catalog.md` §3 — il est créé à chaque
`Library::create` et **rien n'y a jamais écrit**. C'est son usage.

**Ce n'est pas une copie de fichier, et ça ne pouvait pas l'être.** Les
connexions sont en mode **WAL** (`docs/catalog.md` §6) : des pages validées
peuvent encore vivre dans `catalog.db-wal` et non dans `catalog.db`. Copier le
seul fichier produit donc une sauvegarde à laquelle il manque, silencieusement,
le travail le plus récent — précisément celui qu'on voudrait retrouver. C'est
`VACUUM INTO` qui est utilisé : SQLite construit lui-même une image cohérente
de toute la base en un fichier. Le test le vérifie en gardant une connexion
d'écriture **ouverte** pendant la migration, parce que la fermer suffirait à
rabattre le WAL dans le fichier principal et à faire passer une copie naïve.

Trois précisions qui font la différence entre une sauvegarde et une illusion :

* la copie est faite **avant** la première migration et **une seule fois** par
  ouverture, pas une par migration : ce qu'on veut restaurer est l'état d'avant
  la mise à jour, pas un état intermédiaire ;
* si la copie échoue, l'ouverture **échoue** au lieu de migrer quand même. Une
  migration irréversible sur un catalogue non sauvegardé est précisément ce que
  ce paragraphe existe pour empêcher ;
* elle ne se déclenche que s'il y a réellement une migration à appliquer —
  ouvrir une bibliothèque à jour ne recopie rien, sans quoi le dossier
  grossirait à chaque lancement.

Cette partie est **indépendante du reste de l'ADR** : elle ne touche pas au
réseau, elle est utile immédiatement, et elle est livrée sans attendre qu'il y
ait des binaires à télécharger.

### 5. Ce que cet ADR ne fait pas

* **Aucune migration de bibliothèque, aucun changement de schéma.**
* **Aucun pixel, aucune version d'étage.** `pipeline.md` §5 est hors de cause :
  une mise à jour peut changer le rendu — c'est même à ça que servent les
  versions d'étage ([ADR 0042](0042-versioned-stage-pipeline.md)) — mais rien
  ici ne touche au contrat. Et il vaut la peine d'écrire le corollaire
  rassurant, parce que la crainte spontanée est l'inverse : **une mise à jour
  ne retouche aucune photo déjà développée.** Une révision porte sa carte
  d'étages et le moteur honore la version qu'elle enregistre
  ([ADR 0043](0043-collapse-prerelease-render-history.md)) ; une version
  d'étage plus récente ne s'applique qu'au **retraitement**, qui est un acte
  explicite. Le seul état qu'une mise à jour modifie sans qu'on le lui demande
  est le **schéma du catalogue** — d'où le §4.
* **La CLI et le SDK ne se mettent pas à jour.** Une bibliothèque Rust est mise
  à jour par le gestionnaire de paquets de qui l'utilise ; un binaire en ligne
  de commande, par la distribution qui l'a installé. C'est Studio, application
  livrée par installeur, qui a le problème.

## Conséquences

* **Une personne qui installe Leyline apprend qu'une version existe sans avoir
  à y penser**, et sans que Leyline observe qui elle est. C'était le dernier
  manque de la mise en distribution d'ADR 0019.
* **Ce que la vérification révèle malgré tout, et qu'il faut dire.** Aucune
  donnée n'est *transmise* (§3), mais une requête HTTP en révèle par sa seule
  existence : l'adresse IP, et le fait qu'une copie de Leyline s'est lancée à
  cet instant. Une vérification par lancement, plafonnée à une par 24 h,
  fait de ce signal une trace grossière d'usage chez l'hébergeur — pas une
  identité, pas un historique d'édition, mais pas rien non plus. C'est
  exactement ce que le « non » par défaut et la question posée une fois
  laissent décider à chacun, et c'est aussi pourquoi il n'y a **aucun minuteur
  en session** : une seule requête par jour de travail, pas une horloge qui bat.
* **Le projet n'opère aucun service.** Pas de domaine, pas de certificat, pas
  d'hébergement : la disponibilité des mises à jour est celle de GitHub, ce qui
  est déjà la disponibilité du code source. Le corollaire est assumé : changer
  d'hébergeur un jour demandera une release de transition, que les copies
  installées avant elle ne verront pas.
* **Une clé privée devient un actif du projet.** La perdre veut dire publier une
  nouvelle clé publique dans un binaire, donc une mise à jour manuelle pour tout
  le monde. Elle ne vit pas dans le CI, ce qui rend la publication d'une release
  manuelle — c'est le prix de ne pas laisser une machine signer des exécutables
  toute seule.
* **`Backups/` cesse d'être un dossier vide** et devient l'endroit d'où l'on
  repart quand une mise à jour a migré une bibliothèque qu'on voulait laisser
  telle quelle.
* **macOS reste en retrait**, comme pour le packaging
  ([ADR 0019](0019-distribution-i18n.md), et la construction `.dmg` repoussée) :
  sans notarisation, le remplacement du `.app` n'est pas proposé. Ce n'est pas
  une exception de conception, c'est la même dépendance qui bloque déjà la
  distribution macOS.

## Alternatives écartées

* **Un serveur de mise à jour opéré par le projet** (domaine + manifeste
  statique). Donnerait l'indépendance vis-à-vis de GitHub, au prix d'une
  infrastructure à maintenir aussi longtemps que la plus vieille copie
  installée. Un projet qui met « aucun cloud, aucun compte » dans sa vision ne
  commence pas par se doter d'un service dont chaque installation dépend.
* **GitHub Pages plutôt que les assets de release.** Une seconde surface de
  publication pour les mêmes octets, à tenir synchronisée avec les releases à
  la main. La redirection `releases/latest/download/` fait le même travail sans
  second geste.
* **Une URL de mise à jour configurable.** Utile pour tester, et c'est
  exactement pourquoi c'est dangereux : le réglage qui aide au test est le
  réglage qui redirige une installation vers un binaire choisi par
  quelqu'un d'autre. Les tests visent une URL de compilation, pas un réglage
  d'exécution.
* **Vérifier au démarrage par défaut, avec un réglage pour désactiver.** C'est
  le comportement de la plupart des applications, et il inverse la phrase de
  `vision.md` : le réseau partirait sans qu'on l'ait demandé, la case à cocher
  ne servant qu'à réparer après coup. Le défaut est ce qui compte dans « aucune
  vérification silencieuse » — d'où la question posée une fois du §2, qui
  obtient le même résultat en le demandant.
* **Ne vérifier que manuellement** (position tenue par la première rédaction de
  cet ADR). Défendable — un clic *est* le consentement, et rien à concevoir —
  mais presque personne ne clique : les gens attentifs sont informés, les
  autres restent sur leur version indéfiniment. Le risque que cela laisse
  ouvert est nommable : Leyline n'a ni compte, ni synchronisation, ni serveur,
  donc sa surface d'attaque est **l'analyse d'un fichier qu'on ouvre soi-même**,
  essentiellement LibRaw. Un correctif qui n'arrive jamais protège de fichiers
  qu'on ouvrira quand même. La question posée une fois coûte un panneau de
  Préférences qu'il faut de toute façon construire.
* **Le modèle Firefox complet : télécharger et appliquer en silence**, la
  nouvelle version prenant effet au lancement suivant. C'est ce qui rend
  Firefox agréable — on ne le voit jamais arriver — et
  `cargo-packager-updater` **ne sait pas le faire** : `download_and_install()`
  installe *maintenant*, il n'y a pas de mise en attente. « À la Firefox » avec
  cette brique serait donc **plus** intrusif que Firefox, l'installeur se
  déclenchant pendant qu'on travaille. Le reproduire demanderait d'écrire le
  remplacement différé par plateforme (AppImage, NSIS), soit exactement le
  travail qu'[ADR 0019](0019-distribution-i18n.md) a choisi `cargo packager`
  pour ne pas avoir à faire. S'ajoute une raison propre à Leyline, que Firefox
  n'a pas : une mise à jour **migre le catalogue** sans retour arrière, après
  quoi la version précédente ne peut plus ouvrir la bibliothèque. Subir ça sans
  l'avoir déclenché n'est pas acceptable. À rouvrir si la brique gagne un jour
  une installation différée — la question serait alors uniquement celle du
  catalogue.
* **Ne rien faire et laisser les gestionnaires de paquets s'en charger**
  (Flatpak, winget, Homebrew). Ce serait la bonne réponse si Leyline y était
  publié ; ADR 0019 a choisi trois installateurs autonomes précisément parce
  qu'il ne l'est pas. À rouvrir le jour où il l'est — ce serait alors cet ADR
  qu'on remplacerait, pas qu'on complèterait.
* **Sauvegarder le catalogue à chaque ouverture**, plutôt qu'avant une
  migration. Copier des dizaines de mégaoctets à chaque lancement pour un
  événement qui arrive une fois par an, et remplir `Backups/` de copies qu'on
  ne saurait plus distinguer.
