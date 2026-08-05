# ADR 0035 — Profil caméra (DCP) : nouvel étage colorimétrique en tête de pipeline, fichiers fournis par l'utilisateur, référencés et checksummés

**Statut :** Accepté — 2026-07
**Suite :** `camera_profile::v1`, que cet ADR crée, n'est plus la version
courante. [ADR 0062](0062-dcp-illuminant-interpolation.md) remplace la
simplification de §Décision par une vraie interpolation des deux illuminants de
calibration (`v2`), et [ADR 0063](0063-dcp-tables.md) applique enfin les tables
du profil — `HueSatMap` et `LookTable` (`v3`). Le modèle décidé ici — un étage
colorimétrique en tête de pipeline, des fichiers fournis par l'utilisateur,
référencés et checksummés — est inchangé.

## Contexte

`docs/v2-scope.md` §8 relève qu'aucune calibration couleur caméra de type DCP
n'existe aujourd'hui. La correction d'objectif V1 (Lensfun, ADR 0016–0018,
process 3–5) couvre distorsion/vignettage/TCA **géométriques**, pas la
**couleur** du capteur. Un profil DCP calibre le rendu couleur du capteur
(matrices colorimétriques, tables TSL, courbe tonale, *look table*) — donc
très tôt dans le pipeline, à la conversion RGB capteur → espace de travail.

Deux décisions transversales sont **consommées, non re-litigées, ici** :

* **ADR 0027** a élargi `leyline-color` d'« exposer un profil statique »
  (ADR 0015) vers « charger des profils ICC arbitraires et construire des
  `cmsTransform` ». Elle a explicitement placé l'authoring DCP **hors de sa
  propre décision**, en notant qu'il « agit sur la conversion capteur →
  espace de travail, au **début** du pipeline (un véritable événement de
  process version) », mais que cet item pourra désormais « supposer que
  `leyline-color` sera déjà une bibliothèque de transformation ICC générale ».
  Ce document est cet ADR annoncé par ADR 0027.
* **ADR 0028** fige la stratégie de versionnage : une process version par
  fonctionnalité pixel, chacune dans son propre module `processN.rs` gelé,
  créé en copiant le module précédent entier. Le profil caméra est un
  opérateur pixel : il prend donc une nouvelle process version sans que cet
  ADR ait à re-choisir la convention.

`docs/v2-scope.md` §8 laisse trois questions ouvertes propres à l'item : la
dépendance de parsing DCP, l'interaction avec le gel sRGB (résolue en partie
par ADR 0027), et la reproductibilité d'un chemin couleur entièrement nouveau.
Ce document tranche le **placement pipeline, le crate propriétaire, la source
des profils et leur reproductibilité** ; il laisse explicitement ouverte la
**dépendance de parsing**.

## Décision

### Un nouvel étage « Profil caméra », tout premier du pipeline — avant même la correction d'objectif

Un nouvel étage **« Profil caméra »** s'insère dans l'ordre fixe de
`docs/pipeline.md` §3.1 **entre RAW décodé et Correction d'objectif** — c'est
le **tout premier** étage, avant même la correction géométrique d'objectif.

Cela **précise** l'esquisse du §8 (« entre RAW décodé et Balance des blancs »)
en fixant aussi l'ordre **relatif à la correction d'objectif** : un DCP
calibre la réponse **couleur** du capteur (matrices, tables TSL, courbe
tonale, look table) ; la correction d'objectif traite la **géométrie**
(distorsion, vignettage, TCA). Les deux **n'interagissent pas** — l'un remappe
des couleurs, l'autre remappe des positions. Placer la calibration
colorimétrique en tout premier la fait opérer sur les **données les moins
traitées possibles** (le RGB capteur juste décodé), cohérent avec les autres
décisions de placement « opérer sur les données les moins traitées » de cette
série d'ADR — la suppression de tache d'ADR 0032, placée tôt « pour opérer sur
des données proches du linéaire ». Un DCP appliqué après un remapping
géométrique n'aurait aucun sens colorimétrique de plus, et introduirait une
dépendance d'ordre inutile entre deux opérateurs qui n'en ont aucune.

