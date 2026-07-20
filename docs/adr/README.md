# Architecture Decision Records

Chaque décision structurante est consignée ici : contexte, décision, conséquences, alternatives écartées.

Une décision acceptée ne se modifie pas : elle se remplace par un nouvel ADR qui la référence.

| ADR | Décision |
|---|---|
| [0001](0001-rust.md) | Rust comme langage unique |
| [0002](0002-slint.md) | Slint pour l'interface graphique |
| [0003](0003-sqlite.md) | SQLite comme unique base du catalogue |
| [0004](0004-libraw.md) | LibRaw (branche LGPL) pour le décodage RAW |
| [0005](0005-lensfun-littlecms.md) | Lensfun et LittleCMS |
| [0006](0006-blake3.md) | BLAKE3 pour les checksums |
| [0007](0007-git-develop-model.md) | Modèle de développement inspiré de Git |
| [0008](0008-version-as-library-unit.md) | La version comme unité de bibliothèque |
| [0009](0009-gpl3-cla-dual-license.md) | GPL-3.0 + CLA, modèle double licence |
| [0010](0010-relative-paths.md) | Bibliothèque autonome, chemins relatifs |
| [0011](0011-engine-api-model.md) | API moteur : lib Rust, sync/async, événements |
| [0012](0012-rayon-data-parallelism.md) | Parallélisme de données Rayon, rendu bit-pour-bit |
| [0013](0013-process-2-lut-transfer.md) | Process 2 : fonctions de transfert sRGB par table |
| [0014](0014-develop-presets.md) | Presets de développement : catégories partielles, application par révision normale |
| [0015](0015-color-management-srgb.md) | Gestion des couleurs V1 : pipeline et export figés en sRGB |
