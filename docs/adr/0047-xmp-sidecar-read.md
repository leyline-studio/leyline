# ADR 0047 — Lecture des sidecars XMP : une amorce à l'import, jamais une synchronisation

**Statut :** Accepté — 2026-07

## Contexte

`crates/leyline-engine/src/xmp.rs` sait **écrire** un sidecar, et le dit sans
détour dès sa deuxième ligne :

> The catalog is always the source of truth; sidecars exist purely for
> interoperability with other tools and are **never read back**.

`docs/catalog.md` §29 dit la même chose (« Le moteur ne lit jamais les XMP comme
source principale »), et §2.4 pose que le catalogue est la seule source de
vérité. Ces énoncés visent une bonne cible — aucune synchronisation implicite,
aucun fichier externe capable de contredire le catalogue — mais ils ont une
conséquence que personne n'a choisie : **Leyline ne sait rien ingérer**.

Or c'est exactement l'étape 2 de toute migration depuis un autre logiciel, et
la seule que les guides recommandent : *exporter les XMP depuis Lightroom, les
faire lire par le nouvel outil, les mots-clés et les notes suivent les RAW*.
C'est aussi le seul canal qui existe : les réglages de développement ne sont
transférables par personne (algorithmes propriétaires), mais le **travail de
tri** — notes, libellés, mots-clés hiérarchiques, parfois des années de
classement — l'est, et il est bien plus long à refaire qu'une retouche.

Le résultat aujourd'hui : un photographe qui importe ses 40 000 RAW dans
Leyline arrive sur un catalogue vide de tout classement, alors que
l'information est posée à côté de chaque fichier, dans un format que Leyline
écrit déjà lui-même. Ce n'est pas un manque de fonctionnalité, c'est un
blocage d'adoption.

**Ce qui n'est pas en cause.** Le catalogue reste la source de vérité, et rien
ici n'ouvre un second canal d'autorité : c'est précisément ce que la politique
du §3 garantit, et c'est le cœur de la décision plutôt que sa réserve.

## Décision

### 1. Le sidecar amorce, il ne synchronise pas

Leyline lit un sidecar pour **remplir ce qui est vide**, jamais pour remplacer
ce qui existe. C'est ce qui distingue une amorce d'une synchronisation, et ce
qui laisse `docs/catalog.md` §2.4 exact : après lecture, la seule autorité
reste le catalogue, et rien ne relira ce fichier ensuite.

Il n'y a donc **pas** de symétrie avec les trois modes d'écriture de §29 : pas
de mode *Always* en lecture, aucune surveillance de fichier, aucune relecture
au changement, aucune réconciliation. Un sidecar est lu aux deux moments du §2,
et à aucun autre.

### 2. Deux moments de lecture, et deux seulement

* **À l'import**, automatiquement : si un `photo.xmp` se trouve à côté du
  fichier importé, il est appliqué à l'asset qui vient d'être créé. C'est le
  moment qui compte — l'asset est neuf, il n'a rien à écraser, et c'est le
  geste que fait le migrant sans avoir à connaître l'existence d'une commande.
  Aucun réglage : le sidecar est à côté du fichier, donc il concerne ce
  fichier. C'est le sidecar **du fichier source** qui est lu — là où l'autre
  logiciel l'a laissé — et il n'est pas recopié dans `Photos/` : une fois lu,
  il n'a plus d'autorité, donc plus de raison d'être conservé.
* **Explicitement**, sur un asset déjà importé : `Library::read_xmp(asset)`, le
  miroir exact de `Library::write_xmp(asset)`, pour la bibliothèque déjà
  constituée avant que les sidecars n'aient été exportés de l'autre logiciel.

### 2.1 « À côté du fichier » ne suffit pas à le nommer

Le point ci-dessus dit « si un `photo.xmp` se trouve à côté du fichier », et
c'est ce que le code faisait : remplacer l'extension par `.xmp`. Cette phrase
tranchait une question sans la poser — **les autres logiciels ne nomment pas
tous le sidecar pareil**, et deux conventions coexistent :

| Convention | Écrite par | Exemple pour `5D4_7998.CR2` |
|---|---|---|
| Nom complet + `.xmp` | darktable, exiftool | `5D4_7998.CR2.xmp` |
| Extension remplacée | Lightroom, Bridge, Leyline | `5D4_7998.xmp` |

Une seule des deux était lue. Le corpus de test réel a montré ce que cela
coûte : sur neuf sidecars, **le seul qui portait des mots-clés était celui de
darktable**, donc le seul invisible. Rien ne le signalait — c'est exactement le
mode de défaillance que le §6 refuse pour le parsing, arrivé un cran plus haut,
sur le nom de fichier.

