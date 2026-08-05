# ADR 0041 — Rendu interactif : preview à la résolution d'affichage et cache d'étages de pipeline

**Statut :** Accepté — 2026-07

## Contexte

`docs/roadmap.md` phase 7 (« Optimisations CPU/GPU ») est la phase courante.
Deux mesures prises le 2026-07-25 (`crates/leyline-engine/benches/process1.rs`,
groupe `process11`, image synthétique 3 Mpx, i9-9900K 16 threads) cadrent le
problème :

* le coût d'un curseur isolé va de **~0 ms** (courbe tonale, color grading,
  matrice de profil) à **+232 ms** (masque brosse à 64 dabs), **+112 ms**
  (dehaze), **+94 ms** (huit taches), **+70 ms** (réduction de bruit) ;
* le rendu complet d'un CR2 réel (3888×2592) en preview `Small` prend **0,97 s**
  à neutre et **2,39 s** tous curseurs actifs, décodage compris.

Deux gaspillages structurels expliquent l'essentiel de cet écart, et **aucun des
deux n'est un problème d'opérateur** — chaque opérateur pris isolément est déjà
raisonnable :

**1. Le pipeline tourne à une résolution qui n'est jamais affichée.**
`preview::plan_preview` décode en `half_size` pour les classes `Thumbnail` et
`Small`, puis développe **le buffer décodé entier** avant que
`leyline_preview::cache` ne réduise le résultat à `max_edge`. Pour un boîtier
10 Mpx en preview `Small`, cela développe 1944×1296 (2,5 Mpx) pour afficher
1024×683 (0,7 Mpx) : **~3,6× de pixels calculés puis jetés**, à chaque
déplacement de curseur.

**2. Chaque rendu repart du buffer décodé.** `process11::develop` reconstruit la
chaîne complète à chaque appel. Un étage neutre est sauté, mais tout étage
**non neutre en amont** de celui qu'on modifie est recalculé pour rien : bouger
`sharpening` (dernier étage, ~13 ms) re-exécute dehaze, clarté, texture, TSL et
les réglages locaux déjà calculés à l'identique au rendu précédent. C'est la
différence structurelle avec Lightroom, Capture One et Darktable, qui mettent en
cache les étages intermédiaires et ne rejouent que l'aval du nœud édité.

Une troisième piste — **exécuter le pipeline sur le GPU** — a été explicitement
écartée par **ADR 0012** (« déterminisme inter-GPU non garanti ; reporté à une
exploration ultérieure de phase 7 »). Le présent ADR **ne rouvre pas** ce choix :
il traite les deux gaspillages CPU, qui sont sans risque de déterminisme, sans
dépendance nouvelle et multiplicatifs entre eux. La question GPU sera reprise
avec les mesures d'après-optimisation, dans son propre ADR, si elle se justifie
encore.

## Décision

Les deux optimisations portent **exclusivement sur le chemin preview**. Export et
impression restent inchangés, à pleine résolution, sans cache d'étages, **bit
pour bit identiques à aujourd'hui**. Il n'y a donc **aucune nouvelle process
version** : la process version décrit ce que produit le contrat de rendu
(`docs/pipeline.md` §5), et ce contrat ne bouge pas.

### 1. La preview est développée à sa résolution d'affichage

`plan_preview` cesse de développer le buffer décodé pour développer un buffer
déjà réduit à la taille de la classe demandée (`leyline_preview::max_edge`). Le
redimensionnement passe **avant** le pipeline au lieu d'après.

L'ordre exact devient : décoder (`half_size` inchangé) → réduire à `max_edge` →
développer → encoder. `PreviewKind::Full` n'a pas de `max_edge` : ce chemin est
inchangé, il développe à pleine résolution comme aujourd'hui.

> **Suite, 2026-08-05.** Ce paragraphe ne dit rien de ce qu'il advient du
> buffer réduit : il était rebâti à chaque rendu, ce qui est devenu le poste
> dominant une fois le §3 en place. [ADR 0076](0076-proxy-cache.md) le met en
> cache à côté du décodage.

### 2. Les rayons exprimés en pixels sont mis à l'échelle du proxy

