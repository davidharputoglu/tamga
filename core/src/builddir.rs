//! Dossier de construction (options « --generate » / « --single » d'Alien).
//! Build folder: instead of a finished package, TAMGA writes a folder that anyone can edit,
//! then open again in TAMGA to build the package.
//!
//! ```text
//! nom-version/
//!   LISEZMOI.txt   explications (français, English, Türkçe)
//!   package.info   la fiche d'informations (nom, version, dépendances…), modifiable
//!   files/         les fichiers tels qu'ils seront installés (files/usr/bin/…)
//!   scripts/       scripts lancés à l'installation (preinst, postinst, prerm, postrm)
//! ```

use crate::model::{Dependency, Family, Format, Metadata, Package, ScriptStyle, Scripts};
use crate::tree::{Entry, Kind, Tree};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub const INFO_FILE: &str = "package.info";

/// Le dossier est-il un dossier de construction de TAMGA ?
pub fn is_build_folder(dir: &Path) -> bool {
    dir.is_dir() && dir.join(INFO_FILE).is_file() && dir.join("files").is_dir()
}

fn deps_line(list: &[Dependency]) -> String {
    list.iter().map(|d| d.display()).collect::<Vec<_>>().join(", ")
}

fn family_id(f: Family) -> &'static str {
    f.id()
}

/// Texte de la fiche d'informations.
pub fn info_text(meta: &Metadata, format: Format, family: Family, depends: &[Dependency], config: &[String]) -> String {
    let desc = {
        let mut lines = meta.description.lines();
        let first = lines.next().unwrap_or("").trim().to_string();
        let rest: String = lines.map(|l| if l.trim().is_empty() { " .\n".to_string() } else { format!(" {}\n", l.trim_end()) }).collect();
        format!("{first}\n{rest}")
    };
    let style = match meta.scripts.style {
        ScriptStyle::Deb => "deb",
        ScriptStyle::Rpm => "rpm",
        ScriptStyle::Arch => "arch",
        ScriptStyle::Plain => "plain",
        ScriptStyle::None => "none",
    };
    format!(
        "# TAMGA — fiche d'informations du paquet / package information / paket bilgileri\n\
         # Modifiez les valeurs après « : ». Les lignes qui commencent par # sont ignorées.\n\
         # Edit the values after \":\". Lines starting with # are ignored.\n\
         # « : » işaretinden sonraki değerleri değiştirin. # ile başlayan satırlar yok sayılır.\n\
         #\n\
         # Format : format conseillé pour fabriquer le paquet (deb, rpm, arch, sh, slack, tar, slp).\n\
         # Family : façon de nommer les dépendances (debian, fedora, suse, arch, generic).\n\
         # Depends : paquets nécessaires, séparés par des virgules ; « nom (>= 1.2) » pour une version minimale.\n\
         # Config-Files : fichiers de configuration (gardés lors des mises à jour), séparés par des virgules.\n\
         # Description : la suite de la description commence par une espace ; « . » seul = ligne vide.\n\
         Format: {format}\n\
         Family: {family}\n\
         Name: {name}\n\
         Version: {version}\n\
         Release: {release}\n\
         Epoch: {epoch}\n\
         Architecture: {arch}\n\
         Summary: {summary}\n\
         Description: {desc}\
         License: {license}\n\
         Homepage: {url}\n\
         Maintainer: {maint}\n\
         Section: {section}\n\
         Depends: {depends}\n\
         Provides: {provides}\n\
         Conflicts: {conflicts}\n\
         Replaces: {replaces}\n\
         Config-Files: {config}\n\
         Scripts-Style: {style}\n",
        format = format.id(),
        family = family_id(family),
        name = meta.name,
        version = meta.version,
        release = meta.release,
        epoch = meta.epoch.map(|e| e.to_string()).unwrap_or_default(),
        arch = meta.arch,
        summary = meta.summary.lines().next().unwrap_or(""),
        license = meta.license,
        url = meta.url,
        maint = meta.maintainer,
        section = meta.section,
        depends = deps_line(depends),
        provides = deps_line(&meta.provides),
        conflicts = deps_line(&meta.conflicts),
        replaces = deps_line(&meta.replaces),
        config = config.join(", "),
    )
}

