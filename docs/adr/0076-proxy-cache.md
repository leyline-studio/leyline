# ADR 0076 — Le proxy d'affichage se met en cache

**Statut :** Accepté — 2026-08

## Contexte

[ADR 0074](0074-live-preview-while-dragging.md) §3 a branché le rendu sur le
geste, l'a mesuré sur de vrais fichiers, et s'est terminée sur une phrase qui
désigne la suite :

> « Le poste dominant est identifié et n'est pas le pipeline : la prochaine
> mesure porte sur la mise en cache du proxy. »

Le proxy est le buffer d'[ADR 0041](0041-interactive-preview-rendering.md) §1 :
le décodage réduit à la taille de la classe d'aperçu **avant** d'entrer dans le
pipeline. ADR 0041 décide de réduire avant de développer ; elle ne dit rien de
ce qu'il advient du résultat. Il n'en advenait rien — `preview::proxy`
rebâtissait le buffer réduit à chaque appel, y compris pour les cinquante appels
d'un glissement de curseur sur la même photo, à la même classe, depuis le même
décodage.

C'est un calcul **entièrement redondant** : le proxy est une fonction pure du
fichier source et de la taille demandée. Ni les réglages, ni la révision, ni la
version d'étage n'y entrent. Il a donc exactement la nature de ce que
`DecodeCache` garde déjà — et il était le seul terme du chemin live à n'être
gardé nulle part.

### Ce que ça coûtait, mesuré

`live_preview_keeps_up_with_a_finger` (`--release`, aperçu `Small`, i9-9900K),
sur deux fichiers du corpus réel, moyenne de 20 images après la première :

| Fichier | Curseur | Avant |
|---|---|---|
| Canon 60D, 10 Mpx | exposition (rang 40) | 55,4 ms |
| Canon 60D, 10 Mpx | accentuation (rang 190) | 56,8 ms |
| Canon 5D IV, 30 Mpx | exposition | 48,6 ms |
| Canon 5D IV, 30 Mpx | accentuation | 50,1 ms |

Ce tableau dit deux choses. D'abord que le curseur de **fin** de pipeline coûte
autant que celui de tête, alors qu'ADR 0041 §3 mesurait 14 ms contre 60 sur ce
même écart : le cache d'étages fait bien son travail, mais il ne porte plus que
sur une fraction du temps. Ensuite qu'un 30 Mpx ne coûte pas plus qu'un
10 Mpx — les deux sont ramenés au même buffer de 1024 px avant de développer.
Les deux observations pointent le même terme : la réduction elle-même, refaite
à chaque image, indépendante de tout ce qui la suit.

## Décision

### 1. Le proxy est gardé, là où le décodage est déjà gardé

`DecodeCache` cesse d'être un cache de décodages pour devenir un cache de
**buffers source** : les décodages, et les proxies qui en dérivent. Deux listes
MRU dans le même objet, sous le même verrou, jetées ensemble avec la `Library`.

Une entrée de proxy est indexée par `(asset, DecodeParams, max_edge)` :

* `DecodeParams` est déjà la clé du décodage — il porte `half_size` et tout ce
  que la version d'étage `input` demande au décodeur (ADR 0050, 0061, 0066).
  Le proxy ne peut donc pas survivre à un changement qui modifierait le buffer
  dont il dérive ;
* `max_edge` est la classe d'aperçu, seul autre paramètre de la réduction.

Rien d'autre n'entre dans la clé, parce que rien d'autre n'entre dans le
calcul. C'est ce qui rend ce cache sûr : comme celui d'étages, il est purement
**dérivé**, et le jeter à tout instant ne change aucun pixel.

**Un succès saute la réduction *et* le décodage.** Le proxy se suffit à
lui-même : il survit au buffer décodé dont il est issu si celui-ci est évincé
en premier. C'est voulu — garder 4 Mo pour éviter de garder 90 Mo est le bon
échange sur le chemin d'aperçu.

`PreviewKind::Full` n'a pas de `max_edge` et n'a donc pas de proxy : ce chemin
rend le décodage lui-même, à l'échelle 1,0, comme avant. Une image déjà assez
petite ne paie pas non plus une seconde copie — c'est le buffer décodé qui est
enregistré comme son propre proxy.

### 2. Le plafond est en octets, pas en entrées

Les classes d'aperçu s'étalent sur 250× : un proxy `Thumbnail` pèse 0,26 Mo,
un `Small` 4,2 Mo, un `Large` 67 Mo. Un plafond en nombre d'entrées voudrait
donc dire deux choses incompatibles selon la classe. La liste de proxies se
borne en **mémoire — 64 Mo**, ce qui tient une douzaine de `Small` (la classe
de la vue develop, celle du glissement) ou un seul `Large`.

