# ADR 0032 — Suppression de tache : clonage seul, copie bilinéaire déterministe, tôt dans le pipeline

**Statut :** Accepté — 2026-07

## Contexte

`docs/v2-scope.md` §5 (« Suppression de tache / correction ») relève qu'aucun
outil de clonage ni de correction n'existe aujourd'hui pour retirer poussières
de capteur et imperfections. C'est un item **intrinsèquement local** : comme les
réglages masqués (§2), il stocke de la géométrie dessinée sur l'image affichée
et partage donc le verrou de référentiel de coordonnées déjà tranché.

Trois décisions transversales sont **consommées, non re-litigées, ici** :

* **ADR 0026** fixe le référentiel de toute géométrie de correction locale :
  normalisée `[0,1]` relative à l'image après rotation, avant recadrage — le
  même que `crop` — remontée au tampon pré-rotation par la même famille de
  remapping arrière que `rotate` (`process2.rs:446`) et la correction
  d'objectif (`process3.rs:180`). ADR 0026 note explicitement que la suppression
  de tache réutilise ce choix ; ce document le **consomme**, il ne le re-dérive
  pas.
* **ADR 0028** fige la stratégie de versionnage : une process version par
  fonctionnalité pixel, chacune dans son propre module `processN.rs` gelé, créé
  en copiant le module précédent entier. Cette suppression de tache est un
  opérateur pixel : elle prend donc une nouvelle process version, sans que cet
  ADR ait à re-choisir la convention.
* **ADR 0029** a établi, pour les réglages locaux masqués, le schéma d'extension
  d'API (`Param`/`Value` étendus plutôt que de nouvelles méthodes `EditSession`)
  et le principe « géométrie locale dans `settings_json`, aucune table dédiée ».
  Ce document **applique** ces précédents à la tache, sans les re-concevoir.

Le §5 laissait deux questions ouvertes propres à l'item (déterminisme de la
correction *heal*, sélection automatique de la source) ; ce document les tranche
en même temps qu'il fixe le mode, le placement, l'échantillonnage et le
stockage.

## Décision

La suppression de tache est un nouvel opérateur pixel, donc une **nouvelle
process version** : elle prend **le prochain numéro de process disponible au
moment de la sortie de cette fonctionnalité** (ADR 0028), dans son propre module
`processN.rs` copie intégrale du module précédent augmentée du seul étage de
clonage — exactement la convention de duplication par module réaffirmée par
ADR 0028. Cet ADR **ne fige pas** un entier de process précis : l'ordre de
sortie des items 3/4/5/6/8 relève du plan d'implémentation futur.

### La simplification centrale — clonage seul, le *heal* est coupé de la V2

**La V2 ne livre que le mode clonage** : une copie déterministe et adoucie d'un
point source vers un point cible. Le mode *heal* (clonage sans couture, fondu de
type équation de Poisson) est **retranché du périmètre V2 — coupé comme mode,
pas déféré comme drapeau**.

Raisonnement, posé dans le même esprit qu'ADR 0016 retranchant vignettage/TCA du
process 3 « non triviaux à valider sans images de référence sous la main » : un
*heal* impose de **résoudre une équation de blending de Poisson par tache**, un
engagement algorithmique matériellement plus lourd et plus risqué qu'une copie
bilinéaire déterministe, dont la qualité et la correctness ne peuvent pas être
validées de façon responsable **sans images de référence à disposition** — la
barre exacte qu'ADR 0016 a déjà posée pour ce type d'affirmation dans ce projet.
Le clonage (copie déterministe, adoucie par falloff radial, d'un point source
vers un point cible) couvre le cas d'usage — poussière de capteur, imperfection
ponctuelle — que l'analyse de manque de `docs/v2-scope.md` §5 nommait
réellement.

Conséquence de schéma : le champ `spot_removal[].mode` esquissé au §5
(`"clone"|"heal"`) **est abandonné**. Il n'y a que le clonage en V2, donc
**aucun champ `mode` n'est nécessaire** — plus simple que de conserver un champ
`mode: "clone"` à valeur légale unique.

### Place dans le pipeline — tôt, avant le bloc tonal

