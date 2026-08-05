# ADR 0074 — Le rendu suit le curseur

**Statut :** Accepté — 2026-08

## Contexte

On développe une photo à l'aveugle.

`EditSlider` (`ui/widgets/controls.slint`) n'émet `edited(…)` que sur
`PointerEventKind.up` : pendant tout le glissement, la poignée et le nombre
bougent, **l'image ne bouge pas**. On relâche, on regarde, on reprend. Régler
une exposition ou une clarté demande donc une série d'essais successifs là où
tous les logiciels du marché montrent le résultat sous le doigt.

### Ce que ça révèle

[ADR 0041](0041-interactive-preview-rendering.md) parle, du début à la fin, du
« **coût d'un déplacement de curseur** » : il chiffre ce que coûte un rendu
« à chaque déplacement de curseur » (§1), et ses conséquences annoncent que ce
coût « baisse de deux facteurs qui se multiplient ». Tout son raisonnement
présuppose un rendu **continu pendant le geste**.

Ce rendu-là n'existait pas. Le moteur a été rendu rapide pour une interaction
que l'interface n'a jamais câblée — et c'est resté invisible précisément parce
que les deux moitiés étaient correctes chacune de son côté. C'est la deuxième
fois dans la passe d'avant-gel qu'un écart entre une décision et son
implémentation se cache dans un silence plutôt que dans une contradiction.

Le chiffre qui rend la correction possible est déjà mesuré : le cache d'étages
d'ADR 0041 §3 ramène un curseur de fin de pipeline à **~14 ms** sur une carte
1024×683. Le §3 ci-dessous montre que le rendu live coûte en réalité quatre
fois cela — pour une raison qui n'est pas le pipeline — mais l'ordre de
grandeur reste celui d'une interaction, pas celui d'une attente.

## Décision

### 1. Pendant le glissement, le rendu suit

`EditSlider` gagne un second signal, émis **pendant** le déplacement, à côté de
celui du relâchement. Le premier montre, le second engage :

| Signal | Quand | Ce qu'il fait |
|---|---|---|
| `previewing(valeur)` | à chaque déplacement, borné par §3 | rend et affiche, **n'écrit rien** |
| `edited(valeur)` | au relâchement, et au double-clic de remise à neutre | ce qu'il faisait déjà : applique et **commit** |

### 2. Aucune écriture au catalogue pendant un geste

C'est la contrainte qui rend le reste sûr, et elle était déjà prévue :
`EditSession::set` est « valeur en mémoire, aperçu temps réel — aucune écriture
catalogue » ([`engine-api.md`](../engine-api.md) §10.1). Le rendu live prend
donc ce chemin — poser la valeur dans une session, lire ses `Settings`, rendre,
**abandonner la session sans commit** — et le commit reste au relâchement,
inchangé, fenêtre d'amendement comprise.

Trois choses en découlent, toutes voulues :

* **aucune révision n'est créée par un glissement.** Un curseur traversé de
  bout en bout produit une révision, pas cinquante ;
* **le cache d'aperçus n'est pas touché.** Un rendu live n'est ni écrit sur le
  disque ni enregistré comme aperçu valide d'une révision : c'est une *vue*,
  au même titre que le « avant » de la comparaison ou la surimpression de
  masque (ADR 0071). Le disque ne grossit pas d'un octet pendant qu'on règle ;
* **la promesse §5.1 n'est pas en jeu** : rien de tout cela n'est un rendu
  d'export, aucune version d'étage n'apparaît, aucun pixel enregistré.

### 3. Le rendu live passe par le cache d'étages, et se borne dans le temps

Il emprunte `render_scaled_cached` — le chemin d'ADR 0041 §3, celui pour lequel
le cache a été construit : bouger un curseur de fin de pipeline ne rejoue que
l'aval, et le décodage vient du `DecodeCache`.

