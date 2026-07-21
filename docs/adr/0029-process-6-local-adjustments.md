# ADR 0029 — Process 6 : réglages locaux masqués (brosse, radial, gradient)

**Statut :** Accepté — 2026-07

## Contexte

`docs/v2-scope.md` §2 (« Réglages locaux / masqués ») est l'item **fondateur**
de la V2 : il introduit la notion générique de *masque* — une couverture
spatiale `[0,1]` par pixel — au-dessus de laquelle un sous-ensemble de
`Settings` s'applique localement plutôt que globalement. Le §9 le confirme
comme l'infrastructure sur laquelle s'adossent la suppression de tache (§5),
le dehaze masqué (§6) et le color grading régional (§3). Aujourd'hui, tout
`Settings` s'applique globalement (`crates/leyline-core/src/settings.rs`) ;
aucun réglage spatialement restreint n'existe.

Trois ADR ont déjà tranché les verrous transversaux qui pesaient sur cet
item, chacun explicitement « en amont » pour ne pas être re-dérivé ici :

* **ADR 0026** fixe le référentiel de coordonnées de toute géométrie de
  masque : normalisée `[0,1]` relative à l'image après rotation, avant
  recadrage — le même que `crop` — et impose au moteur de la faire remonter
  au tampon pré-rotation par la même famille de remapping arrière que
  `rotate`/la correction d'objectif. Ce document **ne rouvre pas** ce choix ;
  il le consomme.
* **ADR 0028** fige la stratégie de versionnage : une process version par
  fonctionnalité pixel, chacune dans son propre module `processN.rs` gelé,
  créé en copiant le module précédent entier. Ce document **applique** cette
  convention sans la re-litiger.
* **ADR 0027** élargit la gestion des couleurs en sortie sans toucher
  l'espace de travail interne du pipeline — sans rapport direct ici, mais il
  confirme que le rendu interne reste sRGB, l'espace où les opérateurs
  tonals/couleur (que les masques réutilisent) sont définis.

Ce que ces trois ADR ont **laissé ouvert** pour l'item 2 lui-même
(`docs/v2-scope.md` §2, questions 2 et 3) : le **stockage** des masques
(traits vectoriels dans `settings_json` vs table dédiée), et la
**coalescence** (un trait de brosse est-il une intention ou un geste continu
à coalescer comme un drag de curseur ?). ADR 0026 note explicitement que ces
deux points « restent à trancher par l'ADR propre à chaque fonctionnalité ».
Ce document les tranche, en même temps qu'il fixe la place de l'étage dans le
pipeline, ce qu'un masque peut régler, et le placement crate.

Le moteur numérote aujourd'hui `CURRENT_PROCESS = 5`
(`crates/leyline-core/src/settings.rs`) ; cet ajout pixel prend donc le
**process 6**.

## Décision

Les réglages locaux masqués sont **process 6**, dans un nouveau module
`crates/leyline-engine/src/process6.rs`, copie intégrale de `process5.rs`
augmentée du seul étage nouveau — exactement la convention de duplication par
module réaffirmée par ADR 0028. Trois types de masque sont dans le périmètre :
**brosse**, **radial** et **gradient (linéaire)**.

### Place dans le pipeline

Un nouvel étage **« Réglages locaux »** s'insère dans l'ordre fixe de
`docs/pipeline.md` §3.1 **immédiatement après Vibrance/Saturation et avant
Réduction du bruit**. Les réglages locaux réutilisent exactement la même
mathématique d'opérateur tonal/couleur que les réglages globaux (exposition,
contraste, hautes lumières/ombres, blancs/noirs, balance des blancs,
vibrance/saturation), simplement re-paramétrée par masque et fondue par
couverture. Les faire tourner en **une passe masquée supplémentaire, juste
après la passe globale équivalente**, évite de propager la conscience du
masque dans le site d'appel de chaque opérateur global : c'est la plus petite
insertion correcte, pas une refonte du pipeline.

> **Note d'implémentation (pas une édition de spec ici).** Cet ADR ne modifie
> **pas** le diagramme de `docs/pipeline.md` §3.1. Ce diagramme décrit le
> pipeline **tel qu'implémenté** ; contrairement à la correction d'objectif
> (qui était déjà un champ nommé mais inerte depuis le schéma 1), cet étage
> **n'existe pas encore** dans le code. Conformément à CLAUDE.md, la spec est
> mise à jour dans le même changement que l'implémentation réelle, pas dans
> cet ADR de pré-décision. Le présent document fixe seulement **où** l'étage
> atterrira ; le diagramme §3.1 et le tableau des process versions §3.3
> seront amendés par la PR qui livre `process6.rs`.