Un nouvel étage **« Suppression de tache »** s'insère dans l'ordre fixe de
`docs/pipeline.md` §3.1 **immédiatement après Correction d'objectif et avant
Balance des blancs** — le tout premier étage adjacent au tonal. Il opère ainsi
sur les données **les moins traitées** (proches du linéaire décodé/corrigé
objectif), pour la meilleure qualité de clone possible — ce que la ligne
« Pipeline (§3.1) » du tableau `docs/v2-scope.md` §5 fixe déjà (« tôt dans la
chaîne… pour opérer sur des données proches du linéaire »). Cet ADR **confirme
et consomme** ce placement.

Ce point est **plus en amont** que l'étage « Réglages locaux » d'ADR 0029, qui
s'insère après Vibrance/Saturation. Les deux fonctionnalités V2 atterrissent à
des points **différents** de l'ordre fixe, et il n'y a **rien à réconcilier**
entre elles : conformément au versionnage par fonctionnalité d'ADR 0028, celle
qui sort la première ajoute son étage à sa position propre, et la seconde fait de
même, indépendamment — pas de module partagé, pas de synchronisation de
calendrier, pas d'ordre de sortie imposé.

### Référentiel de coordonnées

Repris d'**ADR 0026 sans modification** : les points cible et source de chaque
tache sont stockés en coordonnées normalisées `[0,1]` relatives à l'image après
rotation, avant recadrage — le même référentiel que la géométrie de masque
d'ADR 0029 et que `crop`. L'étage tourne sur un tampon encore dans l'orientation
décodée/corrigée-objectif ; il fait remonter les deux points au tampon
pré-rotation en appliquant la transformation **inverse** de la rotation en
attente, la même technique de remapping arrière que `rotate`/`crop`
(`process2.rs:446`, `:507`) et la correction d'objectif (`process3.rs:180`).
Aucune décision nouvelle : ADR 0026 est cité, pas re-dérivé.

### Échantillonnage du clone — copie bilinéaire déterministe

Pour chaque tache, dans l'ordre du tableau, le moteur copie le disque centré sur
le point **source** vers le disque centré sur le point **cible**, en
rééchantillonnant par **interpolation bilinéaire** à l'aide de la fonction
`bilinear` **déjà présente** dans le module process (`process2.rs:482`,
`process3.rs:549`) — la convention `n + 0.5`, propre à Leyline, de `rotate`/`crop`
(et non `lens_bilinear`, réservé à la convention entière de Lensfun). Trois
paramètres modulent la copie :

* **`radius`** — le rayon du disque copié (coordonnées normalisées, même
  convention que le reste de la géométrie) ;
* **`feather`** — un **falloff radial au bord du disque** : le patch copié se
  fond en douceur au lieu de présenter un cercle à bord franc, la fraction
  copiée décroissant du centre vers le bord selon `feather` ;
* **`opacity`** — l'intensité globale du fondu du patch sur le fond.

Chaque pixel cible reçoit `lerp(fond, échantillon_source, couverture)`, où la
couverture combine le falloff radial et l'opacité. La copie est **pure et
déterministe** (ADR 0012) : mêmes points, mêmes paramètres, mêmes pixels. La
forme exacte de la courbe de falloff est une constante de la PR d'implémentation,
au même niveau de précision qu'ADR 0016/0030/0031 ; seule la **famille** — copie
bilinéaire adoucie par falloff radial — est figée ici.

### Sélection de la source — manuelle uniquement, aucune suggestion moteur

La source est placée **explicitement par l'utilisateur** : il pose lui-même le
point cible **et** le point source. **Aucune fonctionnalité moteur de
suggestion automatique de source n'existe en V2.**

Raisonnement : la question ouverte #2 du §5 signalait déjà qu'une source
proposée par le moteur devrait être **déterministe et enregistrée**, jamais
recalculée au rendu (`docs/pipeline.md` §5). Couper la suggestion automatique en
V2 évite de concevoir ce mécanisme de déterminisme/enregistrement
prématurément.

> **Guidage prospectif (pas un mécanisme conçu ici).** Si une suggestion de
> source est ajoutée un jour, la même règle s'appliquera : quoi que le moteur
> propose est **écrit dans `spot_removal[].source`** comme n'importe quelle
> autre valeur, jamais laissé implicite ni recalculé à la volée. La suggestion
> ne serait qu'une aide de saisie côté client remplissant un champ existant, pas
> un chemin de rendu séparé.

### Stockage — schéma additif, aucune table dédiée