const README: &str = "\
TAMGA — dossier de construction / build folder / yapı klasörü
==============================================================

FRANÇAIS
--------
Ce dossier contient tout ce qu'il faut pour fabriquer le paquet. Vous pouvez le modifier :
  • package.info : la fiche d'informations (nom, version, description, dépendances…).
    Changez simplement le texte après « : ».
  • files/ : les fichiers tels qu'ils seront installés. Par exemple, files/usr/bin/programme
    sera installé dans /usr/bin/programme. Ajoutez, remplacez ou supprimez des fichiers.
  • scripts/ : les commandes lancées pendant l'installation (preinst = avant, postinst = après)
    et la désinstallation (prerm = avant, postrm = après). Facultatif.
Pour fabriquer le paquet : ouvrez TAMGA et glissez ce dossier dans la fenêtre
(ou cliquez sur « Ouvrir un dossier de construction »), puis choisissez le format.
En ligne de commande : tamga convert ce-dossier --to deb   (ou rpm, arch, sh, slack, tar, slp)

ENGLISH
-------
This folder holds everything needed to build the package. You can edit it:
  • package.info: the package information (name, version, description, dependencies…).
    Just change the text after \":\".
  • files/: the files as they will be installed. For example, files/usr/bin/program
    will be installed as /usr/bin/program. Add, replace or delete files.
  • scripts/: commands run during installation (preinst = before, postinst = after)
    and removal (prerm = before, postrm = after). Optional.
