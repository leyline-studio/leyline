# ADR 0060 — Retirer une photo du catalogue, et la supprimer du disque

**Statut :** Accepté — 2026-08

## Contexte

Leyline sait importer une photo. Il ne sait pas l'enlever. Cette absence est
totale : ni `leyline-catalog`, ni `Library`, ni la CLI, ni Studio n'exposent
quoi que ce soit qui retire un asset. Le catalogue n'accumule que des ajouts,
et la seule façon de revenir en arrière est de jeter la bibliothèque entière.

Ce n'est pas seulement un confort manquant. C'est ce qui rend **inapplicable un
remède déjà décidé** : [ADR 0043](0043-collapse-prerelease-render-history.md) §5
a effondré l'historique de rendu d'avant publication sans écrire de migration,
en prescrivant noir sur blanc que « **les bibliothèques de développement se
réimportent** ». Or la réimportation est bloquée par la détection de doublon de
`crates/leyline-engine/src/import.rs:126` : un fichier dont le BLAKE3 est déjà
connu est écarté, sans recours. Une révision devenue illisible ne se répare
donc pas, et l'asset qui la porte ne se remplace pas.

La décision et l'implémentation se contredisent. C'est un bug, pas un manque.

Le schéma, lui, était prêt depuis le début. Les **sept** tables qui référencent
`assets` — `develop_revisions`, `develop_versions`, `develop_current`,
`previews`, `asset_keywords`, `export_history` et l'index de recherche — sont
**toutes déclarées `ON DELETE CASCADE`** (`docs/catalog.md`), et
`PRAGMA foreign_keys = ON` est posé à chaque ouverture
(`crates/leyline-catalog/src/connection.rs:15`). Un `DELETE FROM assets`
nettoie déjà tout le graphe. Seule l'API n'a jamais été écrite.

## Décision

**Deux opérations distinctes, jamais un seul geste ambigu.** C'est la
distinction que fait Lightroom, et elle n'est pas cosmétique : confondre « je ne
veux plus voir cette photo dans mon catalogue » et « détruis ce fichier » est la
faute qu'un logiciel de photo ne peut pas se permettre une seule fois.

### 1. `remove` — retirer du catalogue

Retire les assets du catalogue. **Le fichier n'est pas touché.** Le
développement, les mots-clés, les collections, l'historique d'export et les
previews de ces assets disparaissent avec eux — c'est ce que la cascade fait
déjà.

C'est l'opération par défaut, celle qui débloque ADR 0043 §5 : une fois l'asset
retiré, son empreinte n'est plus connue, et le fichier se réimporte
normalement.

### 2. `delete` — supprimer du disque

Fait tout ce que fait `remove`, **puis** envoie le fichier source à la
corbeille du système.

* **À la corbeille, jamais un effacement définitif.** Une suppression
  irréversible de RAW depuis un logiciel de photo est exactement le geste où
  « vos données vous appartiennent » exige un filet. Si la plateforme n'offre
  pas de corbeille pour ce fichier, l'opération **échoue franchement** au lieu
  de se rabattre en silence sur un `unlink` : un repli silencieux ferait de la
  garantie une loterie.
* **Le sidecar XMP part avec.** `xmp::sidecar_path` donne son chemin
  ([ADR 0047](0047-xmp-sidecar-read.md)) ; laisser un `.xmp` orphelin à côté
  d'un RAW disparu n'a aucun sens.
* **Rien hors de la bibliothèque n'est jamais touché.** L'import en mode
  référence exige déjà que le fichier vive sous la racine de la bibliothèque —
  le catalogue ne stocke que des chemins relatifs à cette racine
  ([ADR 0010](0010-relative-paths.md)). Il n'existe donc aucun cas où
  `delete` sortirait de l'arborescence de la bibliothèque, et le validateur de
  chemin relatif reste le garde à l'entrée.

### 3. Ce que la suppression ne remet pas en cause

`docs/catalog.md` pose que « les révisions ne sont jamais supprimées ». Cet
invariant **porte sur l'historique de développement d'un asset vivant** : on ne
réécrit pas le passé d'une photo qu'on garde, c'est ce qui rend l'undo et les
snapshots fiables. Retirer un asset n'est pas une réécriture d'historique,
c'est la sortie de la photo du catalogue. Les deux règles ne se rencontrent
pas, et `docs/catalog.md` est précisé dans le même changement pour que la
lecture ne soit pas ambiguë.

