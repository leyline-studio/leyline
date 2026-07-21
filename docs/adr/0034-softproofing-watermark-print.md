# ADR 0034 — Épreuvage écran et filigrane : deux surfaces de sortie, aucun process ; le module d'impression reste hors décision

**Statut :** Accepté — 2026-07

## Contexte

`docs/v2-scope.md` §7 regroupe trois manques sous un même item — épreuvage
écran, filigrane, module d'impression — en soulignant que **c'est l'item le
moins « pipeline »** : aucun des trois n'est une modification pixel de la
révision stockée, mais du travail preview / export / gestion des couleurs /
UI.

Le verrou transversal qui les bloquait tous — le gel sRGB de V1 (ADR 0015) —
est déjà tranché par **ADR 0027** : le pipeline de rendu reste sRGB, mais
`leyline-color` passe d'« exposer un profil statique » à « charger des
profils ICC arbitraires et construire des `cmsTransform` entre eux » — une
petite API de transformation (chargement de profil, construction de
transform, application). ADR 0027 note explicitement que l'épreuvage et
l'export non-sRGB **partagent la même primitive sous-jacente** et que le
module d'impression pourra s'appuyer dessus « plutôt que d'en inventer une
troisième ». Ce document consomme cette primitive ; il ne la re-dérive pas.

Restait à trancher, item 7, ce qu'ADR 0027 a délibérément laissé ouvert : la
**forme concrète** de l'épreuvage et du filigrane. C'est l'objet de cet ADR.

**Cadrage explicite d'entrée — ce document ne conçoit pas le module
d'impression.** L'impression (mise en page, formats papier, marges, planches,
sortie via profil imprimante) est une initiative franchement séparée et bien
plus large — « surtout UI + chemin de sortie dédié » selon `docs/v2-scope.md`
§7 lui-même, coté **L/XL** — dont la conception responsable demande sa propre
passe le jour où ce chantier sera réellement planifié, exactement comme le §7
l'a déjà signalé. Cet ADR **ne stub pas** l'impression avec des décisions de
remplissage : il tranche les deux pièces réellement traitables aujourd'hui —
le filigrane et la surface moteur/API de l'épreuvage — et laisse l'impression
à un futur ADR dédié.

## Décision

**Ni le filigrane ni l'épreuvage écran n'est une process version, et aucun
des deux ne touche `settings_json` de développement.** Le tableau de
`docs/v2-scope.md` §7 le pose déjà pour chacun : l'épreuvage est une
transformation « à l'affichage » qui ne modifie **ni** `settings_json` **ni**
les pixels de la révision (« Aucune process/schema : transformation non
persistée ») ; le filigrane est une décoration « à l'export », de même
catégorie que format/qualité (« Aucune process de développement »). Les deux
vivent donc là où la configuration de sortie vit déjà — à l'export,
`ExportRecipe`/`ExportSettings` (ADR 0025, `docs/engine-api.md` §12) ; à
l'affichage, la requête de preview (`docs/engine-api.md` §11) — jamais dans
une révision ni un `process`.

**Contrairement à ADR 0029–0033, ce document n'introduit aucune process
version et n'insère aucun étage dans l'ordre du pipeline `docs/pipeline.md`
§3.1.** C'est la conséquence directe de la nature « non-pipeline » de l'item
relevée au §7 : rien ici ne change les pixels d'une révision stockée.

### Filigrane — texte seul en V2, l'image/logo est coupée

Le filigrane V2 est **exclusivement textuel** : une chaîne, sa police, sa
taille, sa couleur, son opacité, et son ancrage/position. Un tel filigrane
est **entièrement autonome** dans `export_presets.settings_json`
(`docs/catalog.md` §27) — aucun nouveau problème de référence de ressource.