Le rendu étant **synchrone** sur le fil de l'interface, un événement de
déplacement attend le rendu précédent. Une borne suffit donc, et une seule :
**au plus un rendu live toutes les 40 ms** (25 images par seconde au maximum,
la limite étant l'œil et non la machine). Un déplacement arrivé pendant ce
délai est ignoré — jamais mis en file : ce qui compte est la position
**actuelle** du curseur, pas le chemin parcouru pour y arriver. Le rendu final,
lui, est garanti par le commit du relâchement.

**Mesuré, sur de vrais fichiers** (`live_preview_keeps_up_with_a_finger`,
`--release`, aperçu `Small`) :

| Fichier | Curseur | Par image |
|---|---|---|
| Canon 60D, 10 Mpx | exposition (rang 40) | **59 ms** |
| Canon 60D, 10 Mpx | accentuation (rang 190) | 64 ms |
| Canon 5D IV, 30 Mpx | exposition | **54 ms** |

Soit ~17 images par seconde : franchement utilisable, et sans commune mesure
avec l'absence de retour. Mais **deux choses détonnent, et il vaut mieux les
écrire que les découvrir**.

D'abord, le curseur de fin de pipeline n'est **pas** plus rapide que celui de
tête, alors qu'ADR 0041 §3 mesurait 14 ms contre 60 sur ce même écart. Le cache
d'étages fonctionne — il n'est simplement plus le terme dominant : le coût par
image est repris **en amont de lui**, par la réduction du buffer décodé à la
taille d'affichage (`proxy`), refaite à chaque image. C'est le §1 d'ADR 0041,
qui décide de réduire *avant* de développer sans dire que le résultat pourrait
être gardé.

Ensuite, le 30 Mpx n'est pas plus lent que le 10 Mpx : le décodage `half_size`
et cette même réduction ramènent les deux au même nombre de pixels développés.

**Ce qui suit, et qui n'est pas fait ici** : garder le proxy réduit en cache, à
côté du `DecodeCache` qui garde déjà le buffer décodé. Le gain attendu est le
plus gros de tout ce document, et il ne touche à aucun pixel — mais c'est une
décision de mise en cache d'ADR 0041, pas de cette ADR-ci, et elle mérite d'être
prise avec sa propre mesure.

> **Suite, 2026-08-05.** Elle l'a été : [ADR 0076](0076-proxy-cache.md) met le
> proxy en cache et ramène ces 59 ms à **15 ms** (10 Mpx) et 54 à **11 ms**
> (30 Mpx). Les chiffres du tableau ci-dessus restent ceux qui ont motivé la
> décision, ils ne décrivent plus le moteur.

La borne des 40 ms garde son sens dans les deux cas : elle ne mord pas
aujourd'hui, où chaque image coûte plus que ça, et elle mordra le jour où le
proxy sera mis en cache.

### 4. Portée : les curseurs, et eux seuls

Les autres gestes du panneau develop restent au relâchement, et ce n'est pas
un oubli : un point de courbe, une tache, un dégradé, une dab de brosse
**créent une entrée** dans une liste. Ils n'ont pas de continuum à suivre — il
n'y a rien à montrer entre le début et la fin d'un clic. Le curseur est le seul
contrôle dont la valeur intermédiaire a un sens visuel.

## Conséquences

* **On règle en regardant l'image**, ce qui est la façon dont on développe une
  photo. C'est l'écart d'usage le plus visible qui restait face aux logiciels
  établis, et il ne demandait aucun travail de rendu — seulement de brancher
  ce qu'ADR 0041 avait rendu possible.
* **~17 images par seconde, mesurées** (§3), et le même chiffre d'un boîtier
  10 Mpx à un 30 Mpx. Ce n'est pas la fluidité d'un Lightroom, c'est la
  différence entre voir et ne pas voir. Le poste dominant est identifié et
  n'est pas le pipeline : la prochaine mesure porte sur la mise en cache du
  proxy.
* **Rien ne change pour la CLI ni le SDK.** `preview_live` s'ajoute à côté de
  `preview_before` et de la surimpression : la troisième *vue* du moteur, avec
  la même règle — rien de caché, rien d'enregistré.
* **Une session est ouverte puis abandonnée à chaque rendu live.** C'est
  explicitement ce que son contrat permet ; si cela devenait coûteux, la
  réponse serait de garder la session ouverte pendant le geste, pas de
  renoncer au rendu.

## Alternatives écartées

* **Garder le rendu au relâchement** (l'état actuel). C'est l'aveuglement
  décrit en contexte, et il ne se justifiait que par un coût de rendu qu'ADR
  0041 a divisé par cinq.
* **Rendre dans un fil d'arrière-plan** et afficher quand c'est prêt. Ce sera
  peut-être la suite — à 59 ms l'image accuse un retard perceptible sur le
  doigt — mais pas avant d'avoir tenté la mise en cache du proxy (§3), qui
  s'attaque à la cause plutôt qu'à sa perception et n'introduit aucun ordre
  d'arrivée à gérer. Un rendu asynchrone doit garantir qu'une image périmée
  n'écrase pas une plus récente ; c'est de la complexité qu'on ne prend que
  si le coût par image résiste.
* **Mettre les déplacements en file** plutôt que d'ignorer ceux qui arrivent
  trop tôt. Rejouerait le chemin du curseur après coup, en retard sur le
  doigt : le contraire de ce que la décision cherche.
* **Commit à chaque déplacement**, en s'appuyant sur la fenêtre d'amendement
  pour les fondre. Ferait dépendre l'intégrité de l'historique d'un réglage de
  durée, écrirait dans le catalogue à chaque pixel de souris, et invaliderait
  le cache d'aperçus en continu.
