# Leyline

![Leyline Studio](../assets/leyline-studio.png)

> **Open Source RAW Development Platform**
>
> *Fast. Local. Open.*

Leyline est une plateforme de développement photographique RAW : un moteur, et les applications construites autour de lui.

* **Leyline** — la plateforme (le dépôt, les crates, la documentation).
* **Leyline Engine** — le moteur de rendu et de catalogue. Indépendant de toute interface.
* **Leyline Studio** — l'application de bureau. Un client du moteur parmi d'autres, au même titre que la CLI et le SDK.

L'objectif n'est pas de reproduire Lightroom fonctionnalité par fonctionnalité, mais de bâtir une plateforme sans abonnement, sans cloud et sans format propriétaire, dont l'architecture reste maintenable dans vingt ans. Le *pourquoi* est développé dans [`vision.md`](vision.md).

---

## État du projet — juillet 2026

**Le périmètre V1 est intégralement livré**, sur les trois clients (Studio, CLI, SDK) : import, catalogue, miniatures, EXIF, pipeline de développement non destructif complet, correction d'objectif, gestion des couleurs, presets, retraitement, export JPEG/TIFF/WebP/AVIF, impression, tethering, dossier surveillé, vue carte, installateurs et interface multilingue.

**Une bonne partie du périmètre V2 l'est aussi** : courbe tonale, suppression de tache, réglages locaux masqués, mélangeur TSL et color grading, clarté/texture/dehaze, profils caméra DCP (expérimental).

Ce qui reste ouvert :

| Sujet | État |
|---|---|
| Épreuvage écran et filigrane | Décidé ([ADR 0034](adr/0034-softproofing-watermark-print.md)), non implémenté |
| Pipeline d'étages versionnés | Migration en cours ([ADR 0042](adr/0042-versioned-stage-pipeline.md)) |
| Justesse colorimétrique des profils DCP | Non validée contre de vrais `.dcp` Adobe — fonctionnalité signalée comme expérimentale |

Le reste du travail est de la robustesse, de la performance et du polissage, non des fonctionnalités manquantes.

---

## Par où commencer

**Pour comprendre le projet** — lire dans cet ordre, environ trente minutes :

1. [`vision.md`](vision.md) — *pourquoi* le projet existe, pour qui, et ce qu'il refuse d'être.
2. [`architecture.md`](architecture.md) — *comment* il est découpé : les crates, leurs dépendances, les choix techniques et leurs raisons.
3. [`specification.md`](specification.md) — *quoi* : le périmètre livré, et ce qui est volontairement exclu.

**Pour contribuer au code** — enchaîner avec :

4. [`contributing.md`](contributing.md) — style, commits, licence, CLA.
5. [`pipeline.md`](pipeline.md) — le contrat de rendu. À lire **avant** de toucher au moteur : il définit ce qu'est une version d'étage et ce que le projet promet sur la reproductibilité d'un rendu (§5).
6. [`adr/`](adr/README.md) — 43 décisions structurantes, chacune avec son contexte, ses alternatives écartées et ses conséquences. C'est là que se trouve le *pourquoi* de presque tout ce qui surprend dans le code.

**Pour intégrer le moteur** — [`engine-api.md`](engine-api.md), puis le crate `leyline-sdk`, qui est la surface publique stable.

---

## Carte de la documentation

Les documents **de lecture** se lisent d'un bout à l'autre. Les documents **de référence** se consultent : on y cherche une réponse précise, on ne les lit pas linéairement.

| Document | Répond à | Nature |
|---|---|---|
| [`vision.md`](vision.md) | Pourquoi ce projet, pour qui, avec quels principes | Lecture |
| [`architecture.md`](architecture.md) | Quels crates, quelles dépendances, quelles briques externes | Lecture |
| [`specification.md`](specification.md) | Qu'est-ce qui est livré, qu'est-ce qui est exclu | Lecture |
| [`roadmap.md`](roadmap.md) | Où en est le projet, phase par phase | Lecture |
| [`contributing.md`](contributing.md) | Comment contribuer, sous quelle licence | Lecture |
| [`pipeline.md`](pipeline.md) | Ordre des opérations, `settings_json`, versions d'étages, reproductibilité | Référence |
| [`catalog.md`](catalog.md) | Schéma SQLite complet du catalogue | Référence |
| [`engine-api.md`](engine-api.md) | Surface Rust du moteur, modèle d'exécution, sessions d'édition | Référence |
| [`presets.md`](presets.md) | Presets de développement : modèle et comportement | Référence |
| [`v2-scope.md`](v2-scope.md) | Cadrage des fonctionnalités post-V1 | Référence |
| [`v2-implementation-plan.md`](v2-implementation-plan.md) | Séquencement recommandé de ces fonctionnalités | Référence |
| [`adr/`](adr/README.md) | Pourquoi telle décision plutôt qu'une autre | Référence |

---

## Une seule source par sujet

Chaque sujet a **un** document propriétaire, et lui seul fait foi. Les autres y renvoient au lieu de recopier — une information dupliquée finit toujours par diverger, et le lecteur n'a alors aucun moyen de savoir quelle copie est à jour.

En cas de désaccord entre un document et le code, c'est le document qui fait foi : la règle du projet est que le code suit la spécification, et qu'une divergence assumée se règle en modifiant la spécification dans le même changement (plus un ADR si la décision est structurante).

---

## Licence

Leyline est publié sous **GPL-3.0**.

* Moteur et application intégralement Open Source, fonctionnement hors ligne, aucune expiration de licence.
* Des licences commerciales sont prévues à terme (modèle double licence, type Qt) ; la version *community* reste intégralement GPL.
* Les contributions sont soumises à un CLA — voir [`contributing.md`](contributing.md) et [ADR 0009](adr/0009-gpl3-cla-dual-license.md).

---

> **No code before architecture. No architecture before vision.**
