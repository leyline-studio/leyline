# ADR 0044 — Un tampon de travail en lumière linéaire large gamut : Rec. 2020 non borné

**Statut :** Accepté — 2026-07
**Complète :** [ADR 0015](0015-color-management-srgb.md) (sRGB de bout en bout) et
[ADR 0027](0027-color-management-beyond-srgb.md) (élargissement en sortie seulement)
**S'appuie sur :** [ADR 0042](0042-versioned-stage-pipeline.md) et
[ADR 0043](0043-collapse-prerelease-render-history.md)

## Contexte

Le tampon de travail du moteur est décrit en tête de `crates/leyline-engine/src/pixels.rs` :

> RGB entrelacé, échantillons `f32`, **sRGB gamma-encodé, écrêté à [0, 1]**.

C'est l'invariant que les vingt étages de `stages.rs` supposent, et il coûte
trois choses distinctes — qu'il faut séparer, parce qu'elles n'ont ni la même
gravité ni le même remède.

**1. Le gamut est perdu au rang 10, définitivement.** `camera_profile::v1`
convertit le capteur en sRGB linéaire via la matrice DCP puis re-gamma-encode
(`lookup(to_srgb, …)`). Un capteur voit largement au-delà de sRGB : bleus
profonds, verts de végétation saturés, rouges de feu. Ces couleurs sont
écrêtées avant que le premier réglage utilisateur ne s'exécute, et aucun
réglage ultérieur ne peut les récupérer. Sans profil DCP, c'est LibRaw qui fait
le même écrêtage (`output_color = 1`).

**2. La marge de hautes lumières est jetée à l'exposition.** `gains::v1`
convertit en lumière linéaire, multiplie, **`clamp(0.0, 1.0)`**, reconvertit.
Donc +1 EV suivi de −1 EV ne rend pas l'image d'origine : ce qui est passé
au-dessus du blanc lors du premier réglage n'existe plus lors du second. Un RAW
14 bits porte plusieurs diaphragmes au-dessus du blanc de rendu ; le pipeline
les détruit à son quatrième étage.

**3. La math lourde s'exécute sur des valeurs gamma-encodées.** Contraste,
saturation, TSL, et surtout tout ce qui *mélange* des pixels — flou gaussien de
la clarté, de la texture, de la netteté, de la réduction de bruit — opèrent sur
un signal non linéaire. Mélanger deux valeurs encodées ne donne pas le mélange
des lumières correspondantes : c'est l'origine des halos et des dérives de
teinte dans les transitions. Le symptôme le plus net est `pixels::luma()`, qui
applique des coefficients Rec. 709 — définis en lumière linéaire — à des
échantillons encodés.

**Ce qui a changé depuis ADR 0027.** Cet ADR a écarté explicitement
l'élargissement interne : « imposerait une nouvelle process version […] pour un
bénéfice que ni V1 ni les items du §7/§8 ne réclament ». Deux prémisses de ce
raisonnement ne tiennent plus. Le bénéfice est désormais réclamé — c'est la
seule limite que la revue de projet classe « très élevée » en complexité parce
qu'elle touche tout le moteur. Et le coût a été divisé : ADR 0042 a supprimé la
copie intégrale du pipeline, donc une nouvelle version d'étage se paie en
dizaines de lignes.

**Ce qu'ADR 0042 n'a jamais eu à affronter.** Toutes les évolutions livrées
jusqu'ici étaient locales à un opérateur. Un changement d'espace de travail ne
l'est pas : il invalide simultanément l'hypothèse d'entrée des vingt étages. Le
modèle « une version par opérateur, gelée » doit donc répondre à une question
qu'il ne s'était pas posée : **que signifie une révision qui citerait
`gains::v2` (linéaire) et `hsl::v1` (gamma) ?** Le §4 y répond.

**Un trou de contrat existant, que cet ADR ferme au passage.** La configuration
du décodeur change les pixels — `DecodeParams::camera_native` bascule LibRaw
entre sa conversion sRGB intégrée et une sortie capteur linéaire. Elle est
décidée à quatre endroits (`preview.rs`, `export.rs`, `print.rs`) par la même
expression `camera_native: camera_profile.is_some()`, et **n'est épinglée dans
aucune carte `stages`**. Une révision ne décrit donc pas entièrement son rendu
aujourd'hui.

## Décision

### 1. L'espace de travail devient Rec. 2020, en lumière linéaire, point blanc D65

Le choix se joue sur trois critères, et Rec. 2020 est le seul à ne perdre sur
aucun :

