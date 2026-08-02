# ADR 0062 — Interpoler les illuminants de calibration d'un profil DCP

**Statut :** Accepté — 2026-08

**Amende :** [ADR 0035](0035-camera-profile-dcp.md) (la simplification de §Décision), [ADR 0037](0037-dcp-parsing-dependency.md)

## Contexte

Un profil DCP est calibré sous **deux illuminants** : typiquement `Standard
Light A` (tungstène, 2850 K) et `D65` (lumière du jour, 6500 K). Il porte donc
deux jeux de matrices, et la spec DNG dit d'**interpoler entre eux selon la
température de la scène** — une photo au tungstène doit être développée avec la
calibration tungstène.

Leyline **moyenne les deux matrices**, quelle que soit la lumière. C'est une
simplification que le module `dcp.rs` documente depuis ADR 0035, et qu'ADR 0035
avait assumée faute de savoir ce qu'elle coûtait.

Elle a été mesurée le 2026-08-02, sur les deux profils Canon réels dont le
projet dispose. L'écart entre la moyenne et la calibration correcte, en sortie
sRGB linéaire sur `[0, 1]` :

| Échantillon | Canon 60D | Canon 5D Mark IV |
|---|---|---|
| gris neutre | 0,0000 | 0,0001 |
| peau claire | 0,016 | 0,012 |
| ciel | 0,032 | 0,019 |
| rouge saturé | **0,044** | 0,028 |

**L'axe neutre est intact** — c'est ce qui a permis à l'erreur de passer
inaperçue — mais 0,044 vaut 11 niveaux sur 255. C'est visible sur un aplat, et
c'est un biais systématique, pas du bruit.

Les deux profils déclarent bien les deux illuminants, donc le cas « moyenne »
est le cas courant, pas un cas limite.

## Décision

**Un profil garde ses deux jeux de matrices, et la matrice est résolue au
rendu, pas au parsing.**

### 1. La formule

Celle de la spec DNG, vérifiée contre le code de référence du DNG SDK tel que
le reprend RawTherapee (`rtengine/dcp.cc`) :

```
mix = (1/T − 1/T₂) / (1/T₁ − 1/T₂),  borné à [0, 1]
M   = mix · M₁ + (1 − mix) · M₂
```

L'interpolation se fait sur **l'inverse de la température** — en mireds, la
grandeur où l'écart de couleur est perceptuellement linéaire. Interpoler sur
les kelvins donnerait un résultat faux au milieu de l'intervalle, et c'est
l'erreur qu'on commet naturellement.

Les températures des illuminants viennent de la table du DNG SDK : illuminant
17 (`Standard Light A`) → **2850 K**, 21 (`D65`) → **6500 K**. Ce sont les
valeurs du code de référence, pas les valeurs physiques exactes (2856 K pour
l'illuminant A) : c'est celles-là qu'il faut, puisque le but est de produire le
même mélange que la référence.

### 2. D'où vient la température de la scène

C'est le point où Leyline a un raccourci que les autres implémentations n'ont
pas, et il faut le dire : **notre `settings.white_balance` porte déjà une
température en kelvins**. Quand la révision en nomme une, c'est elle, sans
détour et sans approximation.

Reste le cas **« comme à la prise de vue »** (`white_balance: None`), qui est
l'état de toute photo fraîchement importée — donc le cas majoritaire, pas un
cas limite. Là, la température se déduit des multiplicateurs du boîtier que
`SourceColor::Camera { multipliers }` transporte déjà, par le chemin que la
spec décrit : neutre caméra → XYZ → coordonnées *xy* → température, cette
dernière étape par la table isotherme de Robertson (31 entrées, colorimétrie
publiée, reprise telle quelle du DNG SDK).

La conversion neutre → *xy* est itérative : elle a besoin de la matrice pour
trouver le point blanc, et du point blanc pour choisir la matrice. Quelques
passes suffisent, et le DNG SDK en plafonne le nombre — on fait de même.

**Quand rien n'est disponible** — ni température nommée, ni multiplicateurs —
on retombe sur D65 plutôt que sur la moyenne, et on le documente. Une
calibration à un bout de l'intervalle est un choix défendable ; une moyenne
n'en est pas un, elle ne correspond à aucune lumière réelle.

### 3. `camera_profile::v2`

Les pixels changent, donc c'est une nouvelle version d'étage. Les révisions
existantes citent `v1` et continuent de rendre exactement comme aujourd'hui
(`docs/pipeline.md` §5.1), moyenne comprise.

`DcpProfile` cesse d'exposer une matrice unique résolue à la lecture ; il porte
ce que le fichier contient, et une méthode qui résout pour une température
donnée. C'est un changement de forme, pas seulement de valeur : la matrice
n'est plus une propriété du profil, elle est une propriété du couple
(profil, lumière).

### 4. Le piège du cache d'étages

`camera_profile` dépendait de la seule clé `camera_profile`. Il dépend
désormais **aussi de `white_balance`**, et son `Stage::reads` doit le dire
([ADR 0041](0041-interactive-preview-rendering.md) §3). Sans cela, changer la
température réutiliserait un point de contrôle calculé sous l'ancienne, et
mettrait des pixels faux à l'écran — silencieusement.

C'est exactement le genre d'omission que `reads` rend possible, et la raison
pour laquelle sa documentation dit qu'une clé manquante est un bug de
correction et non de performance.

## Conséquences

* Une version d'étage de plus, donc une entrée de plus dans les rendus de
  référence, les précédentes inchangées.
* Le rendu d'une photo au tungstène avec un profil DCP change — en mieux, et
  seulement après retraitement vers `camera_profile::v2`.
* La table de Robertson entre dans `leyline-color`. Trente-et-une lignes de
  données publiées, sans dépendance nouvelle.
* La mention « expérimental » d'ADR 0035 ne bouge pas : elle porte sur la
  concordance avec le *rendu* d'Adobe, que rien ici ne vérifie.

## Alternatives écartées

* **Garder la moyenne.** Mesurée, elle coûte jusqu'à 11 niveaux sur 255 sur des
  couleurs saturées, et ne correspond à aucune lumière physique.
* **Toujours prendre l'illuminant le plus proche** sans interpoler : évite la
  table de Robertson, mais fait sauter le rendu d'un profil à l'autre au
  franchissement d'un seuil, alors que le curseur de température est continu.
* **Résoudre au parsing avec la température de la révision**, en gardant une
  matrice unique : semble plus simple, mais oblige à re-parser le fichier à
  chaque mouvement du curseur de balance des blancs, et fait dépendre un objet
  « profil » d'une photo. La forme suivrait mal le sens.
* **Utiliser la température exacte de l'illuminant A (2856 K)** plutôt que les
  2850 K du DNG SDK : plus juste physiquement, et faux pour ce qu'on cherche —
  reproduire le mélange de la référence.