**L'entrée la plus récente est toujours conservée**, quelle que soit sa taille :
évincer le buffer que l'appelant s'apprête à utiliser ne rendrait pas la
mémoire et perdrait le cache.

Le décodage, lui, garde son plafond en entrées (2) : ses tailles ne varient que
d'un facteur 3, et ce plafond-là est déjà écrit et compris.

### 3. Ce qui ne change pas

* **Aucun pixel.** Le proxy servi est octet pour octet celui qu'une réduction
  fraîche produirait, comme le décodage servi est celui d'un décodage frais.
  `docs/pipeline.md` §5 n'est pas en jeu, ADR 0012 non plus.
* **L'export et l'impression** ne passent pas par là : ils rendent à pleine
  résolution, sans proxy (ADR 0041 §Décision).
* **La borne des 40 ms** d'ADR 0074 §3. Elle ne mordait pas ; elle mord
  désormais, ce qui est exactement ce qu'ADR 0074 annonçait.

## Conséquences

**Mesuré, mêmes fichiers, même machine, même test :**

| Fichier | Curseur | Avant | Après | |
|---|---|---|---|---|
| Canon 60D, 10 Mpx | exposition | 55,4 ms | **15,5 ms** | −72 % |
| Canon 60D, 10 Mpx | accentuation | 56,8 ms | **14,4 ms** | −75 % |
| Canon 5D IV, 30 Mpx | exposition | 48,6 ms | **11,3 ms** | −77 % |
| Canon 5D IV, 30 Mpx | accentuation | 50,1 ms | **12,6 ms** | −75 % |

* **On passe de ~18 à ~70 images par seconde** sur le rendu lui-même. C'est
  au-delà de ce que la borne des 40 ms laisse passer : le curseur est désormais
  limité par la décision d'ADR 0074 (25 images/s, « la limite étant l'œil et
  non la machine ») et non plus par le coût du rendu. Le retard perceptible sur
  le doigt que notait ADR 0074 disparaît, et avec lui la raison qu'elle donnait
  d'envisager un rendu asynchrone : à 14 ms, la complexité d'un ordre d'arrivée
  à gérer ne se paie plus.
* **Le curseur de fin de pipeline redevient le moins cher**, de peu. Le cache
  d'étages d'ADR 0041 §3 reprend la place qu'il avait dans sa propre mesure :
  ce qui le masquait a disparu.
* **64 Mo de plus au pire**, à côté des ~144 Mo que le cache de décodages peut
  déjà tenir. Le chiffre est un plafond, pas une consommation : une session de
  develop sur une photo tient un `Small` et un `Thumbnail`, soit ~4,5 Mo.
* **Un troisième cache dérivé sur le chemin d'aperçu**, après le décodage et
  les étages. Tous trois partagent la même propriété et c'est ce qui les rend
  tenables : les jeter est toujours correct, jamais nécessaire. Aucun n'est
  persistant, aucun n'a d'invalidation à écrire — la clé décrit intégralement
  le calcul.
* **Le poste dominant du rendu live n'est plus identifié.** Les ~14 ms
  restants se répartissent entre le pipeline aval, la conversion en 8 bits et
  l'affichage ; aucun ne ressort assez pour justifier une mesure de plus tant
  que la borne des 40 ms est ce qui limite. La prochaine question de perf
  d'aperçu n'est plus celle-ci.

## Alternatives écartées

* **Garder le proxy dans la session d'édition**, comme ADR 0041 §3 l'avait
  d'abord prévu pour le cache d'étages. Même erreur, corrigée au même endroit :
  la vue develop rend par `Library::preview_live`, qui ouvre et abandonne une
  session par image (ADR 0074 §2). Un cache porté par la session serait vide à
  chaque appel.
* **Un cache de proxies séparé de `DecodeCache`.** Deux objets, deux verrous,
  un paramètre de plus à traverser quatre fonctions — pour deux listes dont
  l'une est calculée à partir de l'autre et dont la clé partage `DecodeParams`.
  Le coût de plomberie ne payait rien.
* **Ne garder que le proxy et jeter le décodage.** Séduisant sur le chemin
  d'aperçu (le proxy s'y suffit) et faux dès qu'on en sort : `PreviewKind::Full`
  et un changement de classe d'aperçu repartent du buffer décodé, qu'il faudrait
  alors re-décoder — ~1 s, contre 90 Mo gardés.
* **Un plafond en nombre d'entrées**, comme celui du décodage. Il aurait fallu
  le dimensionner soit pour `Large` (et ne garder qu'un `Small`), soit pour
  `Small` (et laisser passer 800 Mo de `Large`). L'écart de 250× entre les
  classes rend le nombre d'entrées dénué de sens ici.
* **Persister les proxies sur disque.** Même réponse qu'ADR 0041 pour les
  étages : la réduction coûte moins que sa sérialisation et sa relecture, et
  cela ajouterait un artefact de cache à invalider entre versions du moteur.
