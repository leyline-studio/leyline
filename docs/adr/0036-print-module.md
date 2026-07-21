# ADR 0036 — Module d'impression : « un export avec une dimension physique et un profil de destination », une photo par page, hand-off OS laissé à Studio

**Statut :** Accepté — 2026-07

## Contexte

`docs/v2-scope.md` §7 regroupe trois manques sous un même item — épreuvage
écran, filigrane, module d'impression — en soulignant que **c'est l'item le
moins « pipeline »** : aucun des trois ne modifie les pixels de la révision
stockée. ADR 0034 a tranché les deux premiers tiers (épreuvage vue seule,
filigrane texte à l'export) et **déféré explicitement l'impression à « son
propre futur ADR »**, la cotant L/XL et « surtout UI + chemin de sortie
dédié », en refusant de la stuber avec des décisions de remplissage. Ce
document est cet ADR annoncé.

Deux décisions transversales sont **consommées, non re-litigées, ici** :

* **ADR 0027** a élargi `leyline-color` vers une bibliothèque de
  transformation ICC générale (chargement de profil, construction de
  `cmsTransform`, application), et a explicitement prévu que « le module
  d'impression pourra s'appuyer sur la même primitive de transformation de
  sortie plutôt que d'en inventer une troisième ». C'est exactement ce que ce
  document fait : la conversion vers le profil imprimante/papier réutilise la
  primitive qu'ADR 0027 a construite et qu'ADR 0034 réutilise déjà pour
  l'épreuvage et l'export non-sRGB.
* **ADR 0025** (`docs/engine-api.md` §12) a unifié l'export derrière une seule
  `ExportRequest` (les versions à traiter), une `ExportRecipe`
  (`Adhoc`/`Preset`) et un `ExportPresetId` résolu à l'exécution — la forme que
  ce document reprend telle quelle pour l'impression.

Restait à trancher, item 7 dernier tiers, la **forme concrète** du module
d'impression. C'est l'objet de cet ADR.

## Décision

### Ni process version, ni `settings_json`, ni étage de pipeline

**L'impression n'est pas une process version, ne touche pas `settings_json`
de développement, n'insère aucun étage dans l'ordre du pipeline
`docs/pipeline.md` §3.1.** C'est exactement la même catégorie que le filigrane
et l'épreuvage d'ADR 0034 : imprimer ne change pas les pixels d'une révision,
c'est une **préoccupation de sortie**. Le raisonnement d'ADR 0034 pour ses deux
pièces s'applique mot pour mot ici — rien dans l'impression ne modifie les
pixels d'une révision stockée, donc le contrat de reproductibilité
(`docs/pipeline.md` §5, « même révision → mêmes pixels ») n'est pas engagé, et
aucune process version n'est introduite (la règle de numérotation d'ADR 0028
n'a donc **pas** à jouer ici).

### Périmètre V2 — une photo par page, les planches sont coupées

**La V2 imprime une seule photo par page.** Les planches contact et les
dispositions N-up (plusieurs photos par page, math de grille arbitraire,
rapports d'aspect mixtes, règles de recadrage-pour-remplir) sont **coupées de
V2** — coupe délibérée, pas un oubli. Une planche est un problème de **moteur
de mise en page** matériellement plus grand que « page + marges + DPI » pour
une image unique : il faut trancher la géométrie de grille, la gestion des
orientations mixtes, le recadrage-à-la-cellule, la pagination multi-pages. C'est
une autre nature de chantier, exactement comme ADR 0032 a coupé le *heal*
seamless, ADR 0030 la courbe paramétrique, ADR 0034 le filigrane image/logo et
ADR 0031/0033 les effets régionaux : nommer **la plus petite chose réellement
utile** — imprimer une photo à une taille/un papier/un profil choisis — et
couper le reste proprement plutôt que de le demi-concevoir. Les planches
reviendront dans leur propre futur ADR si elles sont un jour voulues.

### Architecturalement, « un export avec une dimension physique et un profil de destination »

L'impression **n'est pas un nouveau sous-système**. Le chemin de rendu réutilise
la machinerie existante de `leyline-export` :

* Le rendu-vers-tampon puis l'encodage de `leyline-export`
  (`crates/leyline-export/src/lib.rs`) sont réutilisés tels quels ; la seule
  différence est **comment on calcule les dimensions cibles en pixels** : au
  lieu d'un `max_edge` en pixels (ADR 0025/0027), on calcule
  `taille_papier × DPI` (p. ex. 15 × 10 cm à 300 DPI). C'est un mode de
  dimensionnement de plus, pas un chemin de rendu de plus.
* La conversion vers le profil imprimante/papier réutilise **la primitive ICC
  d'ADR 0027** (`leyline-color`) — la même que l'épreuvage et l'export non-sRGB
  d'ADR 0034 emploient déjà. Optionnellement, la vue d'épreuvage d'ADR 0034
  (avec alerte de gamut) peut être chaînée **avant** de valider l'impression,
  pour que l'utilisateur prévisualise le comportement colorimétrique du tirage
  avant de dépenser papier et encre.

**Aucun algorithme de rendu nouveau n'est introduit nulle part dans cet ADR.**

### Persistance — un concept `print_presets`, parallèle à `export_presets`

L'impression se stocke comme un **preset**, sur le modèle exact
d'`export_presets` (`docs/catalog.md` §27) et de `develop_presets`. Un preset
d'impression capture : taille de papier, marges/orientation, DPI cible,
référence au profil ICC de destination, et intention de rendu — le tout dans un
blob `settings_json`, **le même mécanisme** que celui déjà établi pour
`export_presets`/`develop_presets`, aucun mécanisme nouveau inventé.

Les **données par job** (quelles photos, combien de copies) restent un intrant
au moment de la requête, **pas** une partie du preset — exactement comme
`ExportRequest.versions` est séparé de la `ExportRecipe`/du preset stocké
(`docs/engine-api.md` §12, ADR 0025). On suit cette forme directement :

```rust
pub enum PrintRecipe {
    /// Réglages fournis par l'appelant, non stockés.
    Adhoc(PrintSettings),
    /// Un preset stocké, résolu à l'exécution de la requête.
    Preset(PrintPresetId),
}

pub struct PrintRequest {
    pub versions: Vec<VersionId>,   // quelles photos — intrant de job, jamais dans le preset
    pub recipe: PrintRecipe,        // Adhoc(...) ou Preset(id), comme ExportRecipe
    pub copies: u32,                // intrant de job, jamais dans le preset
}
// forme exacte laissée à la PR, comme pour tout ADR de cette série.
```

Table `print_presets`, parallèle à `export_presets` (`docs/catalog.md` §27,
colonnes id/uuid/name/settings_json/created_at reprises à l'identique) :

```sql
CREATE TABLE print_presets (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    settings_json TEXT NOT NULL,   -- papier, marges, orientation, DPI, profil ICC, intention
    created_at INTEGER NOT NULL
);
```

Esquisse d'un `settings_json` de preset d'impression :

```json
{
    "paper": "A4",
    "orientation": "portrait",
    "margins_mm": { "top": 10, "right": 10, "bottom": 10, "left": 10 },
    "dpi": 300,
    "profile": "Profiles/Print/CansonBaryta.icc",
    "intent": "relative-colorimetric"
}
```

### Split crate/propriété — la décision architecturale de fond

Le partage de responsabilité est la vraie décision de cet ADR ; il se lit en
deux moitiés.

**Le rendu — produire un raster/fichier prêt à imprimer à la taille physique
cible et dans le profil de destination — est propriété du moteur**, en
extension de `leyline-export`/`leyline-color`, **sans nouveau crate**. Même
raisonnement qu'ADR 0035 pour le placement du travail DCP : c'est de la logique
de domaine couleur/sortie (dimensionnement physique + transformation ICC de
destination), pas de l'infrastructure couplée aux tampons qui exigerait son
propre crate. Le dimensionnement `papier × DPI` et la conversion de profil sont
adjacents au travail d'export déjà logé là.

**Le hand-off physique vers une imprimante — dialogue d'impression de l'OS,
communication avec le pilote, spooling — n'est explicitement PAS du travail
moteur.** `docs/engine-api.md` §14 est catégorique : « Pas de rendu à l'écran :
le moteur produit des fichiers et des buffers, l'affichage appartient au
client », « Pas de gestion de fenêtres, de raccourcis, de sélection UI ». Le
dialogue d'impression de l'OS est précisément de la gestion de fenêtres et
d'intégration système. Et c'est **exactement le même patron** qu'ADR 0020
(barre de menu) et ADR 0021 (menus contextuels) ont déjà appliqué :
l'intégration OS/UI vit **entièrement dans `leyline-studio`**, câblée sur des
appels moteur déjà existants, **sans ajouter la moindre surface au moteur**
(« Aucun changement du modèle d'événements ou de l'API moteur », ADR 0020 ;
« Aucune nouvelle capacité moteur », ADR 0021). Ici de même : Studio appelle le
moteur pour **rendre un fichier prêt à imprimer**, puis Studio — pas le
moteur — invoque le mécanisme d'impression natif de la plateforme.

### Risque ouvert explicite — le mécanisme de hand-off, laissé à la PR

**Comment** Studio remet précisément la sortie rendue au flux d'impression de
l'OS est un **risque genuinely non résolu**, laissé à la PR d'implémentation —
pas une décision que cet ADR esquive arbitrairement. Les finalistes sont
nommés, le choix est différé :

* **Un PDF auto-généré à la taille de page physique cible, profil embarqué** —
  probablement le choix le plus portable entre les dialogues d'impression
  Windows/macOS/Linux, puisque la quasi-totalité des flux d'impression OS
  acceptent un PDF. Mais ce n'est **pas** posé ici comme un fait acquis.
* **Un raster brut passé à une API d'impression spécifique à la plateforme.**
* **La surface d'impression que Slint pourrait ou non exposer** elle-même.

Ce choix dépend des capacités réelles de Slint et de l'intégration d'impression
native de chaque plateforme **au moment venu** — il est nommé avec la même
honnêteté qu'ADR 0035 a employée pour le choix du parseur DCP. Cet ADR
n'invente **pas** de résolution factice (il n'affirme pas « Leyline génère un
PDF » comme un fait tranché) : il énonce les options finalistes et diffère
explicitement le choix.

## Conséquences

* **Cet ADR clôt entièrement l'item 7 de `docs/v2-scope.md`** : ses trois
  sous-pièces sont désormais tranchées — épreuvage écran et filigrane par
  ADR 0034, module d'impression par ce document. Le seul point restant est le
  **risque nommé** du mécanisme de hand-off OS, flaggé pour la PR
  d'implémentation.
* **Aucune process version, aucun étage pipeline** : comme le filigrane et
  l'épreuvage d'ADR 0034, l'impression vit hors du contrat de reproductibilité
  des révisions (`docs/pipeline.md` §5) — c'est une surface de sortie, pas une
  modification de révision.
* **Le rendu d'impression hérite gratuitement de la plomberie d'export et de la
  primitive ICC d'ADR 0027** : aucun algorithme de rendu neuf, aucun troisième
  chemin couleur — le dimensionnement `papier × DPI` remplace `max_edge`, la
  conversion de profil est celle d'ADR 0027 déjà partagée avec l'épreuvage et
  l'export non-sRGB.
* **Le preset d'impression hérite gratuitement du patron de presets** : une
  table `print_presets` parallèle à `export_presets`, un blob `settings_json`,
  aucun mécanisme nouveau ; les données de job (photos, copies) restent séparées
  du preset, comme `ExportRequest.versions` l'est de `ExportRecipe`.
* **Le moteur ne gagne aucune surface d'intégration OS** : le hand-off imprimante
  vit entièrement dans `leyline-studio`, exactement comme les menus (ADR 0020)
  et les menus contextuels (ADR 0021) — le moteur rend un fichier, Studio le
  remet à l'OS.
* **Les planches contact restent ouvertes pour un futur ADR** avec leur vrai
  coût (moteur de mise en page, grille, orientations mixtes, pagination) : la V2
  refuse seulement de s'y engager, elle ne ferme pas la porte.
* **Le mécanisme de hand-off reste un risque ouvert assumé** pour la PR : PDF
  portable, raster + API plateforme, ou surface Slint — décision qui dépend des
  capacités réelles au moment de l'implémentation, pas tranchée spéculativement
  ici.
* **Ne préjuge pas d'un futur système de plugins/modules.** `docs/roadmap.md`
  liste « Plugins, SDK stable » en Long terme, hors V2. Le split moteur/Studio
  décidé ici (rendu = moteur, hand-off OS = Studio) est un choix de placement
  interne pour la V2 — il n'exclut pas qu'un futur système de plugins vienne
  s'y greffer, par exemple pour fournir des backends d'impression alternatifs
  ou des dispositions de planche (aujourd'hui coupées, voir ci-dessus) sans
  toucher au moteur. Rien ici n'engage la forme de ce futur mécanisme.

## Alternatives écartées

* **Supporter les planches contact / dispositions multi-images par page dès la
  V2.** Écarté : une planche est un moteur de mise en page (géométrie de grille
  arbitraire, rapports d'aspect mixtes, recadrage-à-la-cellule, pagination
  multi-pages) — un problème matériellement plus grand que page/marges/DPI pour
  une image unique, d'une autre nature que le reste. Le demi-concevoir ici
  inventerait une architecture que personne n'a cadrée (« No code before
  architecture »). On nomme la plus petite chose utile (une photo par page) et
  on coupe le reste proprement, comme ADR 0032/0030/0034 l'ont fait pour leurs
  périmètres respectifs. Les planches auront leur propre ADR si elles sont un
  jour voulues.
* **Faire de l'impression un étage du pipeline de développement / une process
  version.** Écarté : imprimer ne modifie pas les pixels d'une révision stockée,
  c'est une préoccupation de sortie — exactement le constat qu'ADR 0034 a posé
  pour le filigrane et l'épreuvage. L'inscrire dans `settings_json` ou lui
  attribuer une process version introduirait un état qui n'affecte aucun pixel
  de révision et mentirait sur ce qu'est une révision, tout en engageant
  inutilement le contrat « même révision → mêmes pixels » (`docs/pipeline.md`
  §5). L'impression vit à la sortie, comme l'export.
* **Faire posséder au moteur l'intégration du dialogue d'impression de l'OS.**
  Écarté : `docs/engine-api.md` §14 exclut explicitement la gestion de fenêtres
  et le rendu à l'écran du moteur — « l'affichage appartient au client ». Un
  dialogue d'impression système est de l'intégration OS/UI, précisément ce
  qu'ADR 0020 (barre de menu) et ADR 0021 (menus contextuels) ont logé
  **entièrement dans `leyline-studio`** sans ajouter la moindre surface au
  moteur. Faire du moteur un gestionnaire de dialogue d'impression casserait ce
  patron établi et le rendrait dépendant de la plateforme — exactement ce que la
  frontière moteur/client existe pour empêcher. Le moteur rend un fichier ;
  Studio le remet à l'OS.
* **Trancher dès maintenant un mécanisme de hand-off précis (p. ex. « toujours
  générer un PDF »).** Écarté : ce choix dépend des capacités réelles de Slint
  et de l'intégration d'impression native de chaque plateforme au moment de
  l'implémentation — l'affirmer tranché aujourd'hui serait inventer une
  résolution factice. Le PDF portable est le finaliste probable, mais le raster
  + API plateforme et une éventuelle surface Slint restent des candidats réels.
  Cet ADR nomme les finalistes et diffère honnêtement le choix à la PR, dans le
  même esprit qu'ADR 0035 a laissé ouvert le choix du parseur DCP — un risque
  assumé, pas esquivé.
