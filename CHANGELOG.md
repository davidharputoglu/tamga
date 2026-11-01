# Changelog

Toutes les modifications notables de TAMGA sont documentées dans ce fichier.
Format basé sur [Keep a Changelog](https://keepachangelog.com/).

## [0.1] - Le moteur de conversion

### Ajouté
- Moteur de conversion en Rust pur (`core/`, crate `tamga-core`) : aucun outil externe nécessaire.
- Lecture et écriture des paquets **.deb**, **.rpm** (format v4, empreintes SHA-256),
  **Arch** (.pkg.tar.zst avec .PKGINFO et .MTREE) et des installateurs **.sh** de TAMGA.
- Analyse des programmes (ELF) : bibliothèques utilisées, glibc minimale.
- Traduction des dépendances par distribution : système local, tables de correspondance,
  règles de nommage Debian, recherche en ligne Repology (avec cache local).
- Installateur .sh autonome en 7 langues : vérification de l'architecture, de la glibc et des
  bibliothèques, sauvegarde des fichiers remplacés, désinstallateur.
- Adaptation optionnelle des scripts d'installation d'un format à l'autre.
- Fusion /usr automatique et fichier ld.so.conf.d quand les bibliothèques sont dans un
  dossier inhabituel pour la distribution cible.
- Interface complète : avertissement au premier lancement, analyse du paquet, choix du format
  et de la distribution cible, tableau des dépendances, résultat avec la commande d'installation,
  fenêtre « À propos ».
- Mode texte : `tamga analyze|deps|convert|system`.
- Nouveau logo (sceau turquoise) comme icône de l'application et favicon.
- Installateurs de TAMGA : .deb, .rpm, AppImage, .sh et archive du code source.
- Construction automatique des installateurs par GitHub Actions, publiés sur la page Releases.
- Installateurs pour les processeurs ARM : aarch64/arm64 (64 bits) et armhf (32 bits, sans AppImage).

### Corrigé
- Le clic sur la zone de dépôt ouvre la vraie fenêtre de choix de fichier (module dialog).
- Le nom du fichier s'affiche correctement quel que soit le séparateur de chemin.
- Identifiant de l'application : `io.github.davidharputoglu.tamga` (au lieu de `com.tauri.dev`).
- CHANGELOG remis en ordre, drapeaux du README réparés.

## [0.1.2] - Les langues

### Ajouté
- Moteur de traduction `src/i18n.js` avec chargement dynamique des langues.
- Sélecteur de langue visuel avec 7 drapeaux.
- Détection automatique de la langue du système au premier lancement.
- Bascule RTL (droite à gauche) automatique pour l'arabe.
- Mémorisation de la langue choisie dans le localStorage.
- Traduction du mot "version" dans les 7 langues.

## [0.1.1] - Première fenêtre vivante

### Ajouté
- Initialisation du projet Tauri (backend Rust + WebView native).
- Première fenêtre TAMGA : thème turquoise / anthracite.
- Création de `src/index.html` et `src/style.css`.
- Synchronisation de la version 0.1.1 dans package.json, tauri.conf.json et Cargo.toml.

## [0.1.0] - Naissance du projet

### Ajouté
- 🏵️ Choix du nom : **TAMGA** (le sceau turcique).
- 🎨 Identité visuelle : logo turquoise sur fond anthracite.
- 🌍 Support multilingue : TR, FR, EN, AR (RTL), IT, PT, ES.
- ⚖️ Documents légaux : LICENSE (MIT), DISCLAIMER.md, PRIVACY.md.
- 🛠️ Choix technologique : Rust + Tauri.