C'est un **véritable événement de réordonnancement** — un nouvel étage inséré
avant l'étage actuellement premier. `docs/pipeline.md` §3.1/§3.3 impose qu'une
telle insertion (comme toute étape qui change les pixels produits par une
révision) prenne une **nouvelle process version** au même titre que n'importe
quelle insertion — **process N, le prochain numéro disponible au moment de la
sortie de cette fonctionnalité** (ADR 0028), dans son propre module
`processN.rs` copie intégrale du module précédent augmentée du seul étage de
profil caméra. Cet ADR **ne fige pas** un entier de process précis :
l'ordre de sortie des items V2 relève du plan d'implémentation futur.

### Placement crate — aucun nouveau crate, extension de `leyline-color`

Le parseur DCP et la logique d'application de ses matrices/tables vivent dans
**`leyline-color`**, pas dans un nouveau crate. Raisonnement : ADR 0027 a déjà
établi `leyline-color` comme croissant d'« un profil statique » vers une
bibliothèque générale de transformation couleur. L'application d'un DCP —
recette matrice fixe + LUT, **appliquée directement** (pas via
`lcms2::cmsTransform`, puisque DCP n'est pas de l'ICC, voir plus bas) — est du
travail de **pipeline couleur adjacent**, parallèle au travail ICC déjà logé
là. Elle n'a **pas** besoin du couplage aux tampons du moteur.

Contraste explicite avec deux placements décidés différemment ailleurs dans
cette série, pour deux raisons différentes :

* **Le masquage (ADR 0029) a reçu un module `leyline-engine`, pas un crate** :
  parce que la rastérisation de masque est étroitement **couplée aux internes
  du tampon de rendu et à son échantillonnage** — une frontière de crate
  séparerait deux choses qui doivent partager ces internes.
* **Le profil caméra reçoit une extension `leyline-color`, pas un crate** :
  parce que c'est de la **logique de domaine couleur**, parallèle au travail
  ICC déjà là (ADR 0027), et **non** quelque chose qui a besoin d'un couplage
  aux tampons du moteur. Le module `processN.rs` **appelle** `leyline-color`
  pour transformer les échantillons couleur, comme il appelle déjà
  `leyline-lens` pour la géométrie.

Deux features, deux raisonnements de placement distincts — énoncés ensemble
pour que le contraste soit lisible.

### La dépendance de parsing DCP — risque ouvert explicite, non résolu ici