Développer une image réduite avec des rayons inchangés donnerait un rendu
**faux**, pas seulement rapide : un flou de σ = 40 px sur un buffer 3,6× plus
petit couvre 3,6× plus de sujet. Tout paramètre dénominé en pixels est donc
multiplié par le facteur d'échelle `s = largeur_proxy / largeur_décodée` :

| Paramètre | Où | Unité |
| --- | --- | --- |
| `sharpening.radius` | paramètre utilisateur | σ pixels |
| `CLARITY_RADIUS` (40,0) | constante d'étage | σ pixels |
| `TEXTURE_RADIUS` (6,0) | constante d'étage | σ pixels |
| `DEHAZE_PATCH_RADIUS` (7) | constante d'étage | rayon pixels |
| σ de la réduction de bruit (`k·2,0`, `k·3,0`) | dérivé de la force | σ pixels |

Les autres paramètres spatiaux sont déjà **normalisés** `[0, 1]` et donc
invariants d'échelle : recadrage, rotation, masques radial/gradient/brosse,
taches (`spot.radius × max(w, h)`), correction d'objectif (géométrie en
coordonnées normalisées). Ils ne sont pas touchés.

Cette mise à l'échelle est une **approximation**, pas une identité : réduire
puis flouter à σ·s n'égale pas flouter à σ puis réduire. Pour une gaussienne
l'écart est petit et va dans le bon sens. Le point important est que la preview
d'aujourd'hui est **déjà** une approximation de l'export — elle développe à
2,5 Mpx puis réduit à 0,7 Mpx, ce qui fait par exemple disparaître une netteté
de rayon 1 px. La preview proxy n'introduit pas une infidélité nouvelle : elle
en remplace une par une autre, moins coûteuse et plus proche de ce que l'export
donnera à taille d'affichage égale.

### 3. Le pipeline preview met en cache des étages intermédiaires

Le rendu preview gagne un cache d'**états intermédiaires**, en mémoire, tenu par
la `Library` à côté du cache de décodage, et jeté avec elle.

> **Amendement du 2026-08-02, à l'implémentation.** Ce paragraphe disait
> « tenu par la session d'édition ouverte ». C'était intenable : la vue
> develop de Studio rend par `Library::preview`, jamais par une
> `EditSession`, si bien qu'un cache porté par la session n'aurait jamais
> été touché par l'interaction même qu'il vise. Il vit donc où vit déjà
> `DecodeCache`. Rien d'autre ne change — le cache reste purement dérivé,
> propre au chemin preview, et jeté à volonté.

Le pipeline est une **séquence linéaire** d'étages. Chaque point de contrôle
retient `(index d'étage, empreinte des réglages de tous les étages amont,
buffer)`. À chaque rendu, le moteur calcule les empreintes de préfixe, retient
le **point de contrôle valide le plus profond**, et ne rejoue que l'aval. Bouger
`sharpening` avec dehaze et clarté actifs ne recalcule alors que `sharpening`.

Les points de contrôle ne sont pas placés à chaque étage — le coût mémoire ne le
justifierait pas — mais **avant les étages chers**, là où le gain paie sa
copie : après correction d'objectif et taches (chers, quasi jamais retouchés en
rafale), après le bloc tonal, après clarté/texture/dehaze, après les réglages
locaux. À la résolution proxy un buffer coûte ~8 Mo (0,7 Mpx × 3 canaux ×
`f32`), soit une trentaine de mégaoctets pour l'ensemble : acceptable pour une
session, et une raison de plus pour que ce cache **n'existe que sur le chemin
preview**, jamais en export où les buffers pleine résolution le rendraient
prohibitif.

**Un seuil désigne une position, pas un étage.** Un point de contrôle se pose
avant le premier étage dont le rang atteint le seuil, jamais devant un rang
exact : l'étage qui occupe ce rang est souvent neutre, donc absent du plan.
La mesure l'a montré — en visant le rang exact, trois des quatre points de
contrôle n'étaient jamais pris, et le gain tombait à 10 %.