To build the package: open TAMGA and drag this folder into the window
(or click \"Open a build folder\"), then choose the format.
Command line: tamga convert this-folder --to deb   (or rpm, arch, sh, slack, tar, slp)

TÜRKÇE
------
Bu klasör paketi oluşturmak için gereken her şeyi içerir. Değiştirebilirsiniz:
  • package.info: paket bilgileri (ad, sürüm, açıklama, bağımlılıklar…).
    « : » işaretinden sonraki metni değiştirmeniz yeterli.
  • files/: kurulacak dosyalar. Örneğin files/usr/bin/program, /usr/bin/program olarak kurulur.
    Dosya ekleyebilir, değiştirebilir veya silebilirsiniz.
  • scripts/: kurulum (preinst = önce, postinst = sonra) ve kaldırma (prerm = önce,
    postrm = sonra) sırasında çalışan komutlar. İsteğe bağlı.
Paketi oluşturmak için: TAMGA'yı açın ve bu klasörü pencereye sürükleyin
(ya da « Yapı klasörü aç » düğmesine tıklayın), sonra biçimi seçin.
Komut satırı: tamga convert bu-klasor --to deb   (ya da rpm, arch, sh, slack, tar, slp)
";

/// Écrit le dossier de construction `<out>/<nom>-<version>/` et retourne son chemin.
pub fn write(meta: &Metadata, tree: &Tree, format: Format, family: Family, depends: &[Dependency], include_scripts: bool, out: &Path) -> Result<PathBuf> {
    // Dossier déjà de construction (on le refait), sinon un sous-dossier « nom-version ».
    let dir = if out.join(INFO_FILE).exists() {
        out.to_path_buf()
    } else {
        out.join(format!("{}-{}", crate::version::deb_name(&meta.name), meta.version.replace(['/', ' '], "_")))
    };
    if dir.join("files").exists() {
        std::fs::remove_dir_all(dir.join("files")).with_context(|| format!("nettoyage de {}", dir.display()))?;
    }
    let files = dir.join("files");
    std::fs::create_dir_all(&files)?;
    let mut config = Vec::new();
    for (p, e) in tree.entries.iter() {
        let dest = files.join(p);
        match &e.kind {
            Kind::Dir => {
                std::fs::create_dir_all(&dest)?;
            }
            Kind::File(b) => {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut src = tree.store.open(b)?;
                let mut f = std::fs::File::create(&dest)?;
                std::io::copy(&mut src, &mut f)?;
                if e.config {
                    config.push(format!("/{p}"));
                }
            }
            Kind::Symlink(t) => {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let _ = std::fs::remove_file(&dest);
                std::os::unix::fs::symlink(t, &dest)?;
            }
        }
    }
    // Droits : après coup, pour que les dossiers restent modifiables pendant la copie.
    {
        use std::os::unix::fs::PermissionsExt;
        for (p, e) in tree.entries.iter().rev() {
            if matches!(e.kind, Kind::Symlink(_)) {
                continue;
            }
            let mode = if e.is_dir() { e.mode | 0o700 } else { e.mode | 0o600 };
            let _ = std::fs::set_permissions(files.join(p), std::fs::Permissions::from_mode(mode & 0o7777));
        }
    }
    std::fs::write(dir.join(INFO_FILE), info_text(meta, format, family, depends, &config))?;
    std::fs::write(dir.join("LISEZMOI.txt"), README)?;
    let sdir = dir.join("scripts");
    let _ = std::fs::remove_dir_all(&sdir);
    if include_scripts && !meta.scripts.is_empty() {
        std::fs::create_dir_all(&sdir)?;
        for (name, body) in [
            ("preinst", &meta.scripts.pre_install),
            ("postinst", &meta.scripts.post_install),
            ("prerm", &meta.scripts.pre_remove),
            ("postrm", &meta.scripts.post_remove),
            ("arch.install", &meta.scripts.arch_install),
        ] {
            if let Some(b) = body {
                std::fs::write(sdir.join(name), b)?;
            }
        }
    }
    crate::vlog!("dossier", "dossier de construction écrit : {} ({} fichiers)", dir.display(), tree.file_count());
    Ok(dir)
}

/// Relit un dossier de construction (éventuellement modifié à la main).
pub fn read(dir: &Path) -> Result<Package> {
    if !is_build_folder(dir) {
        bail!("err.not_build_folder");
    }
    let text = std::fs::read_to_string(dir.join(INFO_FILE))?;
    let clean: String = text.lines().filter(|l| !l.trim_start().starts_with('#')).map(|l| format!("{l}\n")).collect();
    let fields = crate::deb::parse_control(&clean);
    let get = |k: &str| fields.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
    let name = get("Name");
    if name.is_empty() {
        bail!("err.empty_name");
    }
    let description = {
        let d = get("Description");
        d.lines()
            .map(|l| {
                let t = l.trim();
                if t == "." {
                    String::new()
                } else {
                    t.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let family = Family::parse(&get("Family")).unwrap_or(Family::Generic);
    let list = |k: &str| crate::deb::parse_dep_list(&get(k));
    let style = match get("Scripts-Style").as_str() {
        "deb" => ScriptStyle::Deb,
        "rpm" => ScriptStyle::Rpm,
        "arch" => ScriptStyle::Arch,
        "plain" => ScriptStyle::Plain,
        _ => ScriptStyle::Plain,
    };
    let script = |n: &str| std::fs::read_to_string(dir.join("scripts").join(n)).ok().filter(|s| !s.trim().is_empty());
    let mut scripts = Scripts {
        style,
        pre_install: script("preinst"),
        post_install: script("postinst"),
        pre_remove: script("prerm"),
        post_remove: script("postrm"),
        arch_install: script("arch.install"),
    };
    if scripts.is_empty() {
        scripts.style = ScriptStyle::None;
    }
    let meta = Metadata {
        name,
        version: get("Version"),
        release: get("Release"),
        epoch: get("Epoch").parse().ok(),
        arch: if get("Architecture").is_empty() { "noarch".into() } else { crate::version::canonical_arch(&get("Architecture")) },
        summary: get("Summary"),
        description,
        license: get("License"),
        url: get("Homepage"),
        maintainer: get("Maintainer"),
        section: get("Section"),
        depends: list("Depends"),
        provides: list("Provides"),
        conflicts: list("Conflicts"),
        replaces: list("Replaces"),
        dep_family: Some(family),
        scripts,
        origin_format: Format::parse(&get("Format")),
    };
    // Les fichiers, tels quels (tout appartient à root).
    let mut tree = Tree::new()?;
    let root = dir.join("files");
    let configs: Vec<String> = get("Config-Files").split(',').map(|s| s.trim().trim_start_matches('/').to_string()).filter(|s| !s.is_empty()).collect();
    walk(&root, &root, &mut tree)?;
    for c in &configs {
        if let Some(e) = tree.entries.get_mut(c) {
            e.config = true;
        }
    }
    crate::vlog!("dossier", "dossier de construction lu : {} — {} {} ({} fichiers)", dir.display(), meta.name, meta.version, tree.file_count());
    Ok(Package { meta, tree, format: Format::Tar, warnings: vec![] })
}

fn walk(root: &Path, dir: &Path, tree: &mut Tree) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for ent in entries {
        let p = ent.path();
        let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().to_string();
        let md = std::fs::symlink_metadata(&p)?;
        let mtime = md.mtime().max(0) as u64;
        let mode = md.mode() & 0o7777;
        if md.file_type().is_symlink() {
            let t = std::fs::read_link(&p)?;
            tree.insert(&rel, Entry::symlink(&t.to_string_lossy(), mtime));
        } else if md.is_dir() {
            tree.insert(&rel, Entry::dir(mode, mtime));
            walk(root, &p, tree)?;
        } else if md.is_file() {
            let mut f = std::fs::File::open(&p)?;
            let blob = tree.store.add_reader(&mut f)?;
            tree.insert(&rel, Entry::file(blob, mode, mtime));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut t = Tree::new().unwrap();
        t.add_file_bytes("usr/bin/hi", b"#!/bin/sh\necho hi\n", 0o755, 1).unwrap();
        t.add_file_bytes("etc/hi.conf", b"x=1\n", 0o644, 1).unwrap();
        t.entries.get_mut("etc/hi.conf").unwrap().config = true;
        t.insert("usr/bin/hello", Entry::symlink("hi", 1));
        let meta = Metadata {
            name: "hi".into(),
            version: "1.0".into(),
            release: "2".into(),
            arch: "noarch".into(),
            summary: "Hi".into(),
            description: "Ligne 1\n\nLigne 3".into(),
            scripts: Scripts { style: ScriptStyle::Deb, post_install: Some("echo ok\n".into()), ..Default::default() },
            ..Default::default()
        };
        let deps = vec![Dependency::with("bash", crate::model::Op::Ge, "5.0"), Dependency::simple("coreutils")];
        let d = tempfile::tempdir().unwrap();
        let dir = write(&meta, &t, Format::Deb, Family::Debian, &deps, true, d.path()).unwrap();
        assert!(is_build_folder(&dir));
        // Modification à la main de la fiche, comme le ferait un utilisateur.
        let info = std::fs::read_to_string(dir.join(INFO_FILE)).unwrap().replace("Version: 1.0", "Version: 1.1");
        std::fs::write(dir.join(INFO_FILE), info).unwrap();
        let back = read(&dir).unwrap();
        assert_eq!(back.meta.version, "1.1");
        assert_eq!(back.meta.release, "2");
        assert_eq!(back.meta.description, "Ligne 1\n\nLigne 3");
        assert_eq!(back.meta.depends[0].display(), "bash (>= 5.0)");
        assert_eq!(back.meta.dep_family, Some(Family::Debian));
        assert_eq!(back.meta.origin_format, Some(Format::Deb));
        assert_eq!(back.meta.scripts.post_install.as_deref(), Some("echo ok\n"));
        assert!(back.tree.get("etc/hi.conf").unwrap().config);
        assert_eq!(back.tree.read("usr/bin/hi").unwrap(), b"#!/bin/sh\necho hi\n");
        assert!(matches!(back.tree.get("usr/bin/hello").unwrap().kind, Kind::Symlink(_)));
        assert_eq!(back.tree.get("usr/bin/hi").unwrap().mode & 0o777, 0o755);
    }
}
