# ADR 0008 — La version comme unité de bibliothèque

**Statut :** Accepté — 2026-07

## Contexte

Avec le modèle Git (ADR 0007), une copie virtuelle est une branche. Restait à décider où vivent note, label, pick, collections et mots-clés : sur le fichier (asset) ou sur la branche (version) ?

## Décision

Ligne de partage :

```text
Fait sur l'image      → asset      (mots-clés, EXIF, checksum)
Jugement sur un rendu → version    (note, label, pick, collections)
```

La grille énumère des versions. Noter une photo simple = noter sa version `Default`.

## Conséquences

* Chaque copie virtuelle se note, se labellise et se classe indépendamment (parité Lightroom).
* Les mots-clés restent partagés : « retrouve mes hérons » remonte toutes les versions, et l'export XMP reste attaché au fichier.
* La grille coûte une jointure 1:1 indexée (`develop_versions JOIN assets`) — mesuré comme négligeable à l'échelle cible.
* Extension réservée si le besoin apparaît : `version_keywords` additif (`catalog.md` §38).

## Alternatives écartées

* **Tout sur l'asset** : les copies virtuelles partagent la note — limitation réelle constatée dans les workflows de tri.
* **Tout sur la version, mots-clés compris** : duplication du tagging à chaque branche, divergences silencieuses, XMP ambigu.
* **Surcharges nullables (héritage asset → version)** : deux sources de vérité, COALESCE dans chaque requête — dette permanente.
