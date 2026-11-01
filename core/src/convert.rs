//! Point d'entrée : analyser un fichier, puis le convertir.
//! High-level API: load/analyze a file, then convert it.

use crate::deps::{self, DepItem, ResolveCtx, TargetDistro};
use crate::detect::{self, Detected};
use crate::distro::SystemInfo;
use crate::elf::{self, ElfInfo};
use crate::fsdirs;
use crate::model::{Dependency, Family, Format, Metadata, Package};
use crate::scripts::{self, Integration};
use crate::tree::{Entry, Tree};
use crate::{deb, pacman, rpm, shell, version};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Charge un paquet, quel que soit son format.
/// Les erreurs « connues » commencent par « err. » (clé de traduction de l'interface).
pub fn load(path: &Path) -> Result<Package> {
    match detect::detect(path) {
        Detected::Package(Format::Deb) => deb::read(path),
        Detected::Package(Format::Rpm) => rpm::read(path),
        Detected::Package(Format::Arch) => pacman::read(path),
        Detected::Package(Format::Sh) => shell::read(path),
        Detected::ForeignScript => bail!("err.foreign_script"),
        Detected::SourceRpm => bail!("err.source_rpm"),
        Detected::AppImage => bail!("err.appimage"),
        Detected::Archive => bail!("err.archive"),
        Detected::Unknown => bail!("err.unknown_format"),
    }
}

/// Résumé affiché à l'utilisateur après l'analyse.
#[derive(Clone, Debug, Serialize)]
pub struct Analysis {
    pub path: String,
    pub file_name: String,
    pub file_size: u64,
    pub format: Format,
    pub meta: Metadata,
    pub file_count: usize,
    pub dir_count: usize,
    pub installed_size: u64,
    pub elf: ElfInfo,
    pub has_scripts: bool,
    pub config_files: usize,
    pub desktop_files: Vec<String>,
    pub warnings: Vec<String>,
    /// Aperçu des fichiers (200 premiers).
    pub files_preview: Vec<String>,
    /// Formats vers lesquels on peut convertir.
    pub targets: Vec<Format>,
}

pub fn analyze(pkg: &Package, path: &Path) -> (Analysis, ElfInfo) {
    let elf = elf::scan(&pkg.tree);
    let mut warnings = pkg.warnings.clone();
    if let (Some(m), false) = (&elf.machine, pkg.meta.arch == "noarch") {
        if *m != pkg.meta.arch {
            warnings.push("warn.arch_mismatch".into());
        }
    }
    if pkg.meta.arch == "noarch" && elf.elf_files > 0 {
        warnings.push("warn.noarch_binaries".into());
    }
    let a = Analysis {
        path: path.to_string_lossy().to_string(),
        file_name: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        file_size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        format: pkg.format,
        meta: pkg.meta.clone(),
        file_count: pkg.tree.file_count(),
        dir_count: pkg.tree.dir_count(),
        installed_size: pkg.tree.total_size(),
        elf: elf.clone(),
        has_scripts: !pkg.meta.scripts.is_empty(),
        config_files: pkg.tree.entries.values().filter(|e| e.config).count(),
        desktop_files: pkg
            .tree
            .entries
            .keys()
            .filter(|k| k.starts_with("usr/share/applications/") && k.ends_with(".desktop"))
            .cloned()
            .collect(),
        warnings,
        files_preview: pkg.tree.entries.iter().filter(|(_, e)| !e.is_dir()).take(200).map(|(p, _)| format!("/{p}")).collect(),
        targets: Format::all().into_iter().filter(|f| *f != pkg.format).collect(),
    };
    (a, elf)
}

/// Champs que l'utilisateur peut corriger avant la conversion.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MetaOverride {
    pub name: Option<String>,
    pub version: Option<String>,
    pub release: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub maintainer: Option<String>,
    pub license: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConvertOptions {
    pub format: Format,
    pub distro: TargetDistro,
    #[serde(default)]
    pub meta: MetaOverride,
    /// Dépendances finales choisies dans l'interface (sinon : résolution automatique).
    #[serde(default)]
    pub deps: Option<Vec<Dependency>>,
    #[serde(default)]
    pub include_scripts: bool,
    #[serde(default = "yes")]
    pub usrmerge: bool,
    #[serde(default)]
    pub online: bool,
    /// Fichier de sortie, ou dossier (le nom est alors choisi automatiquement).
    pub output: PathBuf,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize)]
pub struct ConvertResult {
    pub path: String,
    pub size: u64,
    pub format: Format,
    pub install_command: String,
    pub depends: Vec<String>,
    pub warnings: Vec<String>,
}

fn apply_override(meta: &mut Metadata, o: &MetaOverride) {
    let set = |dst: &mut String, v: &Option<String>| {
        if let Some(v) = v {
            *dst = v.trim().to_string();
        }
    };
    set(&mut meta.name, &o.name);
    set(&mut meta.version, &o.version);
    set(&mut meta.release, &o.release);
    set(&mut meta.summary, &o.summary);
    set(&mut meta.description, &o.description);
    set(&mut meta.maintainer, &o.maintainer);
    set(&mut meta.license, &o.license);
    set(&mut meta.url, &o.url);
}