* **Point blanc.** D65, comme sRGB, Display P3 et Adobe RGB. Aucune adaptation
  chromatique n'a donc à s'insérer entre les étages ni en sortie — alors que
  ProPhoto (D50) et ACEScg (D60) en imposeraient une, à un endroit où toute
  transformation supplémentaire est une occasion de perdre la reproductibilité.
* **Primaires réelles.** Rec. 2020 est défini sur des longueurs d'onde
  monochromatiques réelles ; ProPhoto tire ~13 % de son volume de primaires
  imaginaires, où « saturer » n'a pas de sens physique et où les opérateurs par
  canal se comportent mal.
* **Couverture.** Rec. 2020 englobe largement le gamut des capteurs
  photographiques courants, ce qui est exactement le besoin — un espace plus
  grand encore n'ajoute que de la place vide.

XYZ est écarté séparément : les opérateurs par canal (balance des blancs,
saturation, TSL) n'y ont aucun sens, et il faudrait convertir dans les deux
sens à chaque étage.

### 2. Le tampon cesse d'être borné par le haut

Le nouveau contrat de `Pixels` :

> RGB entrelacé, échantillons `f32`, **Rec. 2020 en lumière linéaire, valeurs
> ≥ 0 sans plafond**.

Le plancher reste : une valeur négative (couleur hors gamut du capteur après
matrice) est écrêtée à 0. Le plafond disparaît — c'est lui qui détruisait la
marge de hautes lumières, et le conserver aurait laissé les deux tiers du
problème en place.

Cette décision a un prix précis, qu'il faut nommer plutôt que découvrir à
l'implémentation : **chaque étage doit définir son comportement au-dessus de
1**, et cela ne se transpose pas mécaniquement. Trois cas sont déjà identifiés :

* `hsl::v1` passe par une conversion RGB↔TSL dont la clarté `l` suppose [0, 1] ;
* `dehaze::v1` estime un *dark channel* normalisé sur la même hypothèse ;
* `whites_blacks::v1` remappe les extrémités d'une plage qui n'a plus
  d'extrémité haute.

Chacun de ces opérateurs est **re-dérivé**, pas porté.

### 3. Deux étages toujours actifs encadrent le pipeline

Un tampon linéaire non borné n'est ni ce que le décodeur produit ni ce qu'un
écran accepte. Les deux conversions deviennent des étages à part entière,
versionnés et gelés comme les autres :

* **`input`, rang 0.** Porte la configuration du décodeur (sortie linéaire, non
  gamma-encodée) *et* la matrice vers Rec. 2020 linéaire : la matrice DCP quand
  un profil est actif (ADR 0035), celle du décodeur sinon. C'est cet étage qui
  ferme le trou de contrat signalé plus haut — le choix `camera_native` cesse
  d'être une décision d'appelant pour devenir une propriété épinglée du rendu.
* **`output_rendering`, rang 900.** Ramène le tampon non borné à un signal
  d'affichage dans [0, 1] : épaule paramétrique sur les hautes lumières, puis
  encodage vers l'espace de sortie. L'épreuvage écran et la conversion ICC
  d'ADR 0027 s'appliquent **après**, inchangés.

Ces deux étages sont les seuls dont `active` vaut toujours vrai. C'est une
extension explicite d'ADR 0042 §2 (« un étage neutre ne s'exécute pas et
n'apparaît pas ») : ils n'ont pas de valeur neutre, puisqu'il n'existe pas de
rendu sans entrée ni sortie. Leur présence obligatoire dans la carte `stages`
est précisément ce qui rend une révision auto-descriptive quant à son espace de
travail, sans compteur global.

`output_rendering` expose **un** réglage utilisateur, `highlight_rolloff`
(0–100) : 0 écrête durement au blanc, les valeurs croissantes allongent une
épaule au-dessus d'un genou fixe. La valeur par défaut est fixée une fois et
gelée avec la version d'étage — c'est un défaut de rendu, pas une préférence
d'application. Le profil ICC de sortie, lui, reste hors de `settings_json`
(ADR 0027) : il décrit la destination, pas la révision.

### 4. L'espace de tampon est une propriété déclarée de chaque version d'étage

`Version` gagne un champ décrivant l'espace qu'elle exige et produit. Trois
règles en découlent :

1. **`plan()` refuse un plan mixte.** Un ensemble d'étages qui ne s'accordent
   pas sur un espace échoue le rendu par une erreur explicite, au même titre
   qu'une version inconnue (`UnknownStage`, `docs/pipeline.md` §3.4). On ne
   rend jamais « au mieux » un pipeline incohérent.
