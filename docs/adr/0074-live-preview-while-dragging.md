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
1024×683. C'est trois images par battement de paupière.

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
l'aval, et le décodage vient du `DecodeCache`. C'est ce qui fait tenir les
~14 ms.

Le rendu étant **synchrone** sur le fil de l'interface, un événement de
déplacement attend le rendu précédent. Une borne suffit donc, et une seule :
**au plus un rendu live toutes les 40 ms** (25 images par seconde au maximum,
la limite étant l'œil et non la machine). Un déplacement arrivé pendant ce
délai est ignoré — jamais mis en file : ce qui compte est la position
**actuelle** du curseur, pas le chemin parcouru pour y arriver. Le rendu final,
lui, est garanti par le commit du relâchement.

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
* **Le pire cas reste borné par le cache d'étages.** Un curseur amont
  (exposition, balance des blancs) rejoue davantage d'étages qu'un curseur
  aval ; c'est la latence qu'ADR 0041 a mesurée et acceptée, et la borne des
  40 ms empêche une accumulation d'événements de la transformer en retard
  cumulé.
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
* **Rendre dans un fil d'arrière-plan** et afficher quand c'est prêt. Correct,
  et prématuré : à 14 ms le rendu synchrone ne se voit pas, et un rendu
  asynchrone demande de gérer l'ordre d'arrivée des images pour ne pas
  afficher une valeur périmée après une plus récente. À reprendre si un
  curseur amont, sur un boîtier 60 Mpx, sort de la bande acceptable.
* **Mettre les déplacements en file** plutôt que d'ignorer ceux qui arrivent
  trop tôt. Rejouerait le chemin du curseur après coup, en retard sur le
  doigt : le contraire de ce que la décision cherche.
* **Commit à chaque déplacement**, en s'appuyant sur la fenêtre d'amendement
  pour les fondre. Ferait dépendre l'intégrité de l'historique d'un réglage de
  durée, écrirait dans le catalogue à chaque pixel de souris, et invaliderait
  le cache d'aperçus en continu.