Le seul amendement réel est le [contrat de non-destructivité](../pipeline.md)
§6, qui garantit que Leyline ne modifie jamais un fichier source. `delete` ne
le modifie pas non plus — il le retire, sur demande explicite de l'utilisateur,
vers un endroit d'où il se récupère. La garantie visait les écritures
silencieuses du logiciel sur les originaux ; elle n'a jamais visé à empêcher
l'utilisateur de jeter sa propre photo.

### 4. Surface

**Catalogue** — `Catalog::delete_assets(&[AssetId]) -> Result<Vec<String>>`,
en une transaction, rendant les chemins relatifs des previews en cache. Ce
retour est la seule subtilité de l'opération : le cache de previews vit
**hors** de SQLite, la cascade efface ses lignes mais laisserait ses PNG
orphelins sur le disque. C'est exactement la forme, déjà éprouvée, de
`remove_revision_previews`.

**Moteur** — deux méthodes, dont les noms disent lequel touche au disque :

```rust
// Retire du catalogue ; les fichiers ne sont pas touchés.
pub fn remove_assets(&self, assets: &[AssetId]) -> Result<RemovalReport>;
// Retire du catalogue, puis envoie sources et sidecars à la corbeille.
pub fn delete_assets(&self, assets: &[AssetId]) -> Result<RemovalReport>;
```

`RemovalReport` porte ce qui a réellement disparu et ce qui a résisté — un
fichier déjà absent n'est pas une erreur (le catalogue doit pouvoir se nettoyer
d'un fichier que l'utilisateur a déplacé hors de Leyline), un fichier
verrouillé en est une, et l'appelant doit pouvoir dire lequel.

Le moteur émet un événement **`Event::AssetsRemoved { asset_ids }`**, à côté de
`AssetsAdded` et `AssetsChanged` (`docs/engine-api.md` §3.2). Sans lui, toute
vue ouverte sur ces assets — grille, filmstrip, carte — continuerait d'afficher
des lignes mortes.

**CLI** — `leyline remove <ids…>` et `leyline delete <ids…>`, ce dernier exigeant
`--yes` pour s'exécuter sans terminal interactif.

**Studio** — deux entrées distinctes dans le menu Photo et dans le menu
contextuel de la grille, opérant sur **toute la sélection**. Les deux demandent
confirmation, en nommant le nombre de photos ; celle qui touche au disque dit
explicitement que les fichiers partent à la corbeille. Studio n'a aujourd'hui
aucun dialogue de confirmation générique — il en gagne un, réutilisable.

## Conséquences

* ADR 0043 §5 redevient applicable : une bibliothèque de développement héritée
  se répare en retirant les assets et en réimportant le dossier.
* **`import.rs` n'est pas touché.** Ajouter une option « forcer la
  réimportation » traiterait le symptôme en créant des doublons en catalogue,
  là où libérer l'empreinte est la cause propre. La détection de doublon reste
  inconditionnelle.
* Nouvelle dépendance `trash`, pour la corbeille des trois systèmes. Elle est
  vérifiée sur la compilation croisée Windows (`packaging/windows/build-nsis.sh`)
  dans le même changement — une dépendance qui casserait ce build serait à
  rejeter, quel que soit son intérêt.
* `Event` gagne une variante : tout `match` exhaustif chez un consommateur du
  SDK doit la traiter. Le projet n'étant pas publié, cela n'engage personne.
* Aucune migration de schéma, aucune version d'étage, aucun pixel changé. Le
  contrat de rendu (`docs/pipeline.md` §5) n'est pas concerné.

## Alternatives écartées

* **Une seule opération avec un drapeau `delete_files: bool`.** Une signature
  où la destruction d'un original tient dans un booléen est une faute qui
  attend son appelant. Deux noms distincts rendent l'erreur difficile à écrire
  et évidente à relire.
* **Effacement définitif plutôt que corbeille.** Plus simple, sans dépendance,
  et sans aucun recours pour l'utilisateur qui se trompe de sélection. Le coût
  d'une dépendance est très inférieur à celui d'un RAW perdu.
* **Une corbeille interne à la bibliothèque** (déplacer dans un `Trash/`
  maison) : évite la dépendance, mais invente un concept que l'utilisateur doit
  apprendre, occupe son disque sans qu'il le sache, et duplique mal ce que les
  trois systèmes font déjà bien.
* **Une option « forcer » à l'import** : voir Conséquences. Traite le symptôme,
  et laisse deux lignes pour un même fichier.
* **Marquer l'asset comme retiré sans l'effacer** (suppression logique) :
  garderait l'empreinte connue, donc laisserait la réimportation bloquée — le
  problème même qu'il s'agit de résoudre — et alourdirait toutes les requêtes
  d'un filtre permanent.