2. **`pin()` choisit la version compatible, pas la plus récente.** Quand un
   étage quitte sa valeur neutre sur une révision existante, il reçoit la
   version la plus récente **dans l'espace que cette révision déclare déjà**.
   Éditer en 2036 une photo de 2026 ne la fait donc jamais basculer d'espace
   par effet de bord.
3. **Changer d'espace est un retraitement.** La migration d'une révision passe
   par le mécanisme existant (`docs/pipeline.md` §4.5) : elle crée une
   **nouvelle** révision, l'ancienne restant rendable à l'identique.

Aucun axe de versionnage global ne réapparaît : l'espace n'est pas un champ de
la révision, c'est une conséquence lisible des versions d'étages qu'elle cite.

### 5. La transition elle-même : effondrement en place, pas doublement

Il n'y aura **pas** de `v2` des vingt opérateurs à côté de vingt `v1`. Les
corps existants sont réécrits en place dans le nouvel espace, et les rendus de
référence recapturés — exactement la posture d'ADR 0043, pour exactement sa
raison :

> Leyline n'ayant pas été publié, ces versions n'engageaient personne.

Aucune révision au monde ne cite `gains::v1` : conserver son code figerait un
rendu que personne n'a jamais obtenu, et doublerait la surface du moteur pour
protéger un utilisateur qui n'existe pas.

Ce qui *est* livré en revanche, et intégralement, c'est la mécanique du §4 —
déclaration d'espace, refus de plan mixte, épinglage compatible — exercée par
l'étage-fixture à deux versions (`stages/fixture.rs`) qui joue déjà ce rôle
depuis ADR 0043. Après publication, ce paragraphe est mort : un changement
d'espace sera alors une nouvelle version de tous les étages concernés, ce que
le §4 rend possible et cher. **C'est la raison de le faire maintenant.**

### 6. La sortie reste sur 8 bits pour l'instant

`Rendered` ne change pas. L'export 16 bits, qui deviendra tentant une fois le
tampon linéaire en place, relève d'un ADR distinct : il touche
`leyline-export` et les formats, pas l'espace de travail.

Un gain arrive tout de même sans rien demander : la transformation ICC
d'ADR 0027 part désormais d'un flottant linéaire au lieu de pixels sRGB 8 bits
déjà quantifiés. Même surface publique, meilleure conversion.

### 7. Ordre de migration : la mécanique d'abord, l'espace ensuite

Comme ADR 0042 §7, rien ne bouge sans preuve — mais la preuve n'est pas la même
ici, et il faut le dire franchement : **le rendu va changer.** L'égalité bit à
bit n'est donc un critère que pour la première moitié du chantier.

1. **Étape à égalité prouvée.** Introduire `input`, `output_rendering`, le
   champ d'espace et le refus de plan mixte **dans l'espace actuel**
   (sRGB gamma). Les empreintes de `tests/golden/renders.json` ne bougent pas :
   c'est ce qui établit que la mécanique est neutre.
2. **Étape à écart justifié.** Basculer l'espace, re-dériver les corps,
   recapturer le manifeste. Chaque écart est justifié sur des images de test
   synthétiques *et* sur un jeu de RAW réels, opérateur par opérateur — pas par
   une empreinte globale qui dirait seulement « ça a changé ».
3. **Recalibrer les réglages.** Un curseur de contraste ou de saturation n'a
   pas le même effet sur un signal linéaire ; les constantes des courbes sont
   ajustées pour que la même valeur produise un résultat comparable, sinon
   chaque préréglage livré ment.

## Conséquences

* **Le rendu par défaut change pour toute photo.** C'est le point à assumer :
  aucune image ne rend comme avant. Acceptable uniquement avant publication,
  et c'est pourquoi l'ADR est écrit maintenant plutôt qu'après.
* **`pixels::luma()` doit être refait deux fois** : ses coefficients
  s'appliqueront enfin à de la lumière linéaire (ce qu'ils supposaient), et les
  bons coefficients dans le nouvel espace sont ceux de Rec. 2020
  (0,2627 / 0,6780 / 0,0593), pas ceux de Rec. 709. Tous les opérateurs pilotés
  par la luma — hautes lumières/ombres, clarté, texture, netteté, color
  grading, réduction de bruit — changent donc de comportement, y compris là où
  leur formule n'a pas bougé.
* **Les allers-retours par table disparaissent.** `gains::v1` et
  `camera_profile::v1` font aujourd'hui deux `lookup` interpolés par
  échantillon et par canal, uniquement pour entrer et sortir de la lumière
  linéaire. En espace linéaire, la balance des blancs et l'exposition
  redeviennent une multiplication. Le gain est à mesurer avec les benchs
  criterion existants, mais le sens est acquis : moins de travail, pas plus.
