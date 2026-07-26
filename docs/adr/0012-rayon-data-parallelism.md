# ADR 0012 — Parallélisme de données avec Rayon dans le moteur

**Statut :** Accepté — 2026-07

## Contexte

La phase 7 vise des rendus interactifs. Les opérateurs de `process 1` sont
par-pixel ou par-ligne : ils se prêtent au parallélisme de données. Mais le
contrat de reproductibilité (`docs/pipeline.md` §5) exige un rendu strictement
déterministe : un même `settings_json` doit produire les mêmes pixels quel que
soit le nombre de threads. Le nombre de threads est précisément le genre de
variable que §5.1 refuse de laisser entrer dans le résultat — contrairement à
la plateforme et à la chaîne de compilation, que §5.2 place hors garantie.

## Décision

Les boucles chaudes du moteur utilisent Rayon (`par_chunks_mut` par lignes).
Règle absolue : la parallélisation ne change jamais la formule scalaire ni
l'ordre des opérations *pour un échantillon donné*. Chaque ligne est calculée
indépendamment, aucune réduction flottante inter-threads n'est autorisée —
le résultat reste bit-pour-bit identique à l'exécution mono-thread, ce que
les tests de rendu existants vérifient — et, depuis ADR 0042 §7, les rendus
de référence de `stages/golden.rs`, dont aucune empreinte ne dépend du
nombre de threads.

Un process version gelé peut donc être parallélisé après coup : ce n'est pas
un changement de rendu au sens de `docs/pipeline.md` §3.3.

## Conséquences

* À 3 MP, l'édition complète passe de 709 ms à 167 ms (−76 %) sur la machine
  de référence ; les benchmarks `cargo bench -p leyline-engine` suivent ces
  chiffres.
* Rayon possède son pool global : le moteur reste sans runtime asynchrone
  (cohérent avec ADR 0011 — threads natifs).
* Toute optimisation future qui modifierait l'ordre des additions flottantes
  (SIMD horizontal, réductions parallèles) devra passer par une nouvelle
  version de process.

## Alternatives écartées

* **Threads manuels + canaux** : réinventer un work-stealing éprouvé, sans
  gain.
* **GPU (wgpu)** : gains supérieurs mais déterminisme inter-GPU non garanti ;
  reporté à une exploration ultérieure de phase 7, derrière une nouvelle
  version de process si nécessaire.