DCP est le format d'Adobe, fondé sur les **tags TIFF/EP**, **pas** de l'ICC :
`lcms2` ne le parse pas. Que Leyline écrive un **parseur DCP maison minimal**
(les seuls tags nécessaires à l'application) ou **intègre un crate Rust
existant** (s'il en existe un convenable et licenciable au moment venu) est
**laissé à la PR d'implémentation** — cela dépend de ce qui est disponible et
licenciable à ce moment-là. Cet ADR fixe la décision **pipeline/architecture**,
pas le choix de dépendance de parsing.

Dans le même esprit que la prudence d'ADR 0016 (« non triviaux à valider sans
images de référence sous la main » pour vignettage/TCA), la **correctness
colorimétrique** de l'application DCP doit être **validée contre de vrais
fichiers DCP générés par Adobe et leurs rendus de référence** avant toute
sortie — c'est exactement la barre que ce projet s'est déjà fixée pour ce type
d'affirmation. Cet ADR ne prétend pas que le chemin couleur est correct ; il
fixe où il vit et exige sa validation.

### Source des profils — fichiers fournis par l'utilisateur, aucune base embarquée en V2

**Aucune base de profils DCP embarquée en V2.** Contraste explicite avec
Lensfun (ADR 0004/0016) : la correction d'objectif s'appuie sur une **base de
profils ouverte, communautaire, embarquée**, où le matching par chaîne EXIF
contre des milliers de profils fait sens. Les profils **DCP** sont d'une autre
nature : ils sont typiquement **générés par l'utilisateur, boîtier par
boîtier**, via une mire de calibration (ou téléchargés individuellement chez
un tiers) — pas une base ouverte, maintenue par une communauté, que Leyline
pourrait embarquer comme celle de Lensfun. Embarquer une telle base est un
chantier bien plus grand et séparé (droits/licences des données, hébergement,
maintenance), franchement hors périmètre ici.

La V2 laisse donc l'utilisateur **déposer ses fichiers `.dcp`** dans un dossier
de profils **relatif à la bibliothèque** (`docs/catalog.md` §2.3 : chemins
relatifs, jamais absolus, pour la portabilité), par exemple `Profiles/Camera/`,
et les **référence depuis `settings_json` par chemin relatif**. Le profil est
matché **par un chemin explicite stocké**, **pas** auto-matché par le modèle
EXIF de la caméra comme `lens_correction.profile: "auto"` l'est. Raisonnement :
contrairement à la base communautaire de Lensfun où le matching flou d'une
chaîne EXIF contre des milliers de profils embarqués a du sens, le fichier DCP
unique qu'un utilisateur a produit pour son propre boîtier **n'a pas besoin de
matching flou** — une référence explicite est plus simple et plus prévisible.

### Reproductibilité d'un fichier de profil externe référencé — un problème genuinely nouveau

C'est un problème **que ni ADR 0026, ni 0029, ni 0032 n'ont eu à résoudre** :
tous stockent leur géométrie **inline** dans `settings_json`, sans aucune
référence à un fichier externe. Ici, `settings_json` **référence un fichier
`.dcp` hors de lui-même** — un nouvel intrant dont la reproductibilité doit
être garantie.

**Décision : stocker un checksum BLAKE3** (même algorithme qu'ADR 0006,
appliqué à un **nouveau genre de fichier référencé** plutôt qu'à un asset
photo) des octets du fichier `.dcp`, **à côté de son chemin relatif** dans
`settings_json`. Au rendu, si le checksum du fichier courant **ne correspond
pas** au checksum stocké, le moteur **ne doit pas rendre silencieusement avec
un profil modifié**.

Cela **étend** le contrat de reproductibilité de `docs/pipeline.md` §5 — « deux
exécutions sont identiques si et seulement si … la ressource d'entrée est
identique (même `checksum`) », « même révision → mêmes pixels pour toujours » —
à ce **nouveau cas d'un fichier d'entrée référencé de l'extérieur**, exactement
comme le contrat s'applique déjà au checksum de l'asset photo lui-même. Un DCP
est, colorimétriquement, un intrant de rendu au même titre que les pixels
capteur.

**Mode d'échec — pas une catégorie nouvelle.** Un checksum qui ne correspond
pas est traité **comme le moteur traite déjà un `schema`/`process` qu'il ne
reconnaît pas** (`docs/pipeline.md` §3.4) : il **ne modifie jamais la
révision**, **n'édite pas l'asset** (lecture seule), et **affiche la meilleure
préversion disponible avec un avertissement**. Aucune catégorie d'échec
nouvelle n'est inventée : la politique existante « ne détruis pas le travail
antérieur, avertis » est étendue à ce nouveau déclencheur (fichier de profil
manquant ou modifié).

### Stockage / schéma — additif

`camera_profile` est un objet optionnel de `settings_json`. **Absent =
neutre** : le chemin par défaut de LibRaw (sortie sRGB, ADR 0015) inchangé —
exactement le statu quo d'ADR 0015 quand le champ est absent. Aucun bump de
schéma requis, cohérent avec le schéma additif « process +1, schema inchangé »
de la plupart des items V2 (`docs/v2-scope.md` §1) — les champs inconnus d'un
moteur ancien sont préservés verbatim (`Settings::extra`,
`crates/leyline-core/src/settings.rs`).

Esquisse (le style suit `docs/pipeline.md` §3.2) :

```json
{
    "schema": 1,
    "process": 7,

    "exposure": 0.2,
    "camera_profile": {
        "enabled": true,
        "path": "Profiles/Camera/EOS60D-D65.dcp",
        "checksum": "blake3:9f2b…"
    }
}
```

Cas neutre — champ absent, rendu bit-pour-bit identique à la process version
précédente (chemin LibRaw-sRGB par défaut d'ADR 0015) :

```json
{ "schema": 1, "process": 7, "exposure": 0.2 }
```

> *Le `process: 7` ci-dessus est purement illustratif : le numéro réel est le
> prochain disponible au moment de la sortie (ADR 0028), pas fixé par cet ADR.*

> **Note d'implémentation (pas une édition de spec ici).** Cet ADR ne modifie
> **pas** le diagramme de `docs/pipeline.md` §3.1 ni le tableau des process
> versions §3.3. Comme pour ADR 0029–0033, la spec est mise à jour dans le même
> changement que l'implémentation réelle, conformément à CLAUDE.md. Le présent
> document fixe seulement **où** l'étage atterrit (tout premier, avant la
> correction d'objectif), **où** vit son code (`leyline-color`) et **comment**
> sa reproductibilité est garantie (checksum BLAKE3, mode d'échec §3.4) ; le
> diagramme §3.1 et le tableau §3.3 seront amendés par la PR qui livre le
> module process.

> **Note du 2026-08-02, après confrontation à de vrais profils.** Le premier
> `.dcp` authentique essayé a révélé que **aucun** ne pouvait être lu : un
> profil est une IFD nue portant la version `0x4352`, là où le lecteur
> attendait un TIFF standard avec une image. Corrigé par un lecteur d'IFD
> maison — ce que le §Décision décrivait déjà, mais que l'implémentation
> avait délégué à `tiff::Decoder`. Sont désormais vérifiés : la lecture de
> vrais fichiers, et la préservation d'un gris neutre à travers la matrice.
> Reste non vérifiée, et donc la mention « expérimental » reste : la
> concordance avec le *rendu* d'Adobe, qui exige Lightroom ou ACR.

> **Note du 2026-08-03, après comparaison à un rendu indépendant.** Le chemin
> DCP complet — conteneur, matrices, illuminants interpolés ([ADR 0062](0062-dcp-illuminant-interpolation.md))
> et tables ([ADR 0063](0063-dcp-tables.md)) — a été confronté à un rendu
> RawTherapee du même RAW avec le même profil, profil de traitement neutre.
> **Une fois le niveau normalisé, l'écart médian est de 0,0027 sur 1,0**, soit
> moins d'un niveau sur 255, et les rapports de canaux s'accordent à 0,007
> près. La colorimétrie n'est donc plus non validée : elle concorde avec une
> implémentation indépendante et mature de la même spécification.
>
> Ce qui reste, et qui justifie de garder la mention « expérimental » :
>
> * un **gain global de ×1,083** (espace encodé) subsiste, uniforme sur les
>   trois canaux — tonal, pas chromatique. L'enquête du 2026-08-03 a trouvé un
>   vrai défaut de niveau de blanc au passage ([ADR 0066](0066-sensor-white-level.md)) :
>   le décodeur laissait LibRaw choisir ce niveau d'après le pixel le plus clair
>   de chaque image. Sa correction **ne referme pas cet écart-ci** — il reste un
>   facteur ~1,14 non attribué. Ce qui est acquis : **ce n'est pas la couleur**,
>   qui concorde à 0,0027 près ;
> * aucune comparaison à Adobe lui-même, faute de convertisseur disponible.

## Conséquences

* **Le champ `process` garde sa lisibilité sémantique** (ADR 0028) : le nouveau
  numéro signifiera exactement « profil caméra DCP actif », un fait unique et
  lisible, comme `process: 3` signifie « correction de distorsion active ».
* **Sortie neutre gelée** : sans `camera_profile`, l'étage est bit-pour-bit la
  process version précédente — le chemin LibRaw-sRGB d'ADR 0015, inchangé.
  L'invariant « valeur neutre → opérateur entièrement sauté » (`process3.rs`)
  reste vrai pour l'étage entier.
* **`leyline-color` devient le foyer de deux chemins couleur** : la
  transformation ICC de sortie (ADR 0027) et l'application DCP d'entrée (ici) —
  deux logiques de domaine couleur, aucune ne couplée aux tampons du moteur,
  cohérent avec le rôle qu'ADR 0027 lui a donné.
* **La dépendance de parsing DCP reste un risque ouvert** pour la PR : parseur
  maison minimal ou crate existant, selon ce qui est disponible et licenciable
  au moment venu. La correctness colorimétrique doit être validée contre de
  vrais DCP Adobe et leurs rendus de référence avant sortie (barre d'ADR 0016).
* **La reproductibilité d'un fichier de profil externe est désormais couverte**
  par le checksum BLAKE3 (ADR 0006) et le mode d'échec §3.4 — un profil manquant
  ou modifié ne rend jamais silencieusement des pixels différents ; il avertit
  et n'écrit rien. Le contrat « même révision → mêmes pixels » (`docs/pipeline.md`
  §5) tient pour ce nouveau genre d'intrant référencé.
* **La base de profils embarquée reste ouverte pour un futur ADR** avec ses
  vrais coûts (droits, hébergement, maintenance) : la V2 refuse seulement de
  s'y engager spéculativement, elle ne ferme pas la porte.
* **Un module `processN.rs` de plus** (ADR 0028) : coût borné et connu ; aucun
  module de version antérieure n'est touché, le gel « mêmes pixels dans dix ans »
  reste mécaniquement infalsifiable (`docs/pipeline.md` §3.3).

## Alternatives écartées

* **Embarquer une base de profils DCP à la manière de Lensfun.** Écarté : les
  profils DCP sont typiquement générés par l'utilisateur boîtier par boîtier
  (mire de calibration) ou téléchargés individuellement, pas une base ouverte
  et communautaire que Leyline pourrait embarquer comme celle de Lensfun
  (ADR 0004/0016). Bundler une telle base est un chantier bien plus grand et
  séparé — droits/licences des données, hébergement, maintenance — franchement
  hors périmètre V2. Les fichiers fournis par l'utilisateur couvrent le cas
  réel (« ma calibration pour mon boîtier ») ; la base embarquée reviendra dans
  son propre ADR si elle est un jour voulue.
* **Auto-matcher le profil par le modèle EXIF de la caméra** plutôt qu'une
  référence explicite stockée. Écarté : le matching flou par chaîne EXIF a du
  sens pour la **base communautaire de milliers de profils** de Lensfun
  (`lens_correction.profile: "auto"`), pas pour le fichier DCP **unique** qu'un
  utilisateur a produit pour son propre boîtier. Une référence explicite
  (chemin relatif stocké) est plus simple et plus prévisible qu'une devinette
  EXIF quand il n'y a, en pratique, qu'un candidat par boîtier de l'utilisateur.
* **Ne pas checksummer le fichier de profil référencé** (le traiter comme une
  valeur de configuration, non comme un intrant dont la reproductibilité
  compte). Écarté : un DCP est colorimétriquement un intrant de rendu au même
  titre que les pixels capteur — le contrat `docs/pipeline.md` §5 exige une
  ressource d'entrée identique (même checksum) pour garantir « même révision →
  mêmes pixels ». Sans checksum, remplacer ou éditer le `.dcp` sur disque
  changerait silencieusement le rendu d'une révision réputée gelée — exactement
  ce que le contrat interdit. Le checksum BLAKE3 (ADR 0006) et le mode d'échec
  §3.4 étendent la garantie existante à ce nouveau genre de fichier référencé,
  sans inventer de catégorie d'échec nouvelle.
* **Placer la logique de profil caméra dans un nouveau crate dédié** (p. ex.
  `leyline-profile`) plutôt que d'étendre `leyline-color`. Écarté : ADR 0027 a
  déjà fait de `leyline-color` une bibliothèque de transformation couleur
  générale ; l'application DCP (matrice + LUT appliquées directement) est du
  travail de domaine couleur parallèle, sans couplage aux tampons du moteur —
  contrairement au masquage (ADR 0029), logé dans `leyline-engine` précisément
  *parce qu'*il est couplé aux internes du tampon. Deux features, deux raisons
  de placement : le DCP va là où vit déjà la science des couleurs.
* **Placer le nouvel étage après la correction d'objectif** plutôt qu'avant.
  Écarté : le profil caméra calibre la **couleur** du capteur, la correction
  d'objectif remappe la **géométrie** — aucune interaction. Le placer en tout
  premier le fait opérer sur les données les moins traitées (le RGB capteur
  juste décodé), meilleur point pour une calibration colorimétrique, et
  cohérent avec le placement tôt de la suppression de tache (ADR 0032). Le
  placer après la géométrie n'apporterait aucun bénéfice colorimétrique et
  introduirait une dépendance d'ordre inutile entre deux opérateurs qui n'en
  ont aucune.
