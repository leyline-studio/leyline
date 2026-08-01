# Séquencement d'implémentation V2 — recommandation

**Document :** `docs/v2-implementation-plan.md`
**Version :** 0.1
**Statut :** Recommandation (entrée de planification, pas une décision) — **séquencement exécuté**

---

## État de ce document

> Ce document recommandait un ordre de construction pour les sept items de [`v2-scope.md`](v2-scope.md). **Cet ordre a été suivi et le travail est fait**, item 7 compris depuis [ADR 0051](adr/0051-watermark-rasterization-and-soft-proof-surface.md). Il se lit désormais comme une archive de planification, utile pour comprendre les arbitrages retenus, et non comme une liste de tâches. L'état réel est dans [`specification.md`](specification.md) et [`roadmap.md`](roadmap.md).
>
> Une réserve de lecture : ce document raisonne sur la garantie posée par [ADR 0028](adr/0028-process-version-per-feature.md) (une process version par fonctionnalité, duplication par module). [ADR 0042](adr/0042-versioned-stage-pipeline.md) a depuis **remplacé** ADR 0028. La garantie de fond est inchangée — un rendu figé le reste — mais son implémentation ne l'est plus.

---

# 1. Objet

Toute l'architecture V2 est tranchée : `docs/v2-scope.md` a cadré sept manques
(§2 à §8) et les ADR **0026 à 0037** (douze au total) ont arrêté la conception
de chaque item. **Il n'y a plus rien à concevoir.**

Ce document n'est **pas** un ADR (aucune décision à consigner ici) ni un
calendrier. C'est une **analyse de dépendances, d'effort et de risque** destinée
à alimenter le plan d'implémentation que **l'utilisateur** établira. Chaque
énoncé d'ordre ci-dessous est une **recommandation**, jamais un « décidé » : le
plan réel — quoi construire d'abord, dans quel ordre — appartient à
l'utilisateur, et ce document en est un **intrant**, pas un substitut.

Vocabulaire : les items gardent leur numéro de `docs/v2-scope.md` (2 =
réglages locaux, 3 = courbe, 4 = color grading/TSL, 5 = suppression de tache,
6 = dehaze/texture/clarté, 7 = épreuvage/filigrane/impression, 8 = profils DCP).

---

# 2. Résumé des dépendances

La garantie centrale vient d'[ADR 0028](adr/0028-process-version-per-feature.md) :
**un process par fonctionnalité pixel**, chacune dans son propre module
`processN.rs` gelé, aucun partage de code entre modules gelés. Conséquence
directe : aucune fonctionnalité pixel V2 n'a de dépendance **technique dure**
d'ordre de livraison sur une autre. Chacune insère son étage à une position
fixe distincte de l'ordre du pipeline, sans réconciliation.

En clair :

* **Items 3 (courbe, [ADR 0030](adr/0030-tone-curve.md)), 5 (tache,
  [ADR 0032](adr/0032-spot-removal-clone.md)), 8 (DCP,
  [ADR 0035](adr/0035-camera-profile-dcp.md)/[0037](adr/0037-dcp-parsing-dependency.md))
  et les formes _globales_ de 4 (TSL/grading,
  [ADR 0031](adr/0031-hsl-color-grading.md)) et 6 (dehaze/texture/clarté,
  [ADR 0033](adr/0033-clarity-texture-dehaze.md)) n'ont aucune dépendance dure
  entre eux** — ils peuvent sortir dans n'importe quel ordre.
* **La seule dépendance réelle** concerne les **variantes régionales / masquées**
  de 4 et 6 (et toute extension masquée future de 5) : elles sont **déférées à
  un futur ADR adossé à l'infrastructure de masquage d'[ADR 0029](adr/0029-process-6-local-adjustments.md)**
  (item 2). Leur version **globale** ne dépend, elle, **pas du tout** du masquage.

Vérifié directement dans le texte des ADR :

> [ADR 0031](adr/0031-hsl-color-grading.md) §« Color grading régional — hors
> périmètre V2 » : « Appliquer le TSL ou le color grading **sous masque** (les
> combiner avec l'infrastructure spatiale d'ADR 0029) est **explicitement hors
> périmètre de la V2** […] déférée à un futur ADR — pas conçue ici. » La version
> globale (par zone tonale) est « autonome et livrable sans le masquage ».

