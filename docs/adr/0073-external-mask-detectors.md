# ADR 0073 — Les masques détectés : une prise ouverte, des détecteurs séparés

**Statut :** Accepté — 2026-08

## Contexte

C2 de [`competitive-plan.md`](../competitive-plan.md) — les masques
automatiques — est le seul item de l'axe IA compatible avec
[`pipeline.md`](../pipeline.md) §5.1 sans compromis, et
[ADR 0069](0069-closed-extension-boundary.md) a tranché *par où* une
fonctionnalité fermée s'attache : elle produit des réglages, jamais des pixels.

**La moitié ouverte est déjà livrée**, et il faut le dire avant de décider quoi
que ce soit :

* [ADR 0070](0070-stored-mask-coverage.md) a donné à `Mask` une variante
  `Coverage` — un masque peut être une image stockée — et à la bibliothèque
  `store_mask_coverage`, dont le commentaire annonce déjà « the surface a
  closed extension uses through the SDK » ;
* la même ADR, §7, a livré l'import d'un masque **depuis un fichier image** ;
* [ADR 0071](0071-mask-overlay.md) a livré la surimpression, sans laquelle un
  masque calculé serait invisible.

Il ne manque donc rien dans le moteur. Ce qui manque est **la prise** : par
quel geste un utilisateur de Studio obtient un masque du ciel sans passer par
un export, un outil tiers et un import manuel.

### Le geste retenu, et celui qui ne l'est pas

Deux formes existent dans les logiciels du marché :

* **la détection automatique** — un bouton, « le ciel », « le sujet » ;
* **la sélection au clic** — on désigne un point, un modèle type SAM segmente
  ce qu'on a désigné.

La seconde est plus puissante et couvre les deux cas de la première par
inversion. Elle demande en revanche une interface interactive à construire —
points positifs et négatifs, retour immédiat, réencodage à chaque clic — et un
encodeur qui tourne avant le premier clic. **La détection automatique est
retenue** (décision du 2026-08-04) : un bouton, un résultat, aucune interface
nouvelle à inventer. La sélection au clic reste ouverte, et la prise décidée
ci-dessous ne lui ferme pas la porte — elle n'en est que l'appel le plus
simple.

## Décision

### 1. Rien dans le moteur, et un crate à part pour la prise

Aucune ligne n'est ajoutée à `leyline-engine` : tout ce dont un détecteur a
besoin y est déjà (§Contexte). La prise vit dans un **nouveau crate ouvert**,
`leyline-detect`, dont c'est le seul objet :

```
Studio ─→ SDK ─→ leyline-detect ──(processus)──→ un détecteur, quel qu'il soit
```

Ce crate ne détecte rien. Il tient **le contrat, la découverte et
l'invocation** — quelques centaines de lignes, aucune dépendance lourde, et
`leyline-core` pour seul crate Leyline en dessous de lui.

Il est **ré-exporté par `leyline-sdk`**, comme `leyline-map` ou
`leyline-export` le sont : [`architecture.md`](../architecture.md) §À
l'intérieur de Studio impose que Studio ne déclare *qu'une seule* dépendance
Leyline, et cette règle ne se plie pas pour un accessoire. La CLI l'obtient
par le même chemin, sans copier une ligne.

### 2. Un détecteur est un exécutable qui transforme une image en masque

C'est la décision structurante, et elle est délibérément **plus petite** que ce
qu'ADR 0069 §2 envisageait.

```
détecteur --image <entrée.png> --detector <id> --out <sortie.png>
```

* **entrée** : l'aperçu développé, rendu par Studio, en PNG RGB 8 bits ;
* **sortie** : un PNG **gris 16 bits**, `0` = le réglage ne s'applique pas,
  `65535` = il s'applique — exactement le format qu'ADR 0070 §4 a figé ;
* **le reste** : code de sortie `0`, et `stderr` pour dire pourquoi quand ce
  n'est pas `0`.

Quatre conséquences, et ce sont elles qui justifient la forme :