/// Nom de fichier conseillé pour la cible.
pub fn default_file_name(meta: &Metadata, format: Format) -> String {
    match format {
        Format::Deb => {
            let full = version::deb_version(meta);
            let no_epoch = full.split_once(':').map(|(_, r)| r.to_string()).unwrap_or(full);
            format!("{}_{}_{}.deb", version::deb_name(&meta.name), no_epoch, version::deb_arch(&meta.arch))
        }
        Format::Rpm => {
            let (v, r) = version::rpm_version(meta);
            format!("{}-{}-{}.{}.rpm", version::rpm_name(&meta.name), v, r, version::rpm_arch(&meta.arch))
        }
        Format::Arch => {
            let (v, r) = version::pacman_version(meta);
            let e = meta.epoch.filter(|e| *e > 0).map(|e| format!("{e}:")).unwrap_or_default();
            format!("{}-{}{}-{}-{}.pkg.tar.zst", version::pacman_name(&meta.name), e, v, r, version::pacman_arch(&meta.arch))
        }
        Format::Sh => {
            let full = version::deb_version(meta);
            let no_epoch = full.split_once(':').map(|(_, r)| r.to_string()).unwrap_or(full);
            format!("{}-{}-{}.sh", version::deb_name(&meta.name), no_epoch, version::canonical_arch(&meta.arch))
        }
    }
}

pub fn install_command(format: Format, fam: Family, file: &str) -> String {
    let f = format!("./{file}");
    match format {
        Format::Deb => format!("sudo apt install {f}"),
        Format::Rpm if fam == Family::Suse => format!("sudo zypper install --allow-unsigned-rpm {f}"),
        Format::Rpm => format!("sudo dnf install {f}"),
        Format::Arch => format!("sudo pacman -U {f}"),
        Format::Sh => format!("sh {f}"),
    }
}

/// Garde les « provides » simples ; enlève conflits et remplacements entre familles différentes.
fn carry(list: &[Dependency], same_family: bool, keep_plain: bool) -> Vec<Dependency> {
    if same_family {
        return list.to_vec();
    }
    if !keep_plain {
        return vec![];
    }
    list.iter()
        .filter(|d| !d.name.contains('(') && !d.name.contains('/') && !d.name.contains(".so") && !d.name.contains(':'))
        .map(|d| Dependency { name: d.name.clone(), op: None, version: None, alternatives: vec![] })
        .collect()
}

/// Résout les dépendances (utilisé par l'interface pour afficher le tableau).
pub fn resolve_deps(pkg: &Package, elf: &ElfInfo, format: Format, distro: TargetDistro, online: bool, sys: &SystemInfo) -> Vec<DepItem> {
    let ctx = ResolveCtx { format, distro, online, sys };
    deps::resolve(&pkg.meta, elf, &ctx)
}

