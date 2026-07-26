# Vision

Ce document répond à une seule question : **pourquoi Leyline existe**. Le découpage technique est dans [`architecture.md`](architecture.md), le périmètre fonctionnel dans [`specification.md`](specification.md).

---

## Mission

Créer une plateforme open source de développement RAW moderne, rapide et durable.

Leyline n'est pas un moteur de plus derrière une interface : c'est un moteur autour duquel plusieurs applications peuvent être construites — Studio, la CLI, un SDK, et ce que d'autres en feront.

---

## Le constat de départ

Un photographe amateur exigeant a, aujourd'hui, essentiellement deux options :

* **Adobe Lightroom** — performant et cohérent, mais conditionné à un abonnement : cesser de payer, c'est perdre l'accès à son propre travail d'édition.
* **Les alternatives libres** — souvent très puissantes, parfois difficiles d'accès, et fréquemment héritières d'architectures anciennes qui rendent chaque évolution coûteuse.

Leyline propose une troisième voie : un logiciel rapide, moderne, local, multiplateforme, agréable à utiliser, entièrement non destructif, et dont l'architecture est propre **dès le départ** — parce qu'aucune architecture ne se redresse après coup à coût raisonnable.

---

## Principes

Six principes, dans cet ordre de priorité :

**Local First.** Le logiciel fonctionne intégralement hors connexion. Le réseau ne sert qu'à des usages optionnels et explicites : vérifier qu'une mise à jour existe, la télécharger, éventuellement partager un preset. Aucune fonctionnalité d'édition ne dépend d'une connexion.

**Non destructif.** Un fichier RAW n'est **jamais** modifié. Toutes les corrections sont enregistrées séparément, sous forme d'un pipeline d'étapes indépendantes. Le corollaire est une promesse forte : les mêmes réglages sur le même RAW donnent les mêmes pixels dans dix ans — sa portée exacte est définie dans [`pipeline.md`](pipeline.md) §5.

**Rapide.** Les performances ne sont pas une optimisation de fin de parcours ; elles conditionnent le choix du langage, la structure du cache et le modèle de rendu.

**Modulaire.** Chaque crate a une responsabilité unique, et le moteur ignore jusqu'à l'existence de l'interface graphique.

**Pérenne.** Chaque décision structurante est consignée dans un ADR, avec son contexte, ses alternatives écartées et ses conséquences. Un mainteneur de 2036 doit pouvoir reconstituer *pourquoi* une chose est ainsi, pas seulement constater qu'elle l'est.

**Open Source.** Le cœur est publié sous licence libre, afin que la plateforme puisse continuer d'évoluer indépendamment de toute entreprise — y compris de son auteur.

---

## Ce que le photographe possède

**Ses données.** Aucun cloud obligatoire, aucun abonnement, aucun verrou propriétaire. Tout est stocké localement : le catalogue ne contient que des références, des métadonnées, des réglages, des collections et des index — jamais les photos elles-mêmes.

**Ses RAW.** Le fichier d'origine est traité comme une pièce d'archive : lu, jamais réécrit.

**Son travail d'édition.** Les réglages sont stockés dans un format documenté ([`pipeline.md`](pipeline.md) §3.2), dans une base SQLite ordinaire ([`catalog.md`](catalog.md)). Rien n'est chiffré ni obfusqué : un catalogue Leyline reste lisible même sans Leyline.

---

## Ce que Leyline n'est pas

Leyline n'est ni un clone de Lightroom, ni de darktable, ni de Capture One. Certaines de leurs solutions sont reprises quand elles sont bonnes, et écartées avec une raison écrite quand elles ne le sont pas — voir par exemple [ADR 0042](adr/0042-versioned-stage-pipeline.md), qui compare explicitement les trois approches du gel de rendu avant de trancher.

Ce n'est pas non plus un projet qui accumule les fonctionnalités : les exclusions de [`specification.md`](specification.md) sont des décisions, pas des retards.

---

## Ordre de travail

* Vision avant architecture.
* Architecture avant implémentation.
* Documentation avant code.
* API avant interface graphique.
* Simplicité avant accumulation de fonctionnalités.

> **No code before architecture. No architecture before vision.**

---

## Public visé

**L'amateur passionné** — quelques milliers de photos par an, veut cesser de payer un abonnement, cherche un flux de travail simple et rapide.

**L'amateur expert** — bibliothèque importante, utilise la notation, les collections, les mots-clés et le traitement par lots.

**Le développeur** — se sert du moteur comme d'un SDK Rust, ou de la CLI dans ses propres scripts. Ce public n'est pas un bonus : c'est lui qui justifie que le moteur soit indépendant de l'interface.

---

## Pourquoi « Leyline »

Le nom vient des *ley lines*, ces lignes théoriques reliant des lieux remarquables. L'image décrit exactement le fonctionnement interne du logiciel : une photographie suit un chemin composé de transformations, chaque opération reliée à la suivante. Développer une photo devient un parcours.

D'où : **Leyline** pour la plateforme, **Leyline Engine** pour le moteur, **Leyline Studio** pour l'application.

---

## Ambition

Construire une plateforme photographique ouverte, performante et élégante, dont l'architecture soit encore maintenable dans dix ou vingt ans, où chaque décision est documentée, chaque choix technique justifié, chaque module indépendant — et où les photographes restent propriétaires de leurs images.

Le premier utilisateur de Leyline est son créateur. Si la vision est partagée par d'autres photographes et développeurs, le projet pourra devenir une référence open source du développement RAW.