### Référentiel de coordonnées

Repris d'**ADR 0026 sans modification** : la géométrie de chaque masque est
stockée en coordonnées normalisées `[0,1]` relatives à l'image après
rotation, avant recadrage. L'étage process 6 tourne sur un tampon encore dans
l'orientation décodée/corrigée-objectif ; il fait remonter la géométrie au
tampon pré-rotation en appliquant la transformation **inverse** de la rotation
en attente, la même technique de remapping arrière que `rotate`/`crop`
(`process2.rs:446`, `:507`) et la correction d'objectif (`process3.rs`).
Aucune décision nouvelle ici : ADR 0026 est cité, pas re-dérivé.

### Ce qu'un masque peut régler

Un sous-ensemble restreint de `Settings`, réutilisant **exactement les mêmes
champs et formules d'opérateur** que leurs équivalents globaux : balance des
blancs (température/teinte), exposition, contraste, hautes lumières, ombres,
blancs, noirs, vibrance, saturation.

Sont **explicitement hors périmètre** d'un réglage masqué en V2 : correction
d'objectif, réduction du bruit, netteté, rotation/recadrage. Ils restent
**globaux uniquement** (voir *Conséquences* et *Alternatives écartées* pour le
raisonnement — même esprit qu'ADR 0016 retranchant vignettage/TCA du process
3). Ce sont soit des réglages qui n'ont pas de sens spatialement restreint
(rotation/recadrage **définissent** le cadre lui-même), soit des réglages qui
ouvrent des questions bien plus larges (noyaux de netteté/débruitage variant
spatialement, correction d'objectif interagissant avec un remapping arrière
par pixel qui a déjà lieu à un autre étage) — aucune n'a besoin d'être résolue
pour livrer l'infrastructure de masquage de base.

### Placement crate

**Aucun nouveau crate.**

* Les **types de géométrie et de valeurs** des masques vivent dans
  `leyline-core::Settings` — le même crate que `Crop`, `NoiseReduction`,
  `Sharpening`.
* La **rastérisation** (masque → couverture `[0,1]` par pixel) et le
  **compositing** vivent dans `leyline-engine`, dans un nouveau module (p. ex.
  `mask.rs`) consommé par `process6.rs` — le même schéma que `rotate`/`crop`
  qui vivent directement dans les modules process.

Contraste explicite avec `leyline-lens` : ce crate est séparé parce qu'il
enveloppe une dépendance externe (Lensfun) et une base de profils externe. Le
masquage n'enveloppe **rien** d'externe : c'est de la géométrie pure,
étroitement couplée au tampon de rendu et à son échantillonnage. Une frontière
de crate séparerait deux choses qui ont besoin de partager les internes de
tampon/échantillonnage, pour aucun bénéfice à un consommateur hors moteur —
Studio, la CLI et le SDK n'appellent jamais la rastérisation de masque
directement, seulement `EditSession`.

### Stockage — résout `docs/v2-scope.md` §2 question 2

Tout vit dans `settings_json`, **aucune table dédiée**.

* Les masques **paramétriques** (radial, gradient) sont quelques flottants
  chacun — trivialement compacts.
* Les masques **brosse** stockent la **liste de traits** (points ordonnés,
  chacun avec x/y/rayon/flux/dureté), **jamais un bitmap rastérisé** —
  reproductible et portable, cohérent avec `docs/catalog.md` §17 (« une
  révision = un état complet et autonome »).

Comme ADR 0028 le raisonne à propos de la prolifération des modules process :
si les listes de traits devenaient un jour un vrai problème de volume, ce sera
le problème d'un futur ADR **avec des données réelles**, pas quelque chose à
résoudre spéculativement aujourd'hui.

### Extension de l'API — résout `docs/v2-scope.md` §2 question 3

**Aucun mécanisme nouveau.** Le cycle de vie complet d'un masque
(créer/éditer/supprimer) s'exprime par les `set`/`commit` existants plus deux
variantes ajoutées aux enums déjà en place (`session.rs`) :

