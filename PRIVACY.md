# Politique de confidentialité

- TAMGA ne collecte **aucune donnée personnelle**.
- Aucune télémétrie, aucun traceur, aucune publicité.
- Lors des recherches en ligne (option « Chercher en ligne », activable ou désactivable dans
  l'écran de conversion) :
  - seul le **nom du paquet** est envoyé au service public Repology (repology.org) ;
  - pour fournir une bibliothèque manquante, TAMGA télécharge en **HTTPS** les listes de paquets et
    les paquets nécessaires depuis les **serveurs officiels** des distributions (archive.ubuntu.com,
    ports.ubuntu.com, deb.debian.org, archive.debian.org, dl.fedoraproject.org,
    archives.fedoraproject.org, repo.almalinux.org, vault.almalinux.org, download.opensuse.org,
    geo.mirror.pkgbuild.com, mirrors.kernel.org). Comme pour tout téléchargement, ces serveurs voient
    votre adresse IP et le nom des fichiers demandés ; rien d'autre n'est envoyé.
- Les vérifications sur votre machine (paquet installé ? disponible dans les dépôts ?)
  utilisent les outils de votre distribution (dpkg, apt, rpm, dnf, zypper, pacman) ;
  ces outils peuvent eux-mêmes contacter les dépôts configurés sur votre système.
- Un cache local est stocké sur votre machine pour accélérer les recherches
  suivantes (`~/.cache/tamga/` : résultats, listes de paquets, paquets et bibliothèques
  téléchargés). Vous pouvez le supprimer à tout moment depuis la fenêtre « À propos ».
- Le mode verbeux (fenêtre « Paramètres », désactivé par défaut) écrit un journal de ce que fait
  TAMGA dans `~/.cache/tamga/journal.log`, sur votre machine uniquement.
- TAMGA fonctionne entièrement hors-ligne pour les conversions simples.