Le **filigrane image/logo est coupé du périmètre V2** — coupe délibérée, pas
un oubli. Un logo poserait une vraie question de conception que cet ADR
choisit de ne pas trancher à la légère : où vit le fichier logo et comment
est-il référencé ? Un chemin relatif dans la bibliothèque
(`docs/catalog.md` §2.3) — mais le logo n'est pas un asset photo, la règle des
chemins relatifs ne le couvre pas évidemment ? Un chemin absolu choisi par
l'utilisateur — que la portabilité de la bibliothèque (§2.3) interdit
justement de stocker ? Le filigrane texte contourne entièrement cette
question (il n'a aucune ressource externe) et couvre le cas d'usage nommé
dans les listes de manque typiques : nom du photographe, copyright, site web.
Le logo s'ajoutera dans son propre changement le jour où ce problème de
référence sera tranché.

**Placement dans le chemin de rendu d'export.** Le filigrane texte est
composité comme **toute dernière étape du rendu d'export**, *après* la
transformation ICC optionnelle vers le profil de destination d'ADR 0027 et
**immédiatement avant l'encodage** (`leyline-export::encode`,
`crates/leyline-export/src/lib.rs`). Raisonnement : le texte du filigrane doit
être tracé directement dans l'espace RGB de destination (quel que soit le
profil visé par l'export), **pas** repassé par une transformation
colorimétrique photographique conçue pour le contenu image. Le dessiner après
la conversion de profil évite ce désalignement — le décor de sortie hérite de
l'espace de sortie, il ne le traverse pas.

**Stockage — additif.** `ExportSettings` (`crates/leyline-export/src/lib.rs`,
qui **double comme** le `settings_json` des presets d'export, `docs/catalog.md`
§27) gagne un champ optionnel `watermark`. Absent = pas de filigrane. Aucun
bump de schéma d'export : c'est un ajout de champ optionnel de valeur neutre,
au même titre que `max_edge` l'a été.