`spot_removal` est une **liste** optionnelle de `settings_json`, chaque entrée
`{ target: {x,y}, source: {x,y}, radius, feather, opacity }`. **Absent ou liste
vide = neutre** (aucune tache), rendu **bit-pour-bit identique** à la process
version précédente — l'invariant « *a parameter at its neutral value skips its
operator entirely, so the neutral rendering is bit-for-bit the decoded image* »
documenté en tête de `process3.rs:25`. Aucun bump de schéma requis, cohérent avec
le schéma additif « process +1, schema inchangé » de la plupart des items V2
(`docs/v2-scope.md` §1) — les champs inconnus d'un moteur ancien sont préservés
verbatim (`Settings::extra`, `crates/leyline-core/src/settings.rs`).

Le choix « liste dans `settings_json`, **pas de table dédiée** » n'est pas
rouvert ici : la ligne « Catalogue » du tableau `docs/v2-scope.md` §5 le fixait
déjà (liste compacte, cohérente avec « une révision = état complet et autonome »,
`docs/catalog.md` §17). Ce document se contente de le confirmer. Comme ADR 0029
et ADR 0028 le posent : si le volume des listes devenait un jour un vrai
problème, ce sera le problème d'un futur ADR **avec des données réelles**, pas
une optimisation spéculative aujourd'hui.

### Extension de l'API — le schéma `Param`/`Value` d'ADR 0029

**Aucun mécanisme nouveau.** Le cycle de vie complet d'une tache
(ajouter/déplacer/supprimer) s'exprime par les `set`/`commit` existants plus le
même schéma d'extension d'enum qu'ADR 0029 a établi pour les masques
(`session.rs`) : une variante `Param` indexant la position d'une tache dans le
tableau `spot_removal` comme **unité de coalescence**, et une variante `Value`
remplaçant ou supprimant l'entrée complète à cet indice (`Some` = placement
complet, `None` = suppression, même schéma que `Value::Crop(Option<Crop>)`). Une
pose de tache complète — du placement du point à son relâchement — est **un point
de commit** sous la règle existante de `docs/catalog.md` §17 ; des éditions
successives du **même indice** dans la fenêtre d'amendement existante amendent la
révision de tête, exactement comme `Param::LocalAdjustment(usize)` d'ADR 0029.
Ce document ne re-dérive pas ce mécanisme : il pointe ADR 0029 comme précédent et
l'applique.

### Esquisse JSON

Le style suit `docs/pipeline.md` §3.2. Une tache puis le cas neutre :

```json
{
    "schema": 1,
    "process": 6,

    "exposure": 0.2,
    "spot_removal": [
        {
            "target": { "x": 0.62, "y": 0.31 },
            "source": { "x": 0.55, "y": 0.29 },
            "radius": 0.03,
            "feather": 0.40,
            "opacity": 1.0
        }
    ]
}
```

Cas neutre — champ absent (ou `"spot_removal": []`), rendu bit-pour-bit
identique à la process version précédente :

```json
{
    "schema": 1,
    "process": 6,
    "exposure": 0.2
}
```

> *Le `process: 6` ci-dessus est purement illustratif : le numéro réel est le
> prochain disponible au moment de la sortie (ADR 0028), pas fixé par cet ADR.*

> **Note d'implémentation (pas une édition de spec ici).** Cet ADR ne modifie
> **pas** le diagramme de `docs/pipeline.md` §3.1 ni le tableau des process
> versions §3.3. Comme pour ADR 0029/0030/0031, la spec est mise à jour dans le
> même changement que l'implémentation réelle, conformément à CLAUDE.md. Le
> présent document fixe seulement **où** l'étage atterrit et **quelle**
> mathématique il gèle ; le diagramme §3.1 et le tableau §3.3 seront amendés par
> la PR qui livre le module process.

## Conséquences

* **Le champ `process` garde sa lisibilité sémantique** (ADR 0028) : le nouveau
  numéro signifiera exactement « suppression de tache par clonage active », un
  fait unique et lisible, comme `process: 3` signifie « correction de distorsion
  active ».
* **Sortie neutre gelée** : sans tache, l'étage est bit-pour-bit la process
  version précédente. L'invariant « valeur neutre → opérateur entièrement
  sauté » (`process3.rs:25`) reste vrai pour l'étage entier.
* **Le *heal* reste ouvert pour un futur ADR** avec des images de référence à
  disposition : la coupe ne ferme pas la porte, elle refuse seulement de
  s'engager sur un blending de Poisson que le projet ne peut pas valider
  aujourd'hui. Le complexité de l'item retombe ainsi de « **M** (clone) à **L**
  (heal) » (`docs/v2-scope.md` §5) à **M** seul.
