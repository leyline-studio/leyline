# ADR 0004 — LibRaw (branche LGPL) pour le décodage RAW

**Statut :** Accepté — 2026-07

## Contexte

Le décodage RAW exige la couverture de centaines de formats propriétaires (CR3, NEF, ARW, RAF...), en évolution constante à chaque nouveau boîtier.

## Décision

`leyline-raw` s'appuie sur LibRaw, utilisé exclusivement sous sa branche **LGPL-2.1** (sa branche CDDL est incompatible avec la GPL-3.0 du projet).

Le décodage est isolé derrière l'API de `leyline-raw` : aucun autre crate ne voit LibRaw.

## Conséquences

* Couverture de formats immédiate et maintenue par un projet établi.
* FFI C confinée à un seul crate.
* Pour une future version propriétaire : linkage dynamique requis (obligation de substitution LGPL).
* Plan B documenté : `rawler` (décodeur RAW pur Rust, LGPL-2.1) peut remplacer LibRaw derrière la même API si le besoin apparaît.

## Alternatives écartées

* **rawler seul** : pur Rust, séduisant, mais couverture de formats et maturité inférieures à LibRaw aujourd'hui — conservé comme alternative de repli.
* **dcraw** : abandonné.
* **Décodeurs maison** : des années de travail pour rattraper l'existant.