* `Param` gagne **`LocalAdjustment(usize)`**, où l'indice adresse la position
  d'un masque dans le tableau `local_adjustments` courant. Cette variante
  regroupe la géométrie du masque **et** ses curseurs de réglage comme une
  seule unité de coalescence — exactement le schéma déjà utilisé par
  `Param::WhiteBalance`, documenté « *White balance override (temperature +
  tint together: one tool)* ». Un masque est « un outil » au même sens.
* `Value` gagne **`LocalAdjustment(Option<LocalAdjustment>)`** : `Some`
  remplace la définition complète du masque (remplacement de struct entière,
  même schéma que `Value::Crop(Option<Crop>)`/`NoiseReduction`/`Sharpening` —
  aucun patch de champ partiel n'existe nulle part dans cette API) ; `None`
  **supprime** ce masque (miroir de `Crop` dont `None` = plein cadre, de
  `WhiteBalance` dont `None` = retour au « tel que pris »).

La coalescence réutilise **la règle existante inchangée** :

* Un **trait de brosse complet** — appui souris à relâchement — est exactement
  **un point de commit** sous la règle existante de `docs/catalog.md` §17
  (« *l'utilisateur relâche un contrôle (fin de drag)* »). Aucune règle
  nouvelle n'est inventée.
* Des éditions successives du **même indice de masque** dans la fenêtre
  d'amendement de 2 secondes existante amendent la révision de tête —
  exactement la règle d'amendement par `Param` déjà réalisée par le mécanisme
  `Pending::One(Param)` de `session.rs`, appliquée telle quelle à la nouvelle
  variante.

**Conséquence explicite : aucun mécanisme de coalescence nouveau, aucune
méthode `EditSession` nouvelle** (pas d'`add_mask`/`remove_mask`). La surface
de session reste la même minimale (`set`/`commit`/`undo`/`redo`) qu'aujourd'hui.

### Composition / rendu

Les masques s'appliquent **séquentiellement dans l'ordre du tableau** —
l'ordre du tableau est le **seul** ordre d'empilement (pas de champ z-index ni
d'identifiant séparé : la position dans le tableau est le seul mécanisme
d'ordre, à l'image du pipeline fixe lui-même qui n'a pas de concept de
réordonnancement au-delà de sa structure déclarée).

Pour chaque masque, dans l'ordre :

1. rastériser sa couverture (`[0,1]` par pixel, remontée au tampon
   pré-rotation par ADR 0026) ;
2. multiplier par l'opacité du masque ;
3. fondre :
   `output = lerp(buffer, apply_local_operators(buffer, mask.adjustments), coverage)`.

Le masque suivant lit la sortie de ce masque. `apply_local_operators` réutilise
les **mêmes formules d'opérateur par pixel** que le global (re-paramétrées par
masque), **pas** de nouvel algorithme : c'est pourquoi le process 6
n'introduit **aucune mathématique tonale nouvelle**, seulement une application
masquée de mathématique existante.

Un tableau `local_adjustments` **absent ou vide** saute l'étage entier, sortie
**bit-pour-bit identique** à celle du process 5 — ce qui préserve l'invariant
« *a parameter at its neutral value skips its operator entirely, so the
neutral rendering is bit-for-bit the decoded image* » documenté en tête de
`process3.rs`.

### Schéma

**Additif** : un tableau optionnel `local_adjustments`, absent/vide = neutre.
**Aucun bump de schéma requis**, cohérent avec le schéma additif « process +1,
schema inchangé » de la plupart des items V2 (`docs/v2-scope.md` §1) — les
champs inconnus d'un moteur ancien sont déjà préservés verbatim
(`Settings::extra`, `leyline-core/src/settings.rs`).

### Presets — coupe de périmètre explicite

`SettingsGroup` (`docs/engine-api.md` §10.3,
`crates/leyline-core/src/settings.rs`) **ne gagne pas** de variante
`LocalAdjustments` en V2. La géométrie de masque est **spécifique à la
composition** : un filtre radial positionné pour le sujet d'une photo n'a aucun
sens appliqué verbatim à une autre photo — contrairement aux décalages
purement numériques de `Tone`/`Presence` qui transfèrent réellement d'une
photo à l'autre. C'est une coupe délibérée, pas un oubli (même esprit que
`SettingsGroup::Geometry`, déjà exclu par défaut des presets parce que « *geometry
is a per-photo judgment, not a reproducible style* »).

### Esquisse JSON

Une instance de chaque type dans `local_adjustments`, plus le cas neutre. Le
style suit `docs/pipeline.md` §3.2 :

```json
{
    "schema": 1,
    "process": 6,

    "exposure": 0.35,
    "vibrance": 18,

    "local_adjustments": [
        {
            "mask": {
                "type": "radial",
                "cx": 0.5, "cy": 0.42,
                "rx": 0.30, "ry": 0.22,
                "angle": 0.0,
                "feather": 0.40,
                "inverted": false
            },
            "opacity": 1.0,
            "adjustments": { "exposure": 0.6, "contrast": 15, "highlights": -20 }
        },
        {
            "mask": {
                "type": "gradient",
                "x0": 0.5, "y0": 0.0,
                "x1": 0.5, "y1": 0.35
            },
            "opacity": 0.8,
            "adjustments": { "exposure": -0.8, "whites": -10, "temperature": 5200, "tint": 6 }
        },
        {
            "mask": {
                "type": "brush",
                "strokes": [
                    { "x": 0.20, "y": 0.60, "radius": 0.04, "flow": 1.0, "hardness": 0.5 },
                    { "x": 0.23, "y": 0.61, "radius": 0.04, "flow": 1.0, "hardness": 0.5 },
                    { "x": 0.26, "y": 0.62, "radius": 0.04, "flow": 1.0, "hardness": 0.5 }
                ]
            },
            "opacity": 1.0,
            "adjustments": { "saturation": -30, "shadows": 20 }
        }
    ]
}
```

Cas neutre — tableau absent (ou `"local_adjustments": []`), rendu bit-pour-bit
identique au process 5 :

```json
{
    "schema": 1,
    "process": 6,
    "exposure": 0.35
}
```

Chaque `mask` porte les champs de balance des blancs dans `adjustments` sous la
même forme que `WhiteBalance` global (`temperature`/`tint`), et les curseurs
`[-100, +100]` sous la même forme que leurs équivalents globaux : réutilisation
de vocabulaire, aucune unité nouvelle.

## Conséquences

* **Le champ `process` garde sa lisibilité sémantique** (ADR 0028) :
  `process: 6` signifiera exactement « réglages locaux masqués actifs », un
  fait unique et lisible, comme `process: 3` signifie « correction de
  distorsion active ».
* **Sortie neutre gelée** : sans masque, le process 6 est bit-pour-bit le
  process 5. L'invariant « valeur neutre → opérateur entièrement sauté »
  (`process3.rs`) reste vrai, mécaniquement, pour l'étage entier.
* **Aucune surface de session nouvelle** : masques créés/édités/supprimés
  entièrement par `set`/`commit` plus les deux variantes `Param`/`Value`. Le
  reste du moteur (jobs, événements, coalescence, amendement) n'est pas touché.
* **La coupe des réglages masquables** (objectif, débruitage, netteté,
  rotation/recadrage restent globaux) laisse ces quatre chantiers ouverts pour
  un futur ADR, sans bloquer l'infrastructure de base. Un réglage masqué de
  débruitage/netteté demanderait des noyaux variant spatialement ; un objectif
  masqué interagirait avec le remapping arrière déjà en cours à l'étage
  correction d'objectif — questions réelles, mais non nécessaires pour livrer
  le cœur.
* **La coupe des presets** signifie que les styles restent transférables
  (offsets numériques) sans traîner de géométrie non transférable ; le color
  grading régional (`docs/v2-scope.md` §3, §4) pourra rouvrir la question de
  presets régionaux le moment venu, avec son propre ADR.
* **Un module `processN.rs` de plus** (ADR 0028) : coût borné et connu, la
  trajectoire reste linéaire. Aucun module de version antérieure n'est touché ;
  le gel « mêmes pixels dans dix ans » reste mécaniquement infalsifiable
  (`docs/pipeline.md` §3.3).
* **Le contrat de reproductibilité** (`docs/pipeline.md` §5) est respecté : les
  masques brosse stockent des traits vectoriels déterministes (jamais un raster
  dépendant du rendu), la rastérisation et le compositing sont purs et
  déterministes comme tout opérateur (ADR 0012), et tout se sérialise dans
  `settings_json` — « même révision → mêmes pixels ».
* **La spec `docs/pipeline.md` (§3.1, §3.3) n'est pas éditée par cet ADR** :
  elle le sera par la PR d'implémentation, conformément à CLAUDE.md.

## Alternatives écartées

* **Propager le masquage dans chaque opérateur global au lieu d'un étage
  post-passe.** On aurait pu rendre chaque opérateur global (exposition,
  contraste…) conscient du masque à son propre site d'appel, plutôt que
  d'ajouter une passe masquée après Vibrance/Saturation. Écarté : cela
  disperserait la logique de masque dans une dizaine de sites d'appel, chacun
  devant échantillonner la couverture et fondre, alors que la mathématique
  d'opérateur est déjà écrite et gelée ; une seule passe supplémentaire, qui
  réutilise ces mêmes formules re-paramétrées, est la plus petite insertion
  correcte. Cela multiplierait aussi les points où une régression pourrait
  altérer le rendu global — exactement ce que la duplication par module
  (ADR 0028) existe pour éviter.
* **Une table de traits de brosse dédiée au lieu de `settings_json`.** Un blob
  ou une table par révision pour les traits résoudrait un hypothétique problème
  de volume. Écarté : cela casse « une révision = un état complet et autonome »
  (`docs/catalog.md` §17), le socle de la portabilité et de la reproductibilité,
  au profit d'une optimisation dont aucune donnée réelle ne montre le besoin.
  Comme ADR 0028 le pose pour la prolifération des modules : si le volume
  devient un vrai problème un jour, ce sera le problème d'un futur ADR avec des
  données réelles.
* **De nouvelles méthodes `EditSession` dédiées (`add_mask`/`remove_mask`/…)
  au lieu d'étendre `Param`/`Value`.** Écarté : cela dupliquerait le mécanisme
  de coalescence/amendement — chaque nouvelle méthode devrait re-décider
  amendement vs nouvelle révision. Une variante `Param::LocalAdjustment(usize)`
  hérite gratuitement de toute la politique `Pending::One`/fenêtre d'amendement
  existante (`session.rs`), exactement comme `Param::WhiteBalance` regroupe
  déjà deux valeurs (température + teinte) en un outil. La surface de session
  reste minimale et le comportement de coalescence uniforme sur tous les
  paramètres.
* **Inclure objectif / débruitage / netteté / rotation-recadrage dans le jeu
  masquable de V2.** Écarté comme coupe délibérée (esprit d'ADR 0016 coupant
  vignettage/TCA du process 3). Rotation et recadrage définissent le cadre
  lui-même : « masqué » n'y a pas de sens. Débruitage et netteté masqués
  exigeraient des noyaux variant spatialement — un problème algorithmique
  substantiel. La correction d'objectif interagirait avec le remapping arrière
  par pixel déjà appliqué à un autre étage. Aucun de ces quatre n'est requis
  pour l'infrastructure de masquage de base ; les inclure gonflerait le
  process 6 avec des questions non résolues et retarderait le socle dont les
  items 3/5/6 dépendent.
* **Un nouveau crate `leyline-mask` parallèle à `leyline-lens`.** Écarté :
  `leyline-lens` est un crate séparé parce qu'il enveloppe une dépendance
  externe (Lensfun) et une base de profils externe. Le masquage n'enveloppe
  rien d'externe — c'est de la géométrie pure, étroitement couplée aux internes
  de tampon et d'échantillonnage du moteur, que seul `process6.rs` consomme.
  Une frontière de crate séparerait deux choses qui doivent partager ces
  internes, sans bénéfice pour aucun consommateur hors moteur (Studio/CLI/SDK
  n'appellent que `EditSession`). Un module `leyline-engine::mask` est le bon
  grain, comme `rotate`/`crop` vivent directement dans les modules process.
* **Traiter « filtre gradué » et « dégradé linéaire » comme deux types
  distincts.** `docs/v2-scope.md` §2 les liste séparément. Écarté comme un
  quatrième type : Lightroom emploie les deux noms pour un seul mécanisme — un
  dégradé linéaire adouci en travers du cadre. Le périmètre est donc de trois
  types (brosse, radial, gradient), pas quatre ; c'est une clarification de
  périmètre, pas un type inventé pour coller à la lettre du wording.