* **La consommation mémoire ne bouge pas** — le tampon était déjà en `f32`.
  Seule l'absence de plafond change ce qu'il peut contenir.
* **`docs/pipeline.md` est amendé** : §3.1 (les deux étages d'encadrement dans
  la chaîne), §3.3 (tableau des étages, +2 entrées, rangs 0 et 900), §3.4 (le
  refus de plan mixte rejoint les motifs d'échec) et §5.1 (l'espace de travail
  fait partie de ce qu'une révision épingle).
* **ADR 0015 et ADR 0027 reçoivent une note de renvoi.** Aucun des deux n'est
  annulé : ADR 0015 décrivait correctement la V1, et l'élargissement *en
  sortie* d'ADR 0027 reste exact et utile — il devient simplement la seconde
  moitié d'une histoire dont celle-ci est la première.
* **Le risque principal est le comportement au-dessus de 1**, pas la matrice de
  changement d'espace. Une matrice se vérifie sur trois patches ; un opérateur
  conçu pour [0, 1] et nourri de 4.0 produit du plausible-mais-faux, ce qu'aucun
  test d'égalité ne détecte. D'où l'exigence du §7.2 : justification par
  opérateur, sur des images réelles.
* **Le trou de contrat du décodeur est refermé** : `camera_native` cesse d'être
  une expression répétée dans quatre modules pour devenir une propriété
  épinglée de `input::v1`.

## Alternatives écartées

* **Rester en sRGB gamma-encodé (statu quo ADR 0015/0027).** C'était la
  décision correcte tant que le pipeline était court et non publié. Elle ne
  l'est plus une fois que les opérateurs de mélange (clarté, texture, dehaze,
  réduction de bruit, réglages locaux) sont tous livrés : ce sont eux qui
  paient le gamma, et ils sont maintenant la majorité du pipeline.
* **Linéaire mais borné à [0, 1].** Corrigerait le gamut et la math, pas la
  marge de hautes lumières — et laisserait le pipeline dans un état pire à un
  égard : un écrêtage à 1,0 en lumière linéaire est bien plus brutal qu'en
  gamma, parce qu'il concentre la plage utile là où l'œil est le moins
  sensible. Un demi-changement de cette ampleur ne se refait pas deux fois.
* **ProPhoto (ROMM) linéaire, comme Adobe.** Gamut plus large, mais D50 impose
  une adaptation chromatique de chaque sortie, et ses primaires imaginaires
  rendent les opérateurs par canal mal définis dans une part réelle du volume.
  Le seul argument fort est « c'est ce que fait Lightroom » — argument de
  compatibilité, alors que rien ici n'a besoin d'être compatible avec
  Lightroom.
* **ACEScg (AP1).** Excellent pour du scene-referred, mais D60, aucun outil
  photo ne l'attend, et son gamut supplémentaire ne sert aucun capteur photo.
* **XYZ linéaire.** Gamut infini, mais chaque opérateur par canal exigerait un
  aller-retour vers un espace RGB — soit exactement le coût qu'on retire au
  gamma.
* **Un champ d'espace de travail au niveau de la révision.** Plus lisible dans
  `settings_json`, mais c'est un compteur global déguisé — ce qu'ADR 0042 §2 a
  retiré, et la révision cesse d'être auto-descriptive étage par étage. Un
  moteur pourrait alors lire un espace qui contredit les versions citées ;
  déduire l'espace des versions rend cette contradiction impossible à écrire.
* **Insérer automatiquement des étages de conversion entre espaces voisins.**
  Rendrait les plans mixtes légaux — au prix d'allers-retours qui détruisent
  précisément le bénéfice recherché (chaque conversion vers sRGB réécrête le
  gamut) et d'un rendu que personne n'a conçu ni validé. Refuser est plus
  honnête que rendre au mieux.
* **Conserver les vingt `v1` en sRGB gamma et livrer vingt `v2`.** Ce sera
  l'obligation après publication, et le §4 la rend praticable. Avant
  publication, c'est doubler la surface du moteur pour figer des rendus qu'aucune
  révision ne cite (ADR 0043).
* **Un rendu de sortie implicite et non paramétrable.** Une épaule fixe dans le
  code aurait suffi techniquement, mais l'utilisateur qui vient de gagner
  plusieurs diaphragmes de marge doit pouvoir décider comment ils reviennent
  dans l'image : c'est le réglage qui rend le bénéfice visible plutôt que
  théorique.