Esquisse (une recette d'export avec filigrane texte) :

```json
{
    "format": "jpeg",
    "quality": 90,
    "max_edge": 2048,
    "watermark": {
        "text": "© Quentin Duval 2026",
        "font": "sans",
        "size": 3.0,
        "color": "#FFFFFF",
        "opacity": 0.7,
        "anchor": "bottom-right"
    }
}
```

Cas neutre — champ absent, export inchangé par rapport à aujourd'hui :

```json
{ "format": "jpeg", "quality": 90 }
```

> **Note d'implémentation (pas une édition de code ici).** `ExportSettings`
> refuse aujourd'hui tout champ inconnu (`#[serde(default,
> deny_unknown_fields)]`, `crates/leyline-export/src/lib.rs`), et un test s'en
> sert précisément avec `"watermark": "logo.png"` comme exemple de champ non
> reconnu à rejeter (§3.4). La PR qui livre le filigrane fait de `watermark`
> un champ **connu** (un objet, pas la chaîne du test) et met à jour ce test
> dans le même changement — conformément à CLAUDE.md, la spec et le code
> bougent ensemble, pas dans cet ADR de pré-décision.

### Épreuvage écran — surface moteur/API, vue seule

L'épreuvage est **une extension de la requête de preview** (`docs/engine-api.md`
§11), pas une révision ni un preset. Un paramètre d'épreuvage **optionnel**
s'ajoute à un appel de preview : un profil ICC de destination (octets ou
référence), une intention de rendu, et un drapeau optionnel d'alerte de gamut.
Cet ajout est **une option de vue sur un seul appel de preview** — **jamais
persisté**, jamais écrit à aucune révision ni preset, jamais dans
`settings_json`.

La transformation elle-même **réutilise la primitive d'ADR 0027**
(`leyline-color` : chargement de profil, construction de transform,
application) — aucune primitive nouvelle, exactement le partage qu'ADR 0027 a
anticipé entre épreuvage et export non-sRGB. La sortie du pipeline de rendu
(sRGB, `docs/adr/0015`) est **entièrement inchangée** : la transformation
d'épreuvage a lieu **strictement après** le rendu normal, pour l'affichage
seul. L'aperçu se rend d'abord en sRGB comme aujourd'hui, puis, quand un
paramètre d'épreuvage est fourni, `leyline-color` applique la transformation
sRGB → profil de destination (plus l'alerte de gamut si demandée) sur le seul
tampon d'affichage.

Esquisse conceptuelle du paramètre (forme exacte laissée à la PR, comme pour
tout ADR de cette série) :

```rust
struct SoftProof {
    profile: IccProfile,       // octets ICC ou référence vers un profil de destination
    intent: RenderingIntent,   // perceptuel / colorimétrique relatif / …
    gamut_warning: bool,       // surligner les couleurs hors gamut de destination
}
// param optionnel d'un appel preview — jamais sérialisé, jamais catalogué.
```

## Conséquences

* **Cet ADR clôt les deux tiers « épreuvage/filigrane » de l'item 7** qu'ADR
  0027 avait laissés ouverts. Le tiers restant — le module d'impression —
  demeure **genuinely non scopé**, suivi comme travail futur, **ni conçu ici
  ni stubé** avec des décisions de remplissage. C'est une déférence explicite,
  pas un oubli : l'impression aura son propre ADR quand le chantier sera
  planifié, dans le même esprit que `docs/v2-scope.md` §7 l'a coté L/XL et
  décrit comme « surtout UI + chemin de sortie dédié ».
* **Aucune process version, aucun étage pipeline** : le contrat de
  reproductibilité des révisions (`docs/pipeline.md` §5, « même révision →
  mêmes pixels ») n'est pas engagé, exactement comme ADR 0027 l'a établi — le
  filigrane vit à l'encodage d'export, l'épreuvage dans un tampon d'affichage,
  deux surfaces déjà hors du périmètre de ce contrat.
* **Le filigrane hérite gratuitement de la plomberie de presets d'export** :
  un champ dans `ExportSettings`, donc portable, stockable et rejouable comme
  n'importe quelle recette (`docs/catalog.md` §27), sans nouvelle table ni
  nouveau mécanisme.
* **L'épreuvage et l'export non-sRGB (ADR 0027) partagent une seule primitive
  ICC** dans `leyline-color` : construire l'un dérisque l'autre, comme ADR 0027
  l'avait prévu, même si `docs/v2-scope.md` les liste séparément.
* **La coupe du filigrane image/logo** laisse ouvert, pour un futur ADR, le
  vrai problème de référence de ressource (chemin relatif de bibliothèque vs.
  fichier absolu choisi par l'utilisateur) — sans bloquer le cas d'usage
  courant que le texte couvre déjà.

## Alternatives écartées

* **Supporter le filigrane image/logo dès la V2.** Écarté : il faudrait
  trancher où vit le fichier logo et comment il est référencé — un chemin
  relatif de bibliothèque (`docs/catalog.md` §2.3) alors que le logo n'est pas
  un asset photo, ou un chemin absolu que la règle de portabilité (§2.3)
  interdit justement de stocker. Une vraie question de conception que cet ADR
  refuse d'inventer sous pression ; le filigrane texte est entièrement
  autonome dans `settings_json` et couvre le cas nommé (nom/copyright/site).
  Le logo reviendra dans son propre changement, une fois le problème de
  référence tranché — pas déféré comme drapeau, coupé comme fonctionnalité.
* **Compositer le filigrane avant la transformation vers le profil de
  destination.** Écarté : le texte serait alors tracé en sRGB puis repassé
  par la transformation ICC photographique d'ADR 0027, conçue pour le contenu
  image, pas pour un décor. Ses couleurs (un blanc à 70 % d'opacité, une
  teinte de copyright) dériveraient avec le profil de destination. Le tracer
  **après** la conversion, directement en espace de destination, garantit
  qu'un filigrane blanc reste le blanc de destination — le décor hérite de
  l'espace de sortie, il ne le traverse pas.
* **Persister l'état d'épreuvage dans `settings_json` ou un preset** plutôt
  que de le garder un paramètre de requête vue-seule. Écarté : l'épreuvage
  est un **mode de vue**, pas une propriété de la révision (`docs/v2-scope.md`
  §7). L'écrire au catalogue contredirait le constat même du §7 (« ne modifie
  ni `settings_json` ni les pixels ») et introduirait un état qui n'affecte
  aucun pixel rendu ni exporté — un champ qui ment sur ce qu'est une révision.
  Le garder sur l'appel de preview le maintient exactement là où il agit :
  l'affichage, rien d'autre.
* **Scoper le module d'impression dans ce même ADR.** Écarté : l'impression
  est un sous-système mise en page + sortie (marges, planches, profil
  imprimante), coté L/XL et « surtout UI + chemin de sortie dédié » par
  `docs/v2-scope.md` §7 — une initiative franchement plus large que le
  filigrane et l'épreuvage, et d'une autre nature (UI et plomberie de sortie,
  pas architecture de pipeline de développement). La concevoir
  responsablement demande sa propre passe dédiée, le jour où ce chantier sera
  planifié. La stuber ici avec des décisions de remplissage (formats papier,
  modèle de marges, gestion des planches) serait inventer une architecture que
  personne n'a encore réellement cadrée — exactement ce que la maison évite
  (« No code before architecture »). L'impression garde donc son propre ADR à
  venir ; cet ADR se contente de le dire explicitement.