* **Le détecteur ne touche ni au catalogue, ni à la bibliothèque.** Il ne
  l'ouvre pas, ne prend aucun verrou SQLite, ne connaît aucun identifiant.
  C'est *le client ouvert* qui appelle ensuite `store_mask_coverage` et écrit
  la révision par une session d'édition ordinaire. La règle d'ADR 0069 §1 —
  une extension produit des réglages, jamais des pixels — est ici tenue
  **plus strictement** que l'ADR ne l'exigeait : le détecteur ne produit même
  pas un réglage, il produit une image que le code ouvert transforme en
  réglage.
* **Le détecteur n'a pas besoin de lier `leyline-sdk`.** La permission
  additionnelle GPLv3 §7 d'ADR 0069 §4 reste utile pour d'autres formes
  d'extension, mais **celle-ci ne la met pas en jeu** : deux processus qui
  s'échangent deux fichiers PNG ne forment pas une œuvre combinée. La
  frontière de licence est franchie par un `execve`, ce qui est le point le
  plus dur qu'on puisse atteindre.
* **N'importe qui peut en écrire un**, en vingt lignes de Python, avec le
  modèle qu'il veut. La prise a donc une valeur propre pour le projet libre,
  indépendamment de tout produit payant — c'est ce qui la rend légitime dans
  le dépôt ouvert plutôt que taillée pour un vendeur.
* **Un détecteur qui plante ne tue pas Studio.** Un greffon chargé
  dynamiquement, si.

### 3. La découverte : un manifeste par détecteur, dans la configuration de l'utilisateur

Un détecteur s'installe en déposant un manifeste JSON dans
`<config utilisateur>/Leyline/detectors/<id>.json` — le même dossier que
`recent_libraries.json` occupe déjà (`directories::ProjectDirs`), et pour la
même raison : **une bibliothèque est portable et autonome**
(`catalog.md` §37), elle ne doit pas gagner un fichier annexe qui parle de
logiciels installés sur *cette* machine.

```json
{
  "id": "leyline-assist",
  "label": "Leyline Assist",
  "command": "/opt/leyline-assist/leyline-assist",
  "args": ["detect"],
  "detectors": [
    { "id": "sky",     "label": "Ciel" },
    { "id": "subject", "label": "Sujet" }
  ]
}
```

Un manifeste illisible, incomplet, ou dont la commande n'existe pas est
**ignoré** — pas une erreur au démarrage : un détecteur est un accessoire, et
Studio doit se lancer sans. Aucun manifeste, aucun menu : la fonctionnalité
n'apparaît pas plutôt que d'apparaître grisée.

### 4. Ce que le geste produit dans la révision

Un **nouveau réglage local**, avec le masque détecté et des valeurs
**neutres** : la détection choisit *où*, l'utilisateur choisit *quoi*. Créer un
masque avec une exposition déjà posée devinerait l'intention.

Le masque est stocké à la résolution que le détecteur a rendue — ADR 0070 §5
a déjà tranché que la couverture n'a pas à suivre celle du capteur, et le
modèle qui travaille en 512×512 n'a rien à gagner à voir sa sortie
suréchantillonnée avant d'être écrite.

### 5. Les modèles : la licence des **poids** est éliminatoire, et elle élimine

La condition n°5 du plan (§4) demande une licence compatible GPL-3.0. Vérifié
le 2026-08-04, et le résultat justifie à lui seul d'avoir regardé avant de
coder :

| Modèle | Licence | Verdict |
|---|---|---|
| SegFormer ADE20K (NVIDIA) | *NVIDIA Source Code License-NC* | **Écarté** — non commercial |
| RMBG-1.4 (BRIA) | licence propre, commercial payant | **Écarté** |
| U²-Net | Apache-2.0 | Retenu (sujet) |
| BiRefNet | MIT | Retenu (sujet) |
| Zoo MMSegmentation / PaddleSeg (ADE20K, classe *sky*) | Apache-2.0 | Retenu (ciel) |

Les deux premiers sont les plus visibles et les plus faciles à trouver : c'est
exactement le piège que la condition existe pour attraper. Apache-2.0 et MIT
entrent sans difficulté dans un travail GPL-3.0-only, dans ce sens-là.

Le choix définitif d'un modèle par détecteur, sa conversion en ONNX et sa
mesure ne sont **pas** tranchés ici : ils appartiennent au détecteur, donc à
son propre dépôt. Ce qui est tranché ici, c'est le critère et le fait qu'il
soit vérifié avant toute ligne.