> [ADR 0033](adr/0033-clarity-texture-dehaze.md) §« Global uniquement en V2 — le
> masqué est déféré » : « Les trois curseurs sont livrés **en global**. Le
> **dehaze/clarté/texture masqué ou régional** […] est **explicitement hors
> périmètre de la V2** — même coupe d'une ligne qu'ADR 0031 […] déférée à un
> futur ADR adossé à ADR 0029. » Les curseurs globaux sont « livrables **sans
> attendre** l'item 2 ».

**La primitive de couleur d'[ADR 0027](adr/0027-color-management-beyond-srgb.md)
est un socle partagé, pas une fonctionnalité.** ADR 0027 fait passer
`leyline-color` d'« exposer un profil statique » à « **charger des profils ICC
arbitraires et construire des `cmsTransform` entre eux** » — une petite API de
transformation (chargement / construction / application). Cette primitive n'a
aucune process version propre ; c'est une pièce de fondation. Trois surfaces la
consomment :

* l'**épreuvage écran** et l'**export non-sRGB / filigrane**
  ([ADR 0034](adr/0034-softproofing-watermark-print.md)) — la même transformation
  ICC de sortie ;
* le **module d'impression** ([ADR 0036](adr/0036-print-module.md)) — « pourra
  s'appuyer sur la même primitive de transformation de sortie plutôt que d'en
  inventer une troisième » (Conséquences d'ADR 0027, repris mot pour mot par
  ADR 0036).

**Nuance vérifiée (item 8) :** le DCP ([ADR 0035](adr/0035-camera-profile-dcp.md))
**étend le même crate `leyline-color`** qu'ADR 0027 a transformé en bibliothèque
couleur générale — mais il **applique ses matrices/LUT directement, _pas_ via
`cmsTransform`** (« DCP n'est pas de l'ICC »). Il partage donc le **foyer**
(`leyline-color`) et bénéficie de la maturité que la primitive ICC y apporte,
sans consommer littéralement la transformation ICC elle-même. La primitive ICC
proprement dite est consommée par l'item 7 (ses trois sous-pièces) ; le DCP
cohabite dans le même crate comme second chemin couleur (Conséquences d'ADR 0035 :
« `leyline-color` devient le foyer de deux chemins couleur »).

Construire ce socle **une fois** est donc plus économique que de laisser chaque
fonctionnalité en réinventer une version partielle.

---

# 3. Tiers suggérés

