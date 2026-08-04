# ADR 0069 — Une extension fermée s'attache, elle ne duplique pas le dépôt

**Statut :** Accepté — 2026-08

## Contexte

Leyline veut une fonctionnalité payante (les masques assistés, C2 de
`competitive-plan.md`) sans cesser d'être un logiciel libre. La question posée
était : faut-il **cloner le dépôt** et maintenir deux projets portant « le même
code pour l'essentiel », l'un ouvert, l'autre payant ?

Non, et le refus n'est pas d'abord économique.

### Pourquoi un clone est le pire des choix ici

Un fork « presque identique » coûte, pour un mainteneur seul, un report de
correctif à chaque correctif, pour toujours. Mais Leyline a une raison plus
dure que la fatigue.

`pipeline.md` §5.1 promet qu'une version d'étage rend **identiquement, partout
et pour toujours**. Cette promesse est portée par du code gelé : `sharpen::v1`
est littéralement le même code dans tous les binaires. **Deux dépôts ne peuvent
pas être tous les deux propriétaires de `sharpen::v1`.** À la première dérive —
un correctif appliqué d'un côté, un `renders.json` béni deux fois — la promesse
est rompue, et elle est rompue *silencieusement* : personne ne s'en aperçoit
avant qu'une photo de 2026 ne se rende autrement en 2031.

Le clone ne duplique donc pas seulement du code, il duplique **la chose que le
projet promet de ne jamais dupliquer**.

### Ce que le projet a déjà décidé

Le `CLA.md` existe et dit la suite : Leyline « intends, over time, to offer
additional commercial licenses alongside the GPL community edition ». Les
droits nécessaires sont donc déjà rassemblés — contributeurs compris. Ce qui
manquait n'était pas la permission, c'était **la couture** : par où une
extension fermée s'attache sans toucher au dépôt ouvert.

## Décision

**Le dépôt public reste entier et unique. Une extension fermée est un crate
séparé, dans son propre dépôt privé, qui s'attache par une frontière que le
moteur ne franchit jamais.**

### 1. La règle : une extension produit des *réglages*, jamais des pixels

C'est le cœur de l'ADR, et tout le reste en découle.

Une extension peut **lire** une image décodée et **écrire** dans une révision.
Elle ne participe pas au rendu. Elle n'est pas un étage, elle n'a pas de
version d'étage, elle n'apparaît pas dans la carte `stages`.

Architecturalement, un masque assisté est donc **la même chose que l'outil
pinceau** : quelque chose qui produit de la donnée de masque, que le pipeline
ouvert rend ensuite. Le pinceau est piloté par une souris, celui-là par un
modèle ; du point de vue du moteur, la différence n'existe pas.

Trois conséquences, et ce sont elles qui rendent la décision sûre :

* **§5.1 est hors d'atteinte.** Aucun composant fermé n'entre dans le chemin de
  rendu, donc aucune version d'étage ne dépend d'un code que le public ne peut
  pas lire. La promesse la plus chère du projet ne rencontre jamais la frontière
  de licence.
* **La version libre rend tout.** Une photo retouchée avec un masque assisté
  s'ouvre, se rend et s'exporte **à l'identique** sur une compilation sans
  l'extension : le masque est de la donnée dans `settings_json`, comme un tracé
  de pinceau. Ce qui manque à la version libre, c'est l'outil qui *propose* le
  masque, jamais celui qui l'applique.
* **Le format de catalogue ne se scinde pas.** C'est le corollaire du point
  précédent, et la ligne rouge : le jour où un fichier écrit par l'édition
  payante ne serait plus lisible par l'édition libre, la non-destructivité
  (`pipeline.md` §6) serait rompue *contre nos propres utilisateurs*.

### 2. L'attache est le SDK, pas le moteur

Une extension est un **client** de `leyline-sdk`, au même titre que Studio ou
la CLI (`architecture.md` : `Studio → SDK → Engine → Core`). Elle demande un
aperçu, calcule, et écrit par une session d'édition ordinaire.

Le moteur ne gagne donc **aucune surface d'extension** : pas de registre de
greffons, pas de trait de rappel appelé pendant un rendu, pas de chargement
dynamique. Ce qui n'existe pas ne peut pas devenir un canal par lequel du code
fermé s'infiltre dans le pipeline — c'est la garantie du §1, rendue structurelle
plutôt que promise.

### 3. Ce que le moteur doit tout de même apprendre

Pour qu'un masque calculé soit *exprimable* comme donnée, `Mask` doit pouvoir
porter une couverture calculée, et non seulement une géométrie paramétrique
(`Radial`, `Gradient`, `Brush`, `Everything` — ADR 0029, ADR 0048).

C'est un ajout au moteur **ouvert**, avec sa propre ADR et sa propre version
d'étage `local_adjustments` : une couverture stockée est un rendu que les
versions gelées ne savent pas produire, et la règle de capacité s'applique
telle quelle — une version d'étage qui ne sait pas exprimer un réglage le
**refuse**, elle ne l'ignore pas en silence.

