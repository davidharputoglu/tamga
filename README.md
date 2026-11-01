# TAMGA 🏵️

> Le sceau qui unit vos paquets Linux.

![Licence](https://img.shields.io/badge/Licence-MIT-turquoise)
![Rust](https://img.shields.io/badge/Fait%20avec-Rust-orange)
![Tauri](https://img.shields.io/badge/Interface-Tauri-blue)
![Langues](https://img.shields.io/badge/Langues-7-green)

Convertisseur intelligent de paquets Linux :
.deb ↔ .rpm ↔ .pkg ↔ .sh, avec résolution des dépendances
par distribution.

## Ce que fait TAMGA

| De ↓ / vers → | .deb | .rpm | Arch (.pkg.tar.zst) | .sh |
|---|:---:|:---:|:---:|:---:|
| **.deb** (Debian, Ubuntu, Mint…) | — | ✅ | ✅ | ✅ |
| **.rpm** (Fedora, RHEL, openSUSE…) | ✅ | — | ✅ | ✅ |
| **Arch** (.pkg.tar.zst, .pacman) | ✅ | ✅ | — | ✅ |
| **.sh** créé par TAMGA | ✅ | ✅ | ✅ | — |

- **Conversion fidèle** : droits, propriétaires, liens symboliques et fichiers de configuration sont conservés.
- **Dépendances traduites** pour la distribution cible : TAMGA lit les bibliothèques réellement
  utilisées par les programmes (analyse ELF), interroge le système local (paquet installé ?
  disponible dans les dépôts ?), puis ses tables de correspondance, et en option Repology.
- **Installateur .sh autonome** : vérifie l'architecture, la glibc et les bibliothèques, propose la
  commande d'installation adaptée (apt, dnf, zypper, pacman), garde une sauvegarde des fichiers
  remplacés et fournit un désinstallateur.
- **Scripts d'installation** d'origine adaptés au format cible (option avancée, désactivée par défaut).
- **Moteur 100 % Rust** : aucun besoin de dpkg, rpmbuild ou makepkg pour convertir.
- **Mode texte** : `tamga convert paquet.deb --to rpm` (voir `tamga --help-cli`).

## Langues disponibles
🇹🇷 Türkçe | 🇫🇷 Français | 🇬🇧 English | 🇸🇦 العربية |
🇮🇹 Italiano | 🇵🇹 Português | 🇪🇸 Español

## Installer TAMGA

Le dossier `dist/` contient TAMGA prêt à installer :

| Fichier | Pour | Commande |
|---|---|---|
| `TAMGA-0.1.3-1.x86_64.rpm` | Fedora, RHEL, openSUSE | `sudo dnf install ./TAMGA-0.1.3-1.x86_64.rpm` |
| `TAMGA_0.1.3_amd64.deb` | Debian, Ubuntu, Mint | `sudo apt install ./TAMGA_0.1.3_amd64.deb` |
| `TAMGA_0.1.3_amd64.AppImage` | toutes distributions | `chmod +x TAMGA_0.1.3_amd64.AppImage && ./TAMGA_0.1.3_amd64.AppImage` |
| `tamga-0.1.3-x86_64.sh` | toutes distributions | `sh tamga-0.1.3-x86_64.sh` |
| `tamga-0.1.3-source.tar.gz` | code source | voir « Compiler » ci-dessous |

Ces versions demandent une glibc 2.39 ou plus récente (Fedora 40+, Ubuntu 24.04+, Debian 13, Arch).

## Compiler TAMGA

### Fedora
```bash
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
    libappindicator-gtk3-devel librsvg2-devel libxdo-devel nodejs npm
sudo dnf group install development-tools
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Debian / Ubuntu
```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
    libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev nodejs npm
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Puis, dans le dossier du projet
```bash
npm install          # installe l'outil Tauri
npm run dev          # lance TAMGA en mode développement
npm run build        # fabrique les installateurs dans src-tauri/target/release/bundle/
```

Tests du moteur : `cd core && cargo test`.

## Organisation du code

```
core/          moteur de conversion (Rust, sans interface) + outil tamga-cli
  src/deb.rs, rpm.rs, pacman.rs, shell.rs   lecture/écriture de chaque format
  src/deps.rs, mapping.rs, distro.rs        traduction des dépendances
src-tauri/     application de bureau (pont entre l'interface et le moteur)
src/           interface (HTML, CSS, JavaScript) et traductions (locales/)
assets/        logo d'origine
```

## Licence
MIT — voir [LICENSE](LICENSE)
## Avertissement
Voir [DISCLAIMER.md](DISCLAIMER.md)
## Confidentialité
Voir [PRIVACY.md](PRIVACY.md)