Regroupement par **effort + risque + dépendance** — ce sont des **vagues**, pas
un calendrier : aucune date, aucune notion de « semaine » ou de « sprint »
(cette cadence n'existe nulle part dans les docs du projet). L'ordre **entre**
tiers est une recommandation, pas une contrainte technique (§2).

## Tier A — autonome, faible risque, sans dépendance

Bons candidats pour un démarrage précoce ou parallèle : autonomes, complexité
S/M, rien à débloquer d'abord.

| Item | ADR | Complexité | Note |
|---|---|---|---|
| Courbe tonale | [0030](adr/0030-tone-curve.md) | S/M | Courbe par points seule, spline cubique monotone gelée, luminance seule, précalcul LUT. |
| Suppression de tache | [0032](adr/0032-spot-removal-clone.md) | M | Clonage seul (heal coupé), copie bilinéaire déterministe, étage tôt dans le pipeline. |

## Tier B — socle partagé, peu coûteux, débloque trois surfaces

L'extension ICC de `leyline-color` d'[ADR 0027](adr/0027-color-management-beyond-srgb.md) :
petite, sans process version, et sur laquelle s'appuient épreuvage/filigrane
([0034](adr/0034-softproofing-watermark-print.md)), impression
([0036](adr/0036-print-module.md)) et, dans le même crate, le DCP
([0035](adr/0035-camera-profile-dcp.md)). À poser **une seule fois** plutôt que
de laisser chaque fonctionnalité en inventer une version partielle. La poser tôt
dérisque tout le Tier D côté couleur.

## Tier C — fonctionnalités autonomes de taille moyenne

Global uniquement, sans dépendance, mais plus lourdes que le Tier A.

| Item | ADR | Complexité | Note |
|---|---|---|---|
| Color grading / TSL (global) | [0031](adr/0031-hsl-color-grading.md) | M | HSL dérivé du RGB (8 bandes + falloff), zones pondérées par luminance. Régional déféré. |
| Clarté / texture / dehaze (global) | [0033](adr/0033-clarity-texture-dehaze.md) | M à L | Contraste local unifié à deux rayons ; dehaze dark channel prior en forme close. Masqué déféré. |

## Tier D — le gros pari d'infrastructure

| Item | ADR | Complexité | Note |
|---|---|---|---|
| Réglages locaux masqués | [0029](adr/0029-process-6-local-adjustments.md) | **XL** | Plus grosse mise de fond de tout l'ensemble. |

C'est l'item **où une mauvaise estimation a le plus d'effet de bord** : trois
autres fonctionnalités — le color grading régional (4), le dehaze/texture/clarté
masqué (6) et une éventuelle extension masquée de la suppression de tache (5) —
sont **derrière lui**, **non encore conçues** (chacune exigerait son propre futur
ADR). Sous-estimer 0029, c'est décaler tout ce qui pourrait s'y adosser ensuite.
À traiter comme le poste de risque d'effort n°1.

## Tier E — items à risque non-ingénierie (pas seulement de l'effort)

Ceux-ci demandent un **petit travail de recherche / validation _avant_** de
s'engager sur une implémentation complète — distinct de « c'est juste du temps
d'ingénierie ». L'ADR le dit lui-même dans chaque cas :

* **Profils caméra DCP** ([0035](adr/0035-camera-profile-dcp.md) /
  [0037](adr/0037-dcp-parsing-dependency.md)) — la **correctness colorimétrique**
  « doit être **validée contre de vrais fichiers DCP générés par Adobe et leurs
  rendus de référence avant toute sortie** » (barre d'[ADR 0016](adr/0016-process-3-lens-correction.md)).
  Le _parsing_ du conteneur est, lui, résolu et à faible risque (lecteur maison
  minimal au-dessus du crate `tiff` déjà lié, [ADR 0037](adr/0037-dcp-parsing-dependency.md)) —
  mais l'item **ne peut pas sortir de façon responsable sans matériel de
  référence Adobe en main**, pas seulement du temps d'ingénierie.
* **Module d'impression** ([0036](adr/0036-print-module.md)) — le **mécanisme de
  hand-off OS** (PDF portable vs. raster + API plateforme vs. surface Slint) est
  **explicitement laissé à la PR** : une **question de faisabilité non résolue**,
  à investiguer/prototyper avant que l'estimation d'effort ait un sens. Le rendu
  (dimensionnement `papier × DPI` + profil de destination, réutilisant l'export
  et la primitive ICC d'ADR 0027) est, lui, cadré.

Recommandation : pour chacun, une **petite passe de recherche/validation** (obtenir
les DCP Adobe de référence ; prototyper le chemin de hand-off) avant d'engager
l'implémentation complète.

## Tier F — délibérément non conçu (coupes assumées)

À **lister explicitement** pour qu'elles ne soient pas silencieusement oubliées
au moment de planifier — mais elles ne font **pas** partie du périmètre actuel.
Chacune reviendra dans son propre futur ADR si elle est un jour voulue :

| Coupe | Source |
|---|---|
| Variantes régionales / masquées du color grading | [ADR 0031](adr/0031-hsl-color-grading.md) (adossées à 0029) |
| Variantes régionales / masquées de dehaze/texture/clarté | [ADR 0033](adr/0033-clarity-texture-dehaze.md) (adossées à 0029) |
| _Heal_ seamless (suppression de tache) | [ADR 0032](adr/0032-spot-removal-clone.md) |
| Courbes par canal RGB + UI de courbe paramétrique | [ADR 0030](adr/0030-tone-curve.md) |
| Filigrane image / logo | [ADR 0034](adr/0034-softproofing-watermark-print.md) |
| Planches contact / dispositions N-up (impression) | [ADR 0036](adr/0036-print-module.md) |
| Base de profils DCP embarquée | [ADR 0035](adr/0035-camera-profile-dcp.md) |

---

# 4. Réserve

Ce tiering est informé par l'**effort, le risque et la dépendance** — il ne dit
**rien de la valeur**. Il n'exprime aucun jugement sur les fonctionnalités qui
comptent le plus pour les utilisateurs réels ; cet arbitrage n'appartient qu'à
l'utilisateur. Le présent document est un **intrant** du plan d'implémentation,
pas un substitut à cette décision de priorité.
