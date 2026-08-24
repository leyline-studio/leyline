# ADR 0079 — Le RAW et son JPEG sont une seule photo

**Statut :** Accepté — 2026-08

## Contexte

Le 2026-08-24, une bibliothèque de test a été montée sur un corpus réel : trois
dossiers d'un Canon 5D Mark IV, **39 prises de vue**, sorties du boîtier en
RAW+JPEG. La grille en a affiché **78**. Chaque photo, deux fois, côte à côte.

Ce n'est pas une régression : c'est ce que le dépôt a toujours fait, et il ne
l'a jamais décidé. La recherche est facile à refaire — aucun ADR ne parle
d'appairage, de pile ou de fichier compagnon ; `catalog.md` §38 range HDR, focus
stacking et panorama parmi les extensions futures **sans mentionner celle-ci** ;
et `specification.md` §4 ne l'inscrit dans aucune exclusion volontaire. C'est
donc un trou, pas une abstention.

Il mérite d'être refermé avant les autres pour une raison simple : le RAW+JPEG
est un **mode de boîtier**, pas une pratique marginale. Un utilisateur qui
l'active voit sa bibliothèque doubler, et c'est la première chose qu'il voit.

### Ce que le corpus impose à la conception

Trois faits mesurés, qui décident chacun un point de la décision et qu'aucune
lecture du code n'aurait donnés :

* **Les deux fichiers ne sont pas forcément dans le même dossier.** Dans les
  dossiers `2026_08_10`, `_12` et `_14`, les JPEG sont à la racine et les CR2
  dans un sous-dossier `raw/` — donc dans **deux dossiers distincts du
  catalogue**. Dans `2026_08_16`, ils sont côte à côte. Le même appareil, la
  même semaine, deux dispositions. La proximité de dossier ne peut donc pas
  être le critère.
* **L'instant de capture est identique et les dimensions ne le sont pas.** Sur
  `5D4_2325`, les deux fichiers portent `capture_date = 1786731816000`, à la
  seconde près, tandis que le RAW mesure 6744×4502 contre 6720×4480 pour le
  JPEG — les pixels de bordure du capteur. L'instant appaire ; la géométrie,
  non.
* **Le JPEG est importé avant le RAW.** `collect()` énumère par chemin trié :
  `Photos/5D4_2326.JPG` précède `Photos/raw/5D4_2326.CR2`. L'appairage ne peut
  donc pas se contenter de chercher un RAW déjà présent quand un JPEG arrive —
  dans la disposition la plus courante, c'est l'inverse qui se produit.

## Décision

### 1. La paire est un fait du catalogue, pas un artifice d'affichage

`assets` gagne une colonne :

```sql
companion_of INTEGER NULL REFERENCES assets(id) ON DELETE CASCADE
```

`NULL` — le cas de très loin le plus fréquent — signifie « cette photo est
elle-même ». Une valeur désigne le **maître** dont ce fichier est le compagnon.

