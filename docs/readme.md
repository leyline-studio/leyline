# Leyline Studio

> **Open Source RAW Development Platform**
>
> **Code name:** Leyline
>
> **Desktop application:** Leyline Studio
>
> *Fast. Local. Open.*

---

# Introduction

Leyline est un projet de logiciel de développement photographique RAW nouvelle génération.

L'objectif n'est **pas** de reproduire Adobe Lightroom fonctionnalité par fonctionnalité.

L'objectif est de concevoir une plateforme moderne, durable et ouverte permettant de développer des photographies RAW sans abonnement, sans cloud obligatoire et sans compromis sur les performances.

Le projet est développé en Open Source, avec une architecture pensée dès le départ pour durer plusieurs décennies.

---

# Pourquoi ce projet ?

Aujourd'hui, la plupart des photographes amateurs ou passionnés disposent principalement de deux solutions :

* Adobe Lightroom, performant mais basé sur un abonnement.
* Quelques alternatives Open Source souvent très puissantes mais parfois difficiles d'accès ou héritées d'architectures anciennes.

Leyline souhaite proposer une troisième voie.

Un logiciel :

* rapide
* moderne
* local
* multiplateforme
* agréable à utiliser
* entièrement non destructif
* conçu dès le départ avec une architecture propre.

---

# Notre philosophie

## Les photographes restent propriétaires de leurs données.

Aucun cloud obligatoire.

Aucun abonnement.

Aucun verrou propriétaire.

Toutes les données sont stockées localement.

---

## Le RAW appartient au photographe.

Un fichier RAW ne sera **jamais** modifié.

Toutes les corrections seront enregistrées séparément.

---

## Local First

Le logiciel fonctionne totalement hors connexion.

Internet ne sera utilisé que pour :

* vérifier les nouvelles versions (optionnel)
* télécharger les mises à jour
* éventuellement partager volontairement des presets.

---

## Open Source

Le cœur du projet sera publié sous licence libre.

L'objectif est de créer une plateforme durable pouvant continuer à évoluer indépendamment d'une entreprise.

---

# Ce que Leyline n'est pas

Leyline n'est pas :

* un clone de Lightroom
* un clone de Darktable
* un clone de Capture One

Le projet possède sa propre vision.

---

# Les grands principes

* Architecture avant implémentation.
* Vision avant architecture.
* Documentation avant code.
* API avant interface graphique.
* Simplicité avant accumulation de fonctionnalités.

Notre devise est :

> **No code before architecture. No architecture before vision.**

---

# Les objectifs

Leyline doit permettre :

* d'importer une bibliothèque de photos ;
* d'organiser un catalogue ;
* de développer des RAW de manière non destructive ;
* d'exporter rapidement différents formats ;
* de gérer plusieurs centaines de milliers de photographies.

---

# Les utilisateurs visés

## Amateur passionné

* quelques milliers de photos par an ;
* souhaite arrêter de payer un abonnement ;
* recherche un workflow simple et rapide.

---

## Amateur expert

* bibliothèque importante ;
* notation ;
* collections ;
* mots-clés ;
* traitement par lots.

---

## Développeur

Le moteur pourra être utilisé comme SDK Rust.

---

# Les choix techniques

## Langage

Rust

Pourquoi ?

* performances proches du C++
* sécurité mémoire
* excellent écosystème
* très bonne portabilité
* pérennité

---

## Interface graphique

Slint

Pourquoi ?

* multiplateforme
* moderne
* parfaitement intégré à Rust
* léger
* performant

---

## Base de données

SQLite

Pourquoi ?

* aucun serveur
* un simple fichier
* extrêmement rapide
* robuste
* utilisé par de nombreux logiciels photo

Le catalogue ne contient jamais les photos.

Uniquement :

* les références
* les métadonnées
* les réglages
* les collections
* les index

---

## Cache

Les aperçus seront stockés dans un cache dédié.

Par exemple :

```
catalog.db

cache/

preview/

thumbs/
```

Les RAW ne seront relus que lorsque cela est nécessaire.

---

# L'architecture

Le projet sera composé de plusieurs crates Rust.