**Décision : on écrit une convention, on en lit deux.**

* **Lecture** : le nom complet d'abord, l'extension remplacée ensuite. Le
  premier qui répond gagne. Cet ordre n'est pas arbitraire : `photo.CR2.xmp`
  désigne **une** photo, tandis que `photo.xmp` est partagé par tous les
  fichiers de même racine du dossier — un `IMG_2048.xmp` posé à côté d'un
  `IMG_2048.CR2`, d'un `IMG_2048.JPG` et d'un `IMG_2048.dng` (cas réel du
  corpus) ne dit pas duquel il parle. La forme non ambiguë passe donc devant,
  et c'est elle qui tranche quand l'autre logiciel en a écrit une.
* **Écriture** : inchangée, l'extension remplacée. C'est la convention
  d'Adobe, donc celle que cherche le logiciel visé par la migration inverse, et
  la lecture ci-dessus la reprend — l'aller-retour du §4 tient toujours.

L'ambiguïté de la forme partagée reste, elle est inhérente à la convention
d'Adobe et n'est pas à nous de la résoudre : quand plusieurs fichiers de même
racine cohabitent, ils reçoivent la même amorce. Sous la politique du §3, à
l'import, cela revient à donner à chaque copie d'une même prise le classement
que l'autre logiciel lui donnait — la conséquence acceptable de ce que le
sidecar ne dit pas.

### 3. La politique de conflit : remplir, jamais écraser

Le point qui demandait une décision plutôt que du code.

| Donnée | À la lecture |
|---|---|
| Note (`xmp:Rating`) | Appliquée **si** la version courante n'en a pas |
| Libellé (`xmp:Label`) | Appliqué **si** la version courante n'en a pas |
| Mots-clés | **Union** — les mots-clés du sidecar s'ajoutent, aucun n'est retiré |
| Artiste, copyright | Appliqués **si** le champ correspondant est vide |

Trois raisons, dans l'ordre où elles pèsent :

1. **À l'import, le cas intéressant, la question ne se pose pas** : tout est
   vide, donc « remplir » applique le sidecar en entier. La politique ne coûte
   rien là où elle sert le plus.
2. **Sur une relecture explicite, l'utilisateur ne peut pas défaire.** Un
   sidecar peut avoir des années et être plus pauvre que le travail fait depuis
   dans Leyline ; « le fichier gagne » détruirait ce travail silencieusement, en
   masse, sur une commande dont personne n'attend cela.
3. **C'est la seule politique qui ne perd aucune donnée**, dans les deux sens.
   L'union sur les mots-clés est du même esprit : `add_keyword` est déjà
   idempotent, donc relire deux fois le même sidecar est un no-op.

Un mode « le sidecar gagne » serait une décision *séparée*, avec sa propre
confirmation dans l'interface. Il n'est pas pris ici.

### 4. Le champ lu est exactement le champ écrit

Le noyau interopérable de `write_xmp_sidecar` : note, libellé, mots-clés plats
et hiérarchiques, artiste, copyright. Ni plus, ni moins — l'aller-retour
écriture → lecture est l'invariant, et un test le vérifie plutôt qu'un
commentaire l'affirme.

**Les mots-clés hiérarchiques font foi** quand les deux formes sont présentes :
Lightroom écrit `lr:hierarchicalSubject` (`Voyage|Japon|Kyoto`) *et* son
aplatissement `dc:subject`, et ne garder que le second perdrait l'arborescence
que le catalogue sait représenter. Les chemins absents de l'arbre de mots-clés
sont créés, niveau par niveau, sous les nœuds existants.

### 5. La lecture est tolérante ; l'import ne casse jamais

Un sidecar illisible, mal formé, ou qui ne contient aucun des champs ci-dessus
n'est **pas** une erreur d'import : le fichier photo s'importe normalement, sans
métadonnées venues du sidecar. C'est la règle que `import.rs` applique déjà à
ses propres étapes (« Per-file problems never abort the batch »), étendue d'un
cran : ici même le problème d'un fichier n'écarte pas ce fichier, il écarte son
sidecar.

Symétriquement, aucun sidecar n'est **écrit** par une lecture. Lire ne
déclenche pas la synchronisation §29.

### 6. Le parsing prend une dépendance : `roxmltree`