Cet étage est ouvert, gratuit, et rend les masques de tout le monde. C'est ce
qui fait tenir le §1.

### 4. La permission additionnelle GPLv3 §7

Un crate propriétaire lié à `leyline-sdk` forme une œuvre combinée que la
GPLv3 gouverne. Le projet détient les droits nécessaires (§Contexte), il peut
donc l'autoriser — mais **cela doit être écrit**, sans quoi le dépôt public dit
une chose et le binaire livré en fait une autre.

La forme retenue est une *permission additionnelle* au sens de la GPLv3 §7,
consignée dans un fichier propre au projet. Le texte de la GPL lui-même n'est
**jamais** modifié : il reste verbatim dans `LICENSE`.

Rédaction retenue :

> **Additional permission under GNU GPL version 3 section 7**
>
> The copyright holders of Leyline give you permission to combine Leyline with
> software released under terms of your choice, and to convey the resulting
> work, provided that every part of Leyline itself remains governed by the GNU
> General Public License version 3 and is conveyed under those terms.
>
> This permission does not extend to modified versions of Leyline: if you
> modify Leyline, this additional permission does not apply to your modified
> version, and you may remove it.

### 5. Ce qui n'est **pas** décidé ici

**Le système de licence payante.** Vérification de clé, activation, édition
gratuite contre payante : rien de tout cela n'est tranché par cette ADR, et
rien n'a besoin de l'être pour commencer.

C'est délibéré. La frontière ci-dessus ne coûte presque rien et constitue une
meilleure architecture indépendamment de toute question commerciale — elle
permet de commencer le travail sur les masques assistés sans avoir décidé du
modèle. Le contrôle de licence viendra, s'il vient, **derrière** cette frontière
et sans toucher au moteur.

Deux points resteront à trancher ce jour-là, et il vaut mieux les nommer
maintenant :

* la vérification devra être **hors ligne** — `vision.md` (Local First)
  interdit l'appel serveur, et une clé qui téléphone contredirait la promesse
  la plus lisible du projet ;
* `specification.md` §4 range aujourd'hui l'abonnement parmi les exclusions
  délibérées. Une édition payante devra corriger ce texte plutôt que le
  contourner.

## Conséquences

* **Un seul dépôt public, entier.** Rien n'en est retiré, aucune fonctionnalité
  n'y est amputée pour être revendue ailleurs.
* **Le crate fermé est petit** : il propose des masques, il n'en rend aucun.
  Tout ce qui est cher et délicat — décodage, pipeline, couleur, export —
  reste ouvert et partagé.
* **Aucun fork, donc aucun report de correctif.** L'extension suit les
  versions publiées du SDK comme n'importe quel client.
* **Les dépendances devront être revérifiées** avant la première livraison
  fermée : la brique GUI (Slint) offre plusieurs licences dont l'option GPLv3,
  qui cesse de convenir dès qu'un binaire livré n'est plus GPLv3 ; LibRaw et
  Lensfun sont en LGPL, ce qui impose de laisser l'utilisateur relier — une
  décision de build, que la compilation croisée Windows touche déjà.
* La permission du §4 devra être **ajoutée au dépôt** avant la première
  livraison combinée, pas après.

## Alternatives écartées

* **Cloner le dépôt** (la question d'origine). Écartée au §Contexte : deux
  propriétaires pour une même version d'étage, donc une rupture silencieuse de
  §5.1 — plus le report de correctif perpétuel qui, seul, suffirait déjà.
* **Un greffon appelé pendant le rendu.** La forme intuitive, et exactement ce
  que le §1 interdit : le rendu d'une révision dépendrait alors d'un composant
  dont la version n'est pas dans la carte `stages` et dont personne ne peut
  auditer le gel. C'est §5.1 abandonnée pour de la commodité d'architecture.
* **Retirer la fonctionnalité du dépôt ouvert** (open core par soustraction).
  Le moteur y perdrait un étage, et une photo éditée avec l'édition payante
  cesserait d'être rendue par l'édition libre — le format de catalogue se
  scinde, et la non-destructivité est rompue pour l'utilisateur qui a le
  malheur de revenir en arrière.
* **Double licence du moteur entier**, à la Qt, sans extension fermée. C'est le
  modèle que `CLA.md` garde ouvert et il reste possible ; il ne répond
  simplement pas à la question posée ici, parce qu'il monétise les
  redistributeurs — or les utilisateurs de Leyline sont des photographes, qui
  ne redistribuent rien.
* **Un binaire séparé communiquant par le catalogue.** Évite la question du
  liage, au prix d'un second processus, d'un second cycle de vie et d'un
  contournement de la façade que `engine-api.md` §13 existe pour empêcher. La
  permission du §4 coûte trois paragraphes et évite tout cela.
