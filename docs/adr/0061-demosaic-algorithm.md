# ADR 0061 — Choix de l'algorithme de dématriçage

**Statut :** Accepté — 2026-08
**Suite :** `input::v3`, que cet ADR crée, n'est plus la version courante :
[ADR 0066](0066-sensor-white-level.md) fait venir le niveau de blanc du capteur
plutôt que du contenu de la photo (`v4`). Le choix de dématriçage décidé ici
traverse ce changement sans bouger.

## Contexte

Le dématriçage est la toute première décision de rendu : reconstruire trois
canaux par pixel à partir d'un capteur qui n'en mesure qu'un. Son choix se voit
sur le détail fin et les motifs répétitifs — feuillage, tissu, maçonnerie — où
un algorithme produit du moiré là où un autre n'en produit pas.

Leyline ne le choisit pas. `params.user_qual` n'est ni exposé ni même écrit :
on prend le défaut de LibRaw, silencieusement.
[ADR 0050](0050-highlight-reconstruction.md) avait laissé la question ouverte
en toutes lettres. `docs/competitive-plan.md` §A2 la reprend comme le levier de
qualité le moins cher du projet : il est **déjà dans la dépendance**, il ne
reste qu'à le piloter.

Deux faits ont été vérifiés avant de décider, et ils réduisent tous deux le
périmètre par rapport à ce que le plan supposait.

**AMaZE et LMMSE ne sont pas disponibles.** Ils vivent dans les *demosaic
packs* GPL2/GPL3, retirés de la distribution principale de LibRaw depuis la
0.19 et absents de la bibliothèque liée ici (0.20.2 : `user_qual` et
`dcb_iterations` sont présents, aucun symbole de pack ne l'est). Le plan citait
RawTherapee, qui les embarque séparément. Les proposer reviendrait à offrir un
choix qui retombe en silence sur AHD — pire que de ne pas l'offrir.

**Le dématriçage n'a aucun effet sur les petites previews.** Les classes
`Thumbnail` et `Small` décodent en `half_size` (`preview.rs`), et le demi-format
de LibRaw prend un pixel par groupe de Bayer 2×2 : **l'interpolation est
purement et simplement court-circuitée**. Le réglage ne change donc rien tant
qu'on n'est pas en `Medium` ou au-delà, ni à l'export. Ce n'est pas un défaut à
corriger — c'est ce qui rend la navigation rapide — mais c'est un fait que
l'interface doit dire, sous peine de proposer un curseur qui « ne fait rien ».

## Décision

**Le dématriçage devient un réglage nommé, écrit dans la révision, porté par une
nouvelle version de l'étage `input`.**

### 1. Quatre algorithmes, pas sept

`Settings` gagne un champ `demosaic`, dont les valeurs sont nommées par ce
qu'elles font, jamais par le numéro de LibRaw :

| Valeur | LibRaw | Pourquoi elle est là |
|---|---|---|
| `ahd` *(défaut)* | 3 | Le défaut historique de LibRaw et de Leyline. Bon partout, excellent nulle part. |
| `vng` | 1 | Doux sur les dégradés, moins de labyrinthe sur les zones unies. |
| `dcb` | 4 | Meilleur rendu des bords nets ; le choix quand le moiré gêne. |
| `dht` | 11 | Le plus fin sur le détail à haute fréquence, le plus lent. |

Écartés délibérément : **AMaZE et LMMSE**, indisponibles (voir Contexte) ;
**linéaire (0) et PPG (2)**, strictement moins bons que AHD sans être assez
rapides pour que ça compte, le chemin preview tenant déjà la vitesse par son
proxy ; **AAHD (12)**, trop proche d'AHD pour justifier une cinquième entrée
dans une liste que l'utilisateur doit pouvoir parcourir d'un coup d'œil.

### 2. `input::v3`, et le refus qui va avec

Changer le dématriçage change les pixels. C'est donc une **nouvelle version de
l'étage `input`**, jamais une modification de `v2` : les révisions existantes
citent `v1` ou `v2` et continuent de rendre exactement comme aujourd'hui
(`docs/pipeline.md` §5.1).

Le défaut de `v3` reste **AHD**, pour que passer une révision en `v3` sans
toucher au réglage ne déplace aucun pixel. Choisir un « meilleur » défaut aurait
fait diverger les nouvelles photos des anciennes sans que personne ne l'ait
demandé.

Un réglage non-AHD sur une révision épinglée en `input: 1` ou `2` est **refusé
par `validate()`**, en nommant la version qu'il faudrait — la règle de capacité
déjà appliquée par ADR 0050 à `highlight_reconstruction`. Jamais un silence,
jamais un repli discret.

### 3. Ce que l'interface doit dire

Le réglage vit dans le groupe Détail, à côté de la réduction de bruit, et
**annonce lui-même qu'il ne se voit pas à cette taille d'aperçu** tant que la
preview affichée est `Thumbnail` ou `Small`. Un réglage dont l'effet est
invisible sans explication est un réglage qu'on croit cassé.

Les trois clients l'exposent, comme tout le reste du pipeline : Studio, la CLI
(`leyline develop <version> demosaic <ahd|vng|dcb|dht>`) et le SDK.

## Conséquences

* Une version d'étage de plus (`input::v3`), donc une entrée de plus dans les
  rendus de référence de `stages/golden.rs`, et les précédentes inchangées.
* `DecodeParams` gagne un champ, et le shim C un paramètre — même forme que ce
  qu'ADR 0050 a fait pour `highlight`.
* Le champ entre dans `settings_json` et dans le groupe de presets Détail.
* **Le bénéfice ne se voit qu'en `Medium` et au-delà, et à l'export.** Aucune
  mesure de qualité ne sera donc concluante sur une petite preview.
* `dcb_iterations` et `dcb_enhance_fl` restent à leur défaut : ce sont des
  réglages d'un seul algorithme, et les exposer ferait entrer une arborescence
  d'options là où le projet veut une liste plate.

## Alternatives écartées

* **Ne rien exposer et changer le défaut** pour un algorithme jugé meilleur :
  déplace les pixels de tout le monde sans le dire, et prive quand même
  l'utilisateur du choix. Le pire des deux mondes.
* **Exposer les sept valeurs de LibRaw**, y compris celles qui retombent sur
  AHD faute de pack GPL : un menu qui ment.
* **Compiler LibRaw avec les demosaic packs GPL2/GPL3** pour offrir AMaZE :
  imposerait une compilation maison de LibRaw sur les trois plateformes, là où
  ADR 0004 tient à un `.so` système substituable, et rouvrirait une question de
  licence tranchée. À reprendre par son propre ADR si la demande vient.
* **Un réglage global d'application plutôt que par photo** : contredirait la
  reproductibilité — une révision doit porter tout ce qui décide de ses pixels
  (`docs/pipeline.md` §5.1), et un réglage hors révision est précisément ce que
  cette garantie interdit.