**Résultat mesuré le 2026-08-02** (carte de test 1024×683, bloc tonal +
clarté + texture + dehaze + netteté actifs, `--release`) : déplacer le
curseur de netteté, dernier étage du plan, passe de **~60 ms à ~14 ms**,
soit **−78 %**. C'est le gain qu'annonçait le §Contexte.

Le cache est purement **dérivé** : le jeter à tout instant ne change aucun
pixel, seulement le temps de rendu. C'est ce qui le rend sûr — il ne peut pas
introduire d'incohérence d'état, au pire une lenteur.

### 4. Ce qui ne change pas

* **Le contrat de reproductibilité** (`docs/pipeline.md` §5) : inchangé, il
  porte sur le rendu d'export.
* **Les process versions** : aucune nouvelle. Un preview n'est pas une révision.
* **Les modules `processN.rs` gelés** (ADR 0028) : la mise à l'échelle des
  rayons est appliquée **par l'appelant** (le planificateur de preview), qui
  transmet un facteur d'échelle ; elle ne réécrit pas la math figée d'un module
  de process existant.

## Conséquences

* **Le coût d'un déplacement de curseur baisse de deux facteurs indépendants qui
  se multiplient** : ~3,6× de pixels en moins, et le saut de tout étage amont
  inchangé. Sur le pire cas mesuré (masque brosse, 255 ms à 3 Mpx), les deux
  ensemble ramènent la classe de latence dans la même bande que les curseurs
  ordinaires.
* **La preview n'est plus le même calcul que l'export.** C'était déjà vrai (voir
  §2) mais cela devient une propriété **assumée et documentée** plutôt qu'un
  effet de bord du redimensionnement final. Corollaire assumé : un défaut visible
  seulement à pleine résolution (bruit fin, halo de netteté) ne se juge pas sur
  une preview réduite — c'est ce à quoi sert `PreviewKind::Full`.
* **Un facteur d'échelle traverse désormais l'API de rendu interne.** C'est une
  entrée de plus à passer correctement ; le mauvais facteur donne un rendu
  faux et non une erreur, donc il est couvert par des tests dédiés (un rendu
  proxy et un rendu pleine résolution réduit doivent rester proches à tolérance
  donnée).
* **Le cache d'étages ajoute de l'état mutable à la session d'édition.** Il est
  dérivé et jetable, donc sans risque de corruption, mais il faut que
  l'empreinte de préfixe soit **exhaustive** : un réglage oublié dans
  l'empreinte produirait un rendu obsolète. C'est le seul vrai risque de
  correction de cet ADR, et il est testable directement (modifier chaque
  paramètre l'un après l'autre doit invalider le cache).
* **La question GPU reste ouverte, et mieux posée.** Ces deux optimisations
  retirent du travail *inutile* ; le GPU accélère du travail *utile*. Les
  mesurer d'abord évite de porter sur GPU un pipeline qui calculait 3,6× trop de
  pixels — et donnera les chiffres qui manquaient à ADR 0012 pour trancher.

## Alternatives écartées

* **Passer le pipeline sur GPU (wgpu/OpenCL) tout de suite** : écarté par
  ADR 0012 pour le déterminisme inter-GPU, et prématuré ici — cela optimiserait
  un pipeline qui fait structurellement trop de travail. À reprendre après
  mesure, dans son propre ADR.
* **Rendu progressif (afficher une version grossière puis raffiner)** : masque
  la latence au lieu de la réduire, et double les chemins de rendu à maintenir.
  Le proxy à résolution d'affichage donne le même ressenti sans second chemin.
* **Cache d'étages persistant sur disque** : la reconstruction d'un étage coûte
  moins cher que sa sérialisation/relecture aux tailles en jeu, et cela
  introduirait un artefact de cache à invalider entre versions du moteur — tout
  ce que `docs/pipeline.md` évite en ne stockant que des réglages.
* **Mettre en cache un seul état (le buffer décodé)** : c'est exactement l'état
  actuel (`DecodeCache`) — il évite le re-décodage, pas le re-calcul.
* **Développer la preview à `max_edge` sans mettre les rayons à l'échelle** :
  plus rapide et **faux** ; les curseurs de détail et de contraste local
  n'auraient plus le même sens d'une classe de preview à l'autre.