### 6. Le premier détecteur, et où il vit

`leyline-assist` : un exécutable, dépôt privé, deux détections (ciel, sujet),
inférence ONNX en **pur Rust** — le détecteur doit se compiler pour Windows
sans rejouer la douleur d'[ADR 0038](0038-tethered-capture.md), où
`libgphoto2` a fini en fonctionnalité désactivée faute de paquet mingw. C'est
une décision privée, consignée ici parce qu'elle explique pourquoi la prise
n'impose aucun runtime : elle n'en connaît aucun.

Les **poids ne sont jamais dans le dépôt ouvert**, ni dans l'AppImage libre —
condition n°6 du plan. Ils accompagnent le détecteur.

### 7. Ce qui n'est pas tranché ici

* **Le système de clé payante** — renvoi à ADR 0069 §5, qui l'a déjà mis hors
  périmètre et a nommé ses deux contraintes (vérification hors ligne,
  `specification.md` §4 à corriger plutôt qu'à contourner).
* **La sélection au clic** (SAM). La prise ci-dessus la rendrait possible sans
  la servir : elle passe des fichiers, pas des clics. Ce sera une autre ADR,
  et probablement une autre forme de prise.
* **C1, le débruitage IA.** Toujours sans issue, et pour une raison qui se
  formule mieux depuis cet ADR : un débruiteur produit des **pixels**. Il ne
  peut donc ni être matérialisé une fois comme un masque, ni traverser la
  frontière d'ADR 0069, ni entrer dans le chemin de rendu sans emporter §5.1.
  [ADR 0072](0072-measured-noise-profile.md) — le profil de bruit mesuré —
  était la réponse partielle prévue à sa place, et elle est livrée.

## Conséquences

* **Un crate de plus, ouvert et petit** : `leyline-detect`,
  [`architecture.md`](../architecture.md) le liste. Le moteur, lui, ne bouge
  pas d'une ligne — donc aucune version d'étage, donc §5.1 hors de cause,
  donc aucun rendu de référence à bénir.
* **Une photo masquée reste une photo ordinaire.** Le masque détecté est une
  couverture stockée comme une autre : une compilation sans aucun détecteur
  l'ouvre, la rend et l'exporte à l'identique. C'est le §1 d'ADR 0069, vérifié
  ici par construction plutôt que promis.
* **Le format du catalogue ne bouge pas** : ni schéma, ni `settings_json`, ni
  version d'étage. `local_adjustments::v3` rend déjà les couvertures.
* **Studio gagne une entrée par détection découverte**, et rien du tout quand
  aucun manifeste n'existe — ce qui est le cas de toute installation
  d'aujourd'hui.

## Alternatives écartées

* **Un greffon chargé dynamiquement** dans Studio. Interdit par ADR 0069 §2
  côté moteur, et sans intérêt côté interface : la liaison ferait du binaire
  libre et du binaire fermé une œuvre combinée, là où deux processus n'ont
  même pas la question à poser.
* **Le détecteur ouvre la bibliothèque lui-même** (ce qu'ADR 0069 §2
  décrivait : « elle demande un aperçu, calcule, et écrit par une session
  d'édition »). Deux processus sur le même `catalog.db` se disputeraient un
  verrou SQLite que le projet a déjà passé deux ADR à resserrer
  ([ADR 0023](0023-catalog-lock-narrowing-preview.md),
  [ADR 0024](0024-catalog-lock-narrowing-export.md)), et le détecteur devrait
  connaître le modèle de données pour écrire une révision. Passer deux PNG
  supprime les deux problèmes d'un coup.
* **Deux compilations de Studio**, libre et payante. C'est le clone qu'ADR
  0069 refuse, déplacé d'un cran : même report de correctif perpétuel, et une
  interface qui diverge.
* **Embarquer un modèle dans le dépôt ouvert.** Plusieurs centaines de
  mégaoctets pour une fonctionnalité optionnelle (condition n°6), et une
  fonctionnalité de base qui dépendrait des poids (condition n°2).
* **Attendre la sélection au clic** pour ne livrer qu'une fois. Le bouton
  couvre le cas le plus fréquent — le ciel d'un paysage — et la prise qu'il
  demande est celle qu'un futur outil interactif réutilisera pour son propre
  compte.