/// Convertit le paquet chargé vers le format demandé.
pub fn convert(pkg: &Package, elf: &ElfInfo, opts: &ConvertOptions, sys: &SystemInfo) -> Result<ConvertResult> {
    let fam = opts.distro.family(opts.format, sys);
    let mut meta = pkg.meta.clone();
    apply_override(&mut meta, &opts.meta);
    if meta.name.trim().is_empty() {
        bail!("err.empty_name");
    }
    let mut warnings = Vec::new();
    let mut tree: Tree = pkg.tree.clone_shallow();
    if opts.usrmerge {
        fsdirs::apply_usrmerge(&mut tree, fam);
    }

    // Bibliothèques rangées hors des dossiers connus de la cible → ld.so.conf.d
    let lib_dirs = if opts.format == Format::Sh {
        let mut all = Vec::new();
        for f in [Family::Debian, Family::Fedora, Family::Arch] {
            for d in fsdirs::extra_lib_dirs(&tree, f, &meta.arch) {
                if !all.contains(&d) {
                    all.push(d);
                }
            }
        }
        all
    } else {
        fsdirs::extra_lib_dirs(&tree, fam, &meta.arch)
    };
    if !lib_dirs.is_empty() {
        let conf: String = lib_dirs.iter().map(|d| format!("/{d}\n")).collect();
        let name = format!("etc/ld.so.conf.d/tamga-{}.conf", version::deb_name(&meta.name));
        tree.add_file_bytes(&name, conf.as_bytes(), 0o644, crate::tarutil::now())?;
        warnings.push("warn.ld_conf_added".into());
    }
    let has_libs = tree.entries.keys().any(|k| fsdirs::looks_like_shared_lib(k) && !k.contains("/python"));
    let integ = Integration::detect(&tree, !lib_dirs.is_empty() || has_libs);

    // Dépendances
    let final_deps = match &opts.deps {
        Some(d) => deps::dedupe(d.iter().cloned()),
        None => {
            let items = resolve_deps(pkg, elf, opts.format, opts.distro, opts.online, sys);
            if deps::has_missing(&items) {
                warnings.push("warn.missing_deps".into());
            }
            deps::final_deps(&items)
        }
    };
    let same_family = meta.dep_family.map(|f| f == fam).unwrap_or(false);

    // Fichier de sortie
    let out = if opts.output.is_dir() { opts.output.join(default_file_name(&meta, opts.format)) } else { opts.output.clone() };
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    if opts.include_scripts && !meta.scripts.is_empty() {
        warnings.push("warn.scripts_included".into());
    }

    match opts.format {
        Format::Deb => {
            let new_v = version::deb_version(&meta);
            let o = deb::DebWriteOpts {
                depends: final_deps.clone(),
                provides: carry(&meta.provides, same_family, true),
                conflicts: carry(&meta.conflicts, same_family, false),
                replaces: carry(&meta.replaces, same_family, false),
                scripts: scripts::for_deb(&meta.scripts, opts.include_scripts, &new_v, &integ),
                ldconfig_trigger: integ.ldconfig,
            };
            deb::write(&meta, &tree, &o, &out)?;
        }
        Format::Rpm => {
            let (v, r) = version::rpm_version(&meta);
            let o = rpm::RpmWriteOpts {
                requires: final_deps.clone(),
                provides: carry(&meta.provides, same_family, true),
                conflicts: carry(&meta.conflicts, same_family, false),
                scripts: scripts::for_rpm(&meta.scripts, opts.include_scripts, &format!("{v}-{r}"), &integ),
                codec: crate::compress::Codec::Zstd,
            };
            rpm::write(&meta, &tree, &o, &out)?;
        }
        Format::Arch => {
            let o = pacman::PacmanWriteOpts {
                depends: final_deps.clone(),
                provides: carry(&meta.provides, same_family, true),
                conflicts: carry(&meta.conflicts, same_family, false),
                replaces: carry(&meta.replaces, same_family, false),
                install: scripts::for_pacman(&meta.scripts, opts.include_scripts),
            };
            pacman::write(&meta, &tree, &o, &out)?;
        }
        Format::Sh => {
            let libs: Vec<String> = elf.needed.keys().cloned().collect();
            let hints = libs.iter().filter_map(|l| crate::mapping::hint_line(l).map(|h| (l.clone(), h))).collect();
            let o = shell::ShWriteOpts {
                needed_libs: libs,
                hints,
                glibc: elf.glibc.clone(),
                include_scripts: opts.include_scripts,
                integration: integ.clone(),
            };
            // Le .sh garde les dépendances d'origine dans ses métadonnées (pour une reconversion).
            shell::write(&meta, &tree, &o, &out)?;
        }
    }
    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    let file_name = out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(ConvertResult {
        path: out.to_string_lossy().to_string(),
        size,
        format: opts.format,
        install_command: install_command(opts.format, fam, &file_name),
        depends: final_deps.iter().map(|d| d.display_std()).collect(),
        warnings,
    })
}

/// Petite aide pour les tests : crée un paquet en mémoire.
pub fn package_from_parts(meta: Metadata, files: &[(&str, &[u8], u32)], format: Format) -> Result<Package> {
    let mut tree = Tree::new()?;
    for (p, data, mode) in files {
        tree.add_file_bytes(p, data, *mode, 1_700_000_000)?;
    }
    let _ = Entry::dir(0o755, 0);
    Ok(Package { meta, tree, format, warnings: vec![] })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le JSON envoyé par l'interface (app.js) doit être accepté tel quel.
    #[test]
    fn options_from_ui_json() {
        let js = r#"{
            "format": "rpm", "distro": "this_system",
            "meta": {"name": "xterm", "version": "390", "release": "", "summary": "X", "maintainer": "", "license": "", "url": ""},
            "deps": [{"name": "libX11.so.6()(64bit)", "op": null, "version": null},
                     {"name": "curl", "op": "Ge", "version": "8.0"}],
            "include_scripts": false, "usrmerge": true, "online": true,
            "output": "/tmp/x.rpm"
        }"#;
        let o: ConvertOptions = serde_json::from_str(js).unwrap();
        assert_eq!(o.format, Format::Rpm);
        assert_eq!(o.distro, TargetDistro::ThisSystem);
        assert_eq!(o.deps.as_ref().unwrap()[1].display_std(), "curl >= 8.0");
        let d: TargetDistro = serde_json::from_str("\"suse\"").unwrap();
        assert_eq!(d, TargetDistro::Suse);
    }

    #[test]
    fn file_names() {
        let m = Metadata { name: "My App".into(), version: "1.2".into(), release: "3".into(), arch: "x86_64".into(), epoch: Some(2), ..Default::default() };
        assert_eq!(default_file_name(&m, Format::Deb), "my-app_1.2-3_amd64.deb");
        assert_eq!(default_file_name(&m, Format::Rpm), "My-App-1.2-3.x86_64.rpm");
        assert_eq!(default_file_name(&m, Format::Arch), "my-app-2:1.2-3-x86_64.pkg.tar.zst");
        assert_eq!(default_file_name(&m, Format::Sh), "my-app-1.2-3-x86_64.sh");
    }
}