* **La suggestion automatique de source reste ouverte** pour un futur ADR ; si
  elle arrive, elle écrit dans `spot_removal[].source` comme toute autre valeur,
  jamais recalculée au rendu (`docs/pipeline.md` §5).
* **Aucune surface de session nouvelle** : taches créées/déplacées/supprimées
  entièrement par `set`/`commit` plus l'extension `Param`/`Value` d'ADR 0029. Le
  reste du moteur (jobs, événements, coalescence, amendement) n'est pas touché.
* **Deux features locales V2 à des points distincts du pipeline** : la tache tôt
  (avant Balance des blancs), les réglages masqués tard (après
  Vibrance/Saturation, ADR 0029). Aucune ne dépend de l'autre, aucune n'attend
  l'autre — le versionnage par fonctionnalité (ADR 0028) le garantit.
* **Le contrat de reproductibilité** (`docs/pipeline.md` §5) est respecté : tout
  paramètre (cible, source, rayon, feather, opacité) est explicite et
  enregistré, l'échantillonnage est pur et déterministe (ADR 0012), aucune
  source de hasard ni de devinette côté moteur nulle part dans le chemin de
  clonage V2 — tout se sérialise dans `settings_json`, « même révision → mêmes
  pixels ».
* **Un module `processN.rs` de plus** (ADR 0028) : coût borné et connu ; aucun
  module de version antérieure n'est touché, le gel « mêmes pixels dans dix ans »
  reste mécaniquement infalsifiable (`docs/pipeline.md` §3.3).
* **La spec `docs/pipeline.md` (§3.1, §3.3) n'est pas éditée par cet ADR** :
  elle le sera par la PR d'implémentation, conformément à CLAUDE.md.

## Alternatives écartées

* **Livrer le *heal* en V2 à côté du clonage.** Écarté — c'est la coupe centrale
  de cet ADR, dans l'esprit d'ADR 0016 retranchant vignettage/TCA. Un *heal*
  impose de résoudre une équation de blending de Poisson par tache : un
  engagement algorithmique matériellement plus lourd qu'une copie bilinéaire
  déterministe, dont la qualité ne peut être validée de façon responsable sans
  images de référence à disposition — la barre exacte d'ADR 0016. Le clonage
  couvre le cas d'usage réellement nommé (poussière de capteur) ; le *heal*
  s'ajoutera dans son propre ADR le jour où le projet pourra en valider le
  rendu. Coupé comme **mode**, pas déféré comme drapeau : le schéma ne traîne pas
  de champ `mode` à valeur unique.
* **Suggestion automatique de source côté moteur.** Écarté en V2 : elle
  imposerait de concevoir dès maintenant le mécanisme de déterminisme et
  d'enregistrement qu'exige la question ouverte #2 du §5 (une source proposée
  doit être déterministe et écrite dans les paramètres, jamais recalculée au
  rendu). La sélection manuelle sidesteppe ce chantier ; si la suggestion est
  ajoutée plus tard, elle remplira simplement `spot_removal[].source` comme une
  aide de saisie, sans chemin de rendu séparé ni valeur implicite.
* **Placer l'étage après les réglages locaux d'ADR 0029, au lieu de tôt dans le
  pipeline.** Écarté : le clonage donne sa meilleure qualité sur des données
  proches du linéaire décodé/corrigé objectif, avant que le bloc tonal n'étire
  les valeurs (`docs/v2-scope.md` §5). Le placer tard le ferait copier des pixels
  déjà tonalisés, changeant quels pixels alimentent aussi réduction de bruit et
  netteté. Les deux étages n'ont aucun besoin d'être adjacents : ADR 0028 les
  autorise à des positions fixes distinctes sans réconciliation.
* **De nouvelles méthodes `EditSession` dédiées (`add_spot`/`remove_spot`/…) au
  lieu d'étendre `Param`/`Value`.** Écarté pour la même raison qu'ADR 0029 l'a
  écarté pour les masques : cela dupliquerait le mécanisme de
  coalescence/amendement, chaque méthode devant re-décider amendement vs nouvelle
  révision. Indexer le tableau `spot_removal` par une variante `Param` hérite
  gratuitement de toute la politique `Pending::One`/fenêtre d'amendement existante
  (`session.rs`). La surface de session reste minimale
  (`set`/`commit`/`undo`/`redo`) et le comportement de coalescence uniforme sur
  tous les paramètres.
