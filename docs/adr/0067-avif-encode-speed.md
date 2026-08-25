# ADR 0067 — La vitesse d'encodage AVIF est un réglage, et son défaut était le mauvais

**Statut :** Accepté — 2026-08

## Contexte

`leyline-export` encode l'AVIF avec `ravif`, et lui passe une constante :

```rust
ravif::Encoder::new()
    .with_quality(f32::from(settings.quality))
    .with_speed(6)
```

Ce `6` est le seul réglage d'encodeur du projet que personne ne peut voir ni
changer. Il n'a jamais été choisi : c'est une valeur d'exemple, arrivée avec le
code qui l'entoure. Or [le relevé B2](../measured-findings.md) a montré que
l'AVIF coûte **25× le JPEG** à taille égale — c'est le format où un curseur
d'encodeur pèse le plus lourd, et le seul où il est caché.

### Ce que le curseur fait réellement

Chemin d'export complet — `leyline export --format avif`, décodage, rendu et
encodage compris — sur une photo du corpus réel, 18,0 Mpx, qualité 90 :

| `avif_speed` | Temps | CPU | Fichier |
|---|---|---|---|
| 4 | 12,03 s | 97,6 s | 1 562 ko |
| **6 — la constante d'aujourd'hui** | **8,61 s** | **52,5 s** | **1 616 ko** |
| 8 | 8,29 s | 52,2 s | 1 620 ko |
| 9 | 6,24 s | 24,8 s | 1 637 ko |
| 10 | 2,26 s | 10,9 s | 1 803 ko |

Deux choses en sortent.

**1. Le point 6 n'a rien de particulier à défendre.** Passer à 9 rend **28 % du
temps et 53 % du CPU pour 1,3 % de poids**. Aucun arbitrage raisonnable ne
préfère 6 à ce marché-là ; la constante n'a d'ailleurs jamais été choisie pour
en trancher un.

**2. Le bon compromis dépend de la photo.** Le même relevé, encodage isolé
(pixels déjà passés par un JPEG, donc plus faciles à comprimer), inverse le
signe : à 9 le fichier y était **plus petit** qu'à 6 (1 517 ko contre 1 718 sur
18 Mpx ; 1 973 contre 2 008 sur 12 Mpx). Et le prix de la vitesse 10 varie de
0 à 14 % du poids selon l'image — la table de B2, mesurée sur une autre photo,
donnait +14 %, celle ci-dessus donne +10 %.

C'est précisément l'argument contre une constante : quelqu'un qui exporte un
lot de contrôle veut la vitesse 10, quelqu'un qui prépare une galerie en ligne
ne la veut pas, et le bon réglage dépend en plus de ce qu'il y a sur la photo.
Un nombre écrit en dur tranche pour tout le monde.

### Ce que le curseur ne fait pas

Il ne change pas l'image. La qualité visée reste celle de `with_quality` ; la
vitesse ne décide que de l'effort de recherche de l'encodeur — donc du poids
obtenu à cette qualité-là, pas de l'image. Vérifié plutôt qu'affirmé — PSNR
entre le résultat de chaque vitesse et celui de la vitesse 6 : **46,3 à
50,8 dB**, soit au-dessus du seuil de
discernement, et la vitesse 10 n'est pas plus éloignée de 6 que ne l'est la
vitesse 1 (49,0 dB). Contre la source, les six vitesses sont à 0,001 dB les
unes des autres.

C'est ce qui range cette décision hors du périmètre de `pipeline.md` §5.1 : la
promesse porte sur **le rendu** — les pixels que le pipeline produit, gelés par
des versions d'étages. L'encodeur est en aval, il reçoit ces pixels déjà
calculés. Aucun étage, aucune version d'étage n'est en cause ici.

## Décision

**La vitesse d'encodage AVIF devient un champ d'`ExportSettings`, et son défaut
passe de 6 à 9.**

### 1. Le champ

`ExportSettings.avif_speed`, entier de 1 à 10, refusé hors de cet intervalle
par `validate()` comme l'est déjà `quality`. Ignoré par tous les autres
formats, exactement comme `quality` l'est par les formats sans perte — la
structure décrit une recette d'export, pas un codec.

Il est **toujours sérialisé**, y compris dans un preset JPEG. C'est délibéré :
un preset écrit aujourd'hui épingle sa vitesse, donc il produira le même
fichier quand le défaut bougera de nouveau. Le bruit dans le JSON est le prix
de cette propriété, et `format` comme `quality` sont déjà écrits sans condition.

### 2. Le défaut : 9, pas 10

9 est le meilleur marché qu'on puisse imposer à quelqu'un qui n'a rien demandé :
**1,3 % de poids en plus, 28 % de temps et 53 % de CPU en moins**. Un poids
qui bouge d'un centième ne change la décision de personne ; un export deux fois
moins cher en CPU, si.

10 va bien plus vite encore (~4× le défaut), mais son coût en poids varie de
0 à 14 % selon l'image, et l'AVIF est choisi *pour* sa compacité — quelqu'un
qui accepte 25× le temps d'un JPEG le fait pour obtenir un petit fichier. Lui
reprendre 14 % de ce bénéfice par défaut, sans qu'il l'ait demandé, prendrait
la décision à sa place. Le champ du §1 la lui rend : 10 est à un mot.

### 3. Ce que cela change aux presets déjà enregistrés

Un preset AVIF existant ne porte pas le champ, donc il reçoit 9 à la lecture :
**ses prochains exports seront des fichiers différents** — plus rapides, à peu
près du même poids, visuellement identiques (§Contexte). C'est assumé : un
export est un artefact dérivé, régénérable à volonté, et rien dans le catalogue
n'en dépend. Aucune révision, aucune photo, aucun réglage de développement
n'est touché.

## Conséquences

* Un lot AVIF s'exporte **~28 % plus vite pour moitié moins de CPU** sans que
  personne ne change de réglage, et ~4× plus vite pour qui met le curseur à 10.
* Le champ traverse les trois clients : `--avif-speed` dans la CLI (export et
  création de preset), un champ dans le dialogue d'export de Studio, le SDK
  par simple ré-export.
* `docs/catalog.md` §27 gagne la description du champ dans `settings_json`.
* **Le seul réglage d'encodeur caché du projet disparaît.** S'il en réapparaît
  un, la question à poser est celle-ci : est-ce un arbitrage que l'utilisateur
  peut vouloir trancher autrement ?

## Alternatives écartées

* **Ne changer que le défaut, sans exposer le champ.** Corrigerait le point le
  plus visible et laisserait le vrai défaut en place : un arbitrage
  temps/poids, qui dépend de l'usage et de l'image, décidé une fois pour toutes
  dans le code.
* **Défaut à 10.** Écarté au §2 : reprend une part variable du bénéfice qu'on
  vient chercher en choisissant l'AVIF, sans que l'utilisateur l'ait demandé.
* **Un champ générique `speed` pour tous les formats.** Aucun autre encodeur du
  projet n'en a la notion ; un champ que quatre formats sur cinq ignorent
  promettrait un réglage qui n'existe pas.
* **Déduire la vitesse de la taille de l'image** (rapide sur les grandes,
  lent sur les petites). Une heuristique non écrite qui reprendrait la décision
  à l'utilisateur sous une autre forme, en plus difficile à prévoir.