Une colonne nullable plutôt qu'une table `pairs` : la relation est
**asymétrique** (il y a un maître et un suivant, ce qu'une table de paires
devrait réencoder par une colonne de rôle) et elle se lit à chaque affichage de
grille, où une jointure de plus se paierait sur toutes les requêtes pour servir
un cas qui n'existe pas dans la majorité des bibliothèques. Le `ON DELETE
CASCADE` dit la seule chose qui compte : un compagnon ne survit pas à son
maître.

Migration `SCHEMA_V3` — la colonne s'ajoute, elle n'appaire rien (voir §7).

### 2. Le critère : même radical, même instant, même boîtier

Deux fichiers forment une paire si **les trois** tiennent :

1. même **radical** de nom de fichier, casse ignorée (`5D4_2326`) ;
2. même **instant de capture** (`capture_date`, à la seconde) ;
3. même **boîtier** (`metadata.camera`).

**À toute profondeur de la bibliothèque**, et non dans le seul dossier commun —
c'est le premier fait du contexte qui l'exige, et le cas qui a ouvert cet ADR
serait précisément celui qu'une règle par dossier laisserait en double.

Le nom seul ne suffit pas : deux boîtiers remettent leur compteur à zéro, et
`5D4_2326` peut exister deux fois dans une bibliothèque de dix ans. L'instant
seul ne suffit pas non plus, et le corpus le prouve : il contient des rafales à
1/500 s, où deux vues distinctes tombent dans la même seconde. C'est la
conjonction qui rend le faux positif improbable, et chacun des trois critères
pris isolément qui le rend courant.

Un fichier sans instant de capture ne s'appaire pas. Une métadonnée absente
n'est jamais devinée.

### 3. Le maître est le RAW, et une paire fait exactement deux

Le maître est le fichier dont Leyline sait développer les pixels ; le compagnon
est le rendu que le boîtier a écrit. Une paire est donc **un RAW et un
non-RAW** :

* deux RAW (un CR2 et un DNG du même cliché) ne s'appairent pas — aucun des
  deux n'est le rendu de l'autre, et lequel serait le maître n'a pas de
  réponse ;
* deux non-RAW ne s'appairent pas — il n'y a pas de maître à désigner ;
* un compagnon ne peut pas être maître à son tour. Pas de chaînes : un
  `companion_of` pointe toujours vers une ligne dont le `companion_of` est
  `NULL`, et l'appairage refuse tout candidat déjà engagé d'un côté ou de
  l'autre.

Un troisième fichier du même cliché (un HEIF à côté du CR2 et du JPEG)
s'attache au même maître : la colonne l'autorise sans rien changer.

### 4. L'appairage a lieu à l'import, dans les deux sens

Chaque fichier importé cherche son conjoint **parmi ce que la bibliothèque
contient déjà**, dans les deux sens :

* un non-RAW qui trouve un RAW correspondant devient son compagnon ;
* un RAW qui trouve un non-RAW **déjà importé et non appairé** l'adopte.

Le second sens n'est pas une symétrie de confort : c'est le cas normal, établi
par le troisième fait du contexte. Ne livrer que le premier laisserait
précisément le corpus qui a motivé cet ADR intégralement en double.

L'appairage est une **option d'import** (`ImportOptions::pair_companions`,
vraie par défaut ; `--no-pair` dans la CLI), au même rang que `--reference` et
`--flat`. Voir §8 pour la raison pour laquelle ce n'est pas une préférence.

### 5. La grille montre les maîtres — une clause, à un seul endroit

`grid::build` ajoute `AND a.companion_of IS NULL` à sa clause `WHERE`, pour les
deux formes de requête (collection manuelle comprise). Tout ce qui compte,
filtre, trie ou pagine passe par là : le compte, les filtres de prise de vue
d'[ADR 0064](0064-metadata-filters.md), la recherche plein texte et les
collections intelligentes suivent sans qu'aucun d'eux ait à connaître la
notion de paire.

C'est l'argument décisif contre un regroupement fait à l'affichage : là où une
clause SQL rend le catalogue et l'interface d'accord par construction, un
regroupement dans Studio les ferait diverger — la grille montrerait 39 photos
pendant que `leyline ls`, le compteur et les collections en verraient 78.

### 6. Rien n'est caché en silence

Un compagnon n'est pas supprimé, pas déplacé, pas modifié. Il garde sa ligne,
sa version, ses révisions et son classement — il quitte la grille, c'est tout.
Quatre conséquences, toutes délibérées :

* **Il se voit.** `AssetDetails` gagne `companion: Option<AssetId>` et
  `companion_of: Option<AssetId>` ; le panneau de détails de Studio nomme le
  fichier joint, et la vignette porte un badge `RAW+J`.
* **Il se supprime avec son maître.** `remove_assets` et `delete_assets`
  étendent leur liste aux compagnons avant d'agir — sans quoi le `ON DELETE
  CASCADE` effacerait la ligne du JPEG en laissant son fichier sur le disque,
  et le compte rendu de suppression mentirait.
* **Il ne s'exporte pas tout seul.** Un export porte sur des versions, et
  celles d'un compagnon ne sont plus atteignables depuis la grille. Le JPEG du
  boîtier reste un fichier, à l'endroit exact où il a toujours été.
* **Il se détache.** `leyline unpair <library> <asset-id>…` remet
  `companion_of` à `NULL` ; la photo réapparaît dans la grille avec ce qu'elle
  avait. L'appairage est réversible parce qu'il ne détruit rien.

### 7. Une bibliothèque existante ne se réorganise pas toute seule

La migration ajoute la colonne et **n'appaire rien**. Une migration qui
appairerait ferait disparaître la moitié des vignettes d'une bibliothèque au
premier lancement suivant une mise à jour — le pire moment et la pire manière
d'apprendre une fonctionnalité.

L'appairage rétroactif est une commande explicite : `leyline pair <library>`,
qui dit ce qu'elle a fait, et **Bibliothèque ▸ Appairer RAW+JPEG…** dans
Studio, qui annonce le nombre de paires avant d'agir. Même critère qu'à
l'import, même refus des candidats déjà engagés.

### 8. Ce n'est pas une préférence

La règle d'admission d'[ADR 0078](0078-preferences-panel.md) §1 tranche seule :
un réglage entre dans les Préférences s'il porte sur **l'installation**. Celui-ci
porte sur un import, dans une bibliothèque — il échoue à la première condition,
et son endroit naturel est le dialogue d'import, où il se trouve en même temps
que la case « référencer sans copier ».

### 9. Aucun pixel ne bouge

Aucune version d'étage, aucune entrée de révision, aucun rendu. `pipeline.md`
§5.1 n'est pas concerné : l'appairage décide de ce que la grille montre, jamais
de ce que le moteur calcule.

## Conséquences

* Une bibliothèque RAW+JPEG affiche le nombre de **prises de vue**, et non le
  nombre de fichiers. C'est le but ; c'est aussi un changement visible pour
  quiconque comptait sur l'ancien comptage.
* Le classement, les mots-clés et les collections d'un compagnon deviennent
  inatteignables tant qu'il est appairé. Ils ne sont pas perdus — `unpair` les
  rend — mais ils cessent d'être modifiables, ce qui est cohérent avec « une
  seule photo » et n'est écrit nulle part ailleurs.
* Le schéma du catalogue passe en **v3**. `Catalog::open` refuse déjà un
  catalogue plus récent que la version supportée, et [ADR 0077](0077-application-updates.md)
  §4 copie `catalog.db` dans `Backups/` avant toute migration : les deux
  garde-fous jouent ici sans rien ajouter.
* `import` fait désormais une lecture de plus par fichier (la recherche du
  conjoint), sur un index. Le coût est mesuré à l'implémentation ; s'il n'est
  pas négligeable devant la copie et le calcul de l'empreinte, c'est
  l'implémentation qu'il faut revoir, pas la décision.

## Alternatives écartées

**Ne pas importer le JPEG quand un RAW du même nom existe.** L'option la plus
courte — zéro changement de schéma, aucune migration. Écartée parce qu'elle
répond à la mauvaise question : le photographe ne veut pas que son JPEG
disparaisse, il veut qu'il cesse d'être une deuxième photo. Un JPEG hors
catalogue n'est plus visible, plus exportable, plus classable, et le retrouver
demande un second import à part. C'est une perte d'information déguisée en
simplification.

**Regrouper à l'affichage seulement.** Le catalogue garde deux photos, Studio
en montre une. Écartée par §5 : elle fait mentir le catalogue à l'interface, et
la divergence se paie sur tout ce qui compte ou filtre. Elle serait aussi à
refaire dans chaque client — Studio, la CLI et le SDK ont chacun leur listage —
là où une clause dans `grid::build` les sert tous.

**Une table de piles générale (`stacks`), dont RAW+JPEG serait un cas.** C'est
la généralisation tentante : rafales, bracketing HDR, panoramas et RAW+JPEG
sont tous « plusieurs fichiers, une photo ». Écartée parce qu'elle décide en
même temps quatre problèmes dont un seul est posé, et parce que les trois
autres n'ont pas la propriété qui rend celui-ci facile : une paire RAW+JPEG a
un maître **évident**, désigné par le type de fichier, sans intervention de
l'utilisateur. Une pile de rafale n'en a pas. Le jour où les piles arrivent,
elles trouveront `companion_of` en place et décideront de ce qu'elles en font ;
l'inverse — livrer un modèle général pour servir le seul cas trivial — coûterait
plus cher et déciderait moins bien.

**Appairer sur l'instant de capture seul.** Résisterait au renommage d'un des
deux fichiers, ce que la conjonction ne fait pas. Écartée sur une mesure : le
corpus contient des rafales à 1/500 s, où deux clichés distincts partagent la
seconde. Résister au renommage est un cas rare ; appairer deux vues d'une
rafale est un cas courant.

**Appairer dans le même dossier seulement.** Plus prudent, et suffisant pour un
boîtier qui écrit les deux fichiers côte à côte. Écartée sur le premier fait du
contexte : la moitié du corpus de test range les RAW dans un sous-dossier, et
c'est exactement le cas qui a fait ouvrir cet ADR.

## Ce que cet ADR ne fait pas

* **Les piles au sens général** — rafales, bracketing, panorama — restent hors
  périmètre, et `catalog.md` §38 continue de les annoncer comme extensions
  futures.
* **Aucun réglage ne circule entre le RAW et son compagnon.** Développer le RAW
  ne touche pas le JPEG, et réciproquement. Ce serait une décision distincte,
  et elle demanderait de dire ce que devient un réglage qu'un JPEG ne peut pas
  porter.
* **Le JPEG ne devient pas un aperçu du RAW.** Le boîtier l'a rendu avec son
  propre profil ; l'utiliser comme aperçu ferait mentir la vignette sur ce que
  Leyline rendra. Le cache d'aperçus d'[ADR 0075](0075-preview-cache-retention.md)
  reste seul responsable de ce que la grille montre.
