# ADR 0068 — Un lot d'export traite plusieurs photos à la fois

**Statut :** Accepté — 2026-08

## Contexte

[Le relevé B2](../competitive-plan.md) a chronométré le chemin d'export photo
par photo, et en a tiré une piste : les encodeurs rapides (JPEG, PNG, TIFF,
WebP) sont mono-thread, donc *« recouvrir l'encodage du fichier n avec le rendu
du n+1 »* récupérerait « de l'ordre de 10 à 15 % » d'un lot.

Mesurer le lot lui-même donne un tout autre ordre de grandeur. 12 fichiers
30 Mpx d'un même dossier du corpus, révision neutre, sortie JPEG q90,
i9-9900K 16 threads :

```
lot séquentiel (aujourd'hui) : 30,02 s — 77,5 s CPU, soit 280 % de 1 600 %
```

**Le lot n'utilise pas 3 cœurs sur 16.** Le problème n'est donc pas que
l'encodage laisse des cœurs libres pendant l'encodage : c'est que le pipeline
d'**une seule** photo, décodage LibRaw compris, ne sait pas remplir la machine —
ni pendant le décodage, ni pendant l'encodage, et à peine pendant le rendu.
Recouvrir deux étages d'un même fichier ne s'attaque qu'à une fraction de ce
qui reste vide.

Le même lot, en traitant plusieurs photos à la fois, mesure ce qu'il y a
réellement à prendre — chiffres de l'implémentation décidée ici :

| Photos en vol | Temps | Accélération | CPU | Pic RSS |
|---|---|---|---|---|
| 1 — l'ancien comportement | 30,05 s | 1,00× | 289 % | 845 Mo |
| 2 | 16,84 s | 1,78× | 545 % | 1,45 Go |
| **4 — le défaut retenu** | **10,68 s** | **2,81×** | 916 % | 2,38 Go |
| 6 | 8,81 s | 3,41× | 1 136 % | 3,27 Go |
| 8 | 9,05 s | 3,32× | 1 134 % | 4,69 Go |

Le gain plafonne vers 6, et **régresse** à 8 : au-delà, le pipeline d'une photo
ne sait plus quoi faire des cœurs supplémentaires, et la mémoire commence à
coûter. **Il ne s'agit donc pas de 10 à 15 %, mais d'un facteur 2,8 à 3,4.**

### Ce que coûte une photo en vol

C'est la contrainte qui décide de tout le reste. La colonne « Pic RSS » ci-dessus
croît linéairement : ~600 Mo par photo supplémentaire à 30 Mpx, et une révision
chargée (exposition, clarté, débruitage) mesure 785 Mo là où une révision neutre
en prend 665. À 45 Mpx cela dépasse le gigaoctet par photo. **Le parallélisme se
paie en mémoire, linéairement**, et une machine qui se met à paginer perdra bien
plus que le facteur 2,8 gagné.

## Décision

**`Library::export` traite plusieurs photos en parallèle, avec un nombre de
photos en vol borné et réglable.**

### 1. Le degré : 4 par défaut

`min(4, available_parallelism())`.

4 prend **2,81× sur les 3,41× disponibles** — 82 % du gain — pour 2,4 Go de
pointe à 30 Mpx. 6 prendrait 3,41× mais pour 3,3 Go, et 8 fait déjà *moins bien*
que 6 pour 4,7 Go. Sur une machine qui édite des photos, la mémoire n'est pas
libre : le défaut prend la part franche du gain et laisse la machine
utilisable.

Réglable par `ExportRequest.concurrency: Option<usize>`, parce que le bon
nombre dépend de la machine et de la taille des fichiers — deux choses que le
moteur ne peut pas deviner. Ce champ appartient à la **requête**, pas à
`ExportSettings` : c'est une propriété de l'exécution, pas de la recette, et il
n'a donc rien à faire dans le `settings_json` d'un preset ([ADR 0067](0067-avif-encode-speed.md)
§1 a tranché l'inverse pour la vitesse AVIF, qui est bien une propriété du
fichier produit).

### 2. Les noms de sortie sont réservés d'avance, en ordre de requête

Le lot garantit aujourd'hui deux choses qu'une exécution concurrente casserait
en silence :

* un export **n'écrase jamais** un fichier existant ;
* deux versions d'un même asset dans un lot se disputent le même nom, et c'est
  **la seconde** qui échoue.

Avec des rendus concurrents, le `exists()` de l'une et l'écriture de l'autre
s'entrelacent : deux photos peuvent passer le test puis s'écraser. Ce n'est pas
un détail d'ordonnancement, c'est la perte d'un fichier.