```
Leyline

├── leyline-core
├── leyline-engine
├── leyline-raw
├── leyline-catalog
├── leyline-preview
├── leyline-color
├── leyline-lens
├── leyline-export
├── leyline-sdk
├── leyline-cli
└── leyline-studio
```

Chaque crate possède une responsabilité unique.

---

# Le moteur

Le moteur est indépendant de l'interface graphique.

Il pourra être utilisé :

* par Leyline Studio ;
* par un outil CLI ;
* par des scripts ;
* par d'autres applications.

L'interface graphique n'est qu'un client du moteur.

---

# Le pipeline

Toutes les corrections sont appliquées sous forme de pipeline.

```
RAW

↓

Balance des blancs

↓

Exposition

↓

Contraste

↓

Ombres

↓

Hautes lumières

↓

Correction optique

↓

Netteté

↓

Export
```

Chaque étape est indépendante.

---

# Le catalogue

Le catalogue repose sur SQLite.

Il gère :

* bibliothèques ;
* collections ;
* mots-clés ;
* notes ;
* couleurs ;
* EXIF ;
* historique ;
* recherches.

---

# Le développement RAW

La première version proposera notamment :

* exposition
* contraste
* balance des blancs
* noirs
* blancs
* ombres
* hautes lumières
* vibrance
* saturation
* rotation
* recadrage
* correction d'objectif
* réduction du bruit
* netteté

---

# Export

Formats prévus :

* JPEG
* TIFF
* PNG
* WebP
* AVIF

---

# Ce qui ne sera pas présent en V1

Volontairement :

* cloud
* compte utilisateur
* abonnement
* intelligence artificielle
* HDR
* panorama
* reconnaissance faciale
* synchronisation automatique

Ces fonctionnalités pourront être étudiées plus tard si elles apportent une réelle valeur.

---

# Licence

Leyline est publié sous **GPL-3.0**.

* moteur et application Open Source (GPL-3.0) ;
* fonctionnement totalement hors ligne ;
* aucune expiration de licence ;
* vérification des mises à jour uniquement pour proposer une nouvelle version ;
* des licences commerciales seront proposées à terme (modèle double licence, type Qt) — la version community reste intégralement GPL ;
* les contributions sont soumises à un CLA (voir `contributing.md`).

---

# Pourquoi "Leyline" ?

Le nom est inspiré des **Ley Lines**, ces lignes théoriques reliant différents lieux remarquables.

Cette idée représente parfaitement le fonctionnement interne du logiciel :

Une photographie suit un chemin composé de plusieurs transformations.

Chaque opération est reliée à la suivante.

Le développement d'une photo devient un parcours.

Le nom de l'application est donc :

# Leyline Studio

Le moteur :

# Leyline Engine

La plateforme :

# Leyline

---

# Notre ambition

Nous ne voulons pas simplement développer un logiciel.

Nous voulons construire une plateforme photographique moderne.

Une architecture suffisamment propre pour être encore maintenable dans dix ou vingt ans.

Une plateforme où chaque décision est documentée.

Où chaque choix technique est justifié.

Où chaque module est indépendant.

Où les photographes restent propriétaires de leurs images.

---

# Une vision à long terme

Leyline est pensé comme un projet Open Source de référence.

Avant la première ligne de Rust, toute l'architecture sera documentée :

* vision
* principes
* architecture
* modèle métier
* pipeline
* catalogue
* API
* moteur
* SDK
* interface
* conventions
* tests
* ADR (Architecture Decision Records)

Cette documentation constituera la base du projet pendant toute sa durée de vie.

---

# Conclusion

Leyline n'a pas vocation à remplacer Lightroom du jour au lendemain.

Son objectif est beaucoup plus ambitieux sur le long terme :

Construire une plateforme ouverte, performante, durable et élégante permettant aux photographes de développer leurs images sans dépendre d'un abonnement, d'un cloud ou d'un format propriétaire.

Le premier utilisateur de Leyline sera son créateur.

Mais si cette vision est partagée par d'autres photographes et développeurs, le projet pourra devenir une véritable référence Open Source dans le domaine du développement RAW.