Contrairement à [ADR 0037](0037-dcp-parsing-dependency.md), qui a écrit un
lecteur maison minimal pour les tags DCP, ce lecteur-ci ne lit pas nos propres
fichiers : il lit ceux de Lightroom, de darktable, d'exiftool, de Capture One.
En pratique cela veut dire des préfixes de namespace variables (`xmp:` n'est
qu'une convention, seule l'URI compte), la même donnée tantôt en attribut
tantôt en élément (`xmp:Rating="4"` ou `<xmp:Rating>4</xmp:Rating>`), des
paquets XMP encadrés de `<?xpacket?>`, et de l'espace blanc partout. Un lecteur
maison n'y échoue pas franchement, il y échoue **silencieusement** — il rend un
catalogue sans mots-clés sans que rien ne le signale, ce qui est le pire mode
de défaillance possible pour une fonctionnalité de migration.

`roxmltree` est retenu : arbre en lecture seule, conscient des namespaces (donc
indifférent aux préfixes), Rust pur, sans dépendance transitive lourde et sans
brique C — contrairement à LibRaw, Lensfun ou LittleCMS, il n'ajoute rien à la
chaîne de build ni aux installeurs.

### 7. Hors périmètre, explicitement

* **Les réglages de développement du namespace `crs:`** (Adobe Camera Raw). Ils
  sont lisibles mais intraduisibles : appliquer une `crs:Exposure2012` à notre
  pipeline produirait une image *différente* de celle que l'utilisateur voyait
  dans Lightroom, tout en prétendant l'avoir reproduite. Aucun logiciel ne le
  fait, et le faire à moitié serait mentir sur le seul point où Leyline
  promet l'exactitude.
* **Le XMP embarqué** dans le RAW, le DNG ou le JPEG. Seuls les sidecars `.xmp`
  posés à côté du fichier sont lus ; l'embarqué demanderait d'écrire dans les
  fichiers d'origine pour rester cohérent, ce que le contrat de
  non-destructivité interdit (`docs/pipeline.md` §6).
* **Les collections, piles, instantanés.** Aucune représentation interopérable
  n'existe : chaque logiciel les stocke dans son propre catalogue.

## Conséquences

* **La migration depuis Lightroom devient réelle sans nouvelle interface.** Le
  chemin est celui que tout le monde documente déjà : *Métadonnées → Enregistrer
  les métadonnées dans les fichiers* chez Adobe, puis un import Leyline
  ordinaire. Studio, la CLI et le SDK en bénéficient tous les trois du seul fait
  qu'ils importent, sans qu'aucun n'ajoute un écran.
* **`docs/catalog.md` §29 gagne une section « Lecture »**, et sa phrase « le
  moteur ne lit jamais les XMP » est corrigée en ce qu'elle voulait dire : le
  moteur ne les lit jamais **comme autorité**. §2.4 est inchangé.
* **Une dépendance de plus dans `leyline-engine`**, la première de parsing XML.
  Elle est confinée au module `xmp` : rien d'autre dans le moteur ne voit
  `roxmltree`.
* **L'aller-retour devient un invariant testé.** Écrire un sidecar depuis un
  asset classé, relire dans un asset vierge, comparer : c'est le test qui
  garde le §4 honnête quand l'un des deux côtés bougera.
* **Le risque d'un import qui ralentit** est borné : deux `stat` par fichier
  importé quand aucun sidecar n'existe (le cas courant), un parse de quelques
  kilo-octets quand il en existe un.
* **La suppression vers la corbeille emporte les deux formes.** Laisser
  derrière soi le sidecar d'une photo qui n'existe plus, c'est le voir
  ressusciter au prochain import du même dossier.

## Alternatives écartées

* **« Le sidecar gagne »**, en lecture explicite. Écarté au §3 : perte de
  données silencieuse et irréversible, sur une commande d'apparence anodine.
  Reste possible plus tard, comme mode distinct et confirmé.
* **Un mode *Always* en lecture** (surveiller les sidecars et se
  resynchroniser). C'est un second canal d'autorité sur les mêmes champs, donc
  la fin de §2.4 — et une classe entière de conflits à arbitrer pour un besoin
  que personne n'a exprimé.
* **Lire aussi `crs:`** pour « au moins approcher » le rendu Lightroom. Écarté
  au §7 : une approximation présentée comme une reprise est pire qu'une absence
  annoncée.
* **Un lecteur XML maison**, dans l'esprit d'ADR 0037. Écarté au §6 : le mode de
  défaillance sur des fichiers étrangers est le silence, et c'est inacceptable
  pour de la migration. La comparaison avec le DCP ne tient pas — un conteneur
  TIFF que nous lisons pour nos propres fichiers de profil n'a pas la variabilité
  d'un paquet RDF écrit par quatre logiciels concurrents.
* **Un import de catalogue Lightroom** (lire le `.lrcat`, qui est du SQLite).
  Transférerait les collections, que les XMP ne portent pas. Écarté pour
  l'instant : format non documenté et versionné par Adobe, donc une surface de
  rétro-ingénierie permanente — à rouvrir sur demande réelle, avec son ADR.