D'où une **passe de planification préalable**, séquentielle et en ordre de
requête, qui lit le catalogue une fois pour tout le lot et attribue son nom de
sortie à chaque version. Un nom déjà réservé par une version *antérieure* fait
échouer la version postérieure immédiatement, avant tout décodage. Le
comportement documenté est donc conservé **et rendu déterministe**, là où il
dépendait auparavant de l'ordre d'arrivée sur le disque.

Cette passe a un second effet : le verrou catalogue est pris **une fois** pour
tout le lot au lieu d'une fois par photo, et plus du tout pendant les rendus.

### 3. Les threads du lot ne sont pas des tâches rayon

`docs/engine-api.md` §3.1 a déjà tranché la question pour le pool de jobs : le
vol de tâches rayon peut recruter le thread d'un travail pour en exécuter un
autre, et si le premier tient le mutex catalogue — non réentrant — c'est un
interblocage. La même règle s'applique ici. Les photos en vol sont portées par
des threads ordinaires (`std::thread::scope`) ; **le rendu continue d'utiliser
le pool rayon global à l'intérieur de chaque photo**, ce qui est précisément la
raison pour laquelle 4 photos suffisent à remplir 16 cœurs.

La discipline de verrou d'[ADR 0024](0024-catalog-lock-narrowing-export.md)
n'est pas seulement conservée, elle devient nécessaire : aucun worker ne tient
le catalogue pendant qu'il rend.

### 4. Ce que la concurrence ne change pas

* **Les pixels.** Chaque photo est rendue indépendamment ; rien n'est partagé
  entre deux rendus. `pipeline.md` §5.1 est intact, et un test le vérifie plutôt
  que de l'affirmer : le même lot, en séquentiel puis en concurrent, produit des
  fichiers **identiques octet pour octet**. Ce test n'était pas écrivable avant
  le correctif de l'horodatage ICC — le profil embarqué portait l'heure, donc
  deux exports identiques différaient déjà d'un octet sans que personne ne le
  sache.
* **L'ordre du rapport.** `ExportReport.exported` et `.failed` restent en ordre
  de requête, quel que soit l'ordre d'achèvement.
* **La progression.** `progress(done, total)` compte les fichiers écrits ; seul
  l'ordre dans lequel ils s'achèvent change, ce qu'aucun client n'observe.
* **La tolérance aux pannes.** Une photo qui échoue ne fait pas tomber le lot,
  comme avant.
* **`export_batch`**, la fonction libre, reste séquentielle : elle prend un
  `&mut Catalog`, donc un accès exclusif, et n'a par construction pas de quoi
  paralléliser. C'est le chemin que les tests du moteur utilisent directement ;
  les trois clients passent tous par `Library::export`.

## Conséquences

* Un lot d'export va **~2,8× plus vite** sans qu'aucun réglage ne change, et
  ~3,4× pour qui monte le degré à 6.
* `--concurrency <n>` dans la CLI ; Studio et le SDK prennent le défaut, Studio
  n'ayant aucune raison d'en savoir plus que le moteur sur la machine.
* La mémoire de pointe d'un export est multipliée par le degré. C'est la raison
  du défaut prudent, et la première chose à regarder si un lot devient lent au
  lieu d'aller plus vite.
* Le journal `export_history` s'écrit désormais dans l'ordre d'achèvement, plus
  dans l'ordre de requête. Aucune lecture n'en dépend : les lignes portent leur
  horodatage.
* La passe de planification tient tout le lot en mémoire (une `Settings` parsée
  par version). Négligeable devant une seule image, mais c'est ce qui interdit
  d'appliquer la même recette à un lot de plusieurs centaines de milliers de
  photos sans le découper.

## Alternatives écartées

* **Recouvrir l'encodage du fichier *n* avec le rendu du *n+1***, la piste
  d'origine de B2. Elle vise le bon symptôme mais trop petit : la mesure montre
  qu'il manque 13 cœurs sur 16, pas seulement ceux d'un encodage mono-thread.
  Elle est en outre plus intrusive — il faut scinder `render_export` en deux
  moitiés et les rendre communicantes — pour une fraction du gain que donne le
  traitement de plusieurs photos.
* **Paralléliser avec `rayon::par_iter` sur les versions.** Le tour de main
  évident, et l'interblocage décrit au §3 : c'est la raison pour laquelle le
  pool de jobs n'en est pas un non plus.
* **Un degré égal à `available_parallelism()`.** Sur la machine de référence,
  8 photos en vol vont déjà *moins vite* que 6 pour 1,4 Go de plus — et sur une
  machine à 32 threads, de quoi paginer avec des fichiers 45 Mpx.
* **Adapter le degré à la taille des images.** Une heuristique non écrite, du
  type de celles qu'[ADR 0067](0067-avif-encode-speed.md) a écartées : le champ
  du §1 rend la décision à qui connaît sa machine.
