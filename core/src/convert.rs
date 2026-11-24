//! Point d'entrée : analyser un fichier, puis le convertir.
//! High-level API: load/analyze a file, then convert it.

use crate::bundle::{self, BundledLib};
use crate::deps::{self, DepItem, ResolveCtx, TargetDistro};
use crate::repos::Progress;
use crate::detect::{self, Detected};
use crate::distro::SystemInfo;
use crate::elf::{self, ElfInfo};
use crate::fsdirs;
use crate::model::{Dependency, Family, Format, Metadata, Package};
use crate::scripts::{self, Integration};
use crate::tree::{Entry, Tree};
use crate::compress::Codec;
use crate::{deb, pacman, rpm, shell, slack, slp, svr4, version};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Charge un paquet, quel que soit son format.
/// Les erreurs « connues » commencent par « err. » (clé de traduction de l'interface).
pub fn load(path: &Path) -> Result<Package> {
    if path.is_dir() {
        crate::vlog!("analyse", "lecture du dossier {}", path.display());
        return crate::builddir::read(path);
    }
    let det = detect::detect(path);
    crate::vlog!("analyse", "lecture de {} ({}) — format détecté : {:?}", path.display(), crate::journal::size(std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)), det);
    match det {
        Detected::Package(Format::Deb) => deb::read(path),
        Detected::Package(Format::Rpm) => rpm::read(path),
        Detected::Package(Format::Arch) => pacman::read(path),
        Detected::Package(Format::Sh) => shell::read(path),
        Detected::Package(Format::Slack) => slack::read(path, true),
        Detected::Package(Format::Tar) => slack::read(path, false),
        Detected::Package(Format::Slp) => slp::read(path),
        Detected::Package(Format::Solaris) => svr4::read(path),
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
    /// Ouvert depuis un dossier de construction (voir `builddir`).
    #[serde(default)]
    pub is_folder: bool,
    /// Format conseillé (dossier de construction).
    #[serde(default)]
    pub suggested: Option<Format>,
}

pub fn analyze(pkg: &Package, path: &Path) -> (Analysis, ElfInfo) {
    let mut elf = elf::scan(&pkg.tree);
    if pkg.format == Format::Solaris {
        // Bibliothèques Solaris : sans équivalent sous Linux.
        elf.needed.clear();
        elf.glibc = None;
        elf.glibcxx = None;
    }
    let mut warnings = pkg.warnings.clone();
    if let (Some(m), false) = (&elf.machine, pkg.meta.arch == "noarch" || pkg.format == Format::Solaris) {
        if *m != pkg.meta.arch {
            warnings.push("warn.arch_mismatch".into());
        }
    }
    if pkg.meta.arch == "noarch" && elf.elf_files > 0 {
        warnings.push("warn.noarch_binaries".into());
    }
    if crate::journal::on() {
        let m = &pkg.meta;
        crate::journal::write(
            "analyse",
            &format!(
                "{} {} {} ({}) — {} fichiers, {} programmes ELF{}\nbibliothèques utilisées : {}\nbibliothèques fournies : {}\ninterpréteurs : {}\ndépendances déclarées : {}{}",
                m.name,
                m.version,
                m.release,
                m.arch,
                pkg.tree.file_count(),
                elf.elf_files,
                elf.glibc.as_ref().map(|g| format!(", glibc ≥ {g}")).unwrap_or_default(),
                if elf.needed.is_empty() { "aucune".into() } else { elf.needed.keys().cloned().collect::<Vec<_>>().join(", ") },
                if elf.provided.is_empty() { "aucune".into() } else { elf.provided.iter().cloned().collect::<Vec<_>>().join(", ") },
                if elf.interpreters.is_empty() { "aucun".into() } else { elf.interpreters.iter().cloned().collect::<Vec<_>>().join(", ") },
                m.depends.iter().map(|d| d.display()).collect::<Vec<_>>().join(", "),
                if warnings.is_empty() { String::new() } else { format!("\navertissements : {}", warnings.join(", ")) }
            ),
        );
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
        targets: if path.is_dir() { Format::all().to_vec() } else { Format::all().into_iter().filter(|f| *f != pkg.format).collect() },
        is_folder: path.is_dir(),
        suggested: if path.is_dir() { pkg.meta.origin_format } else { None },
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
    /// Architecture du paquet créé (x86_64, aarch64, i686, armv7, noarch…).
    #[serde(default)]
    pub arch: Option<String>,
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
    /// Compression des archives tar et Slackware : gz, xz, bz2, zst, none.
    #[serde(default)]
    pub compression: Option<String>,
    /// Fournir avec le paquet les bibliothèques absentes de la cible (en ligne).
    #[serde(default = "yes")]
    pub bundle: bool,
    /// Bibliothèques à fournir, choisies dans l'interface (avec `deps`).
    #[serde(default)]
    pub bundled: Option<Vec<BundledLib>>,
    /// Remettre des droits « propres » sur les fichiers (comme un paquet officiel).
    #[serde(default)]
    pub fixperms: bool,
    /// Écrire un dossier de construction au lieu du paquet (dans `output`).
    #[serde(default)]
    pub as_folder: bool,
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
    /// Bibliothèques fournies avec le paquet : « libfoo.so.1 ← Ubuntu 22.04 · libfoo1 1.0 ».
    #[serde(default)]
    pub bundled: Vec<String>,
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
    if let Some(a) = o.arch.as_deref().map(str::trim).filter(|a| !a.is_empty() && *a != "auto") {
        meta.arch = crate::version::canonical_arch(a);
    }
}

/// Compression utilisée par défaut pour les formats à base de tar.
pub fn default_codec(format: Format) -> Codec {
    match format {
        Format::Slack => Codec::Xz,
        Format::Slp => Codec::Bzip2,
        _ => Codec::Gzip,
    }
}

/// Compression choisie (ou par défaut), limitée à ce que le format accepte.
pub fn codec_for(format: Format, choice: Option<&str>) -> Codec {
    let c = choice.and_then(Codec::parse).unwrap_or_else(|| default_codec(format));
    match format {
        Format::Slack if matches!(c, Codec::Gzip | Codec::Xz | Codec::Bzip2) => c,
        Format::Slack => Codec::Xz,
        Format::Tar if c.writable() => c,
        Format::Tar => Codec::Gzip,
        _ => default_codec(format),
    }
}

fn no_epoch(v: String) -> String {
    v.split_once(':').map(|(_, r)| r.to_string()).unwrap_or(v)
}

/// Nom de fichier conseillé pour la cible.
pub fn default_file_name(meta: &Metadata, format: Format) -> String {
    default_file_name_with(meta, format, None)
}

pub fn default_file_name_with(meta: &Metadata, format: Format, compression: Option<&str>) -> String {
    match format {
        Format::Deb => {
            let v = no_epoch(version::deb_version(meta));
            format!("{}_{}_{}.deb", version::deb_name(&meta.name), v, version::deb_arch(&meta.arch))
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
            let v = no_epoch(version::deb_version(meta));
            format!("{}-{}-{}.sh", version::deb_name(&meta.name), v, version::canonical_arch(&meta.arch))
        }
        Format::Slack => {
            // nom-version-arch-build : le « - » sépare les champs, il est interdit dans la version.
            let (v, r) = version::pacman_version(meta);
            let codec = codec_for(format, compression);
            format!("{}-{}-{}-{}{}", version::pacman_name(&meta.name), v, slack::slack_arch(&meta.arch), r, slack::slack_ext(codec))
        }
        Format::Tar => {
            let v = no_epoch(version::deb_version(meta));
            let codec = codec_for(format, compression);
            format!("{}-{}-{}{}", version::deb_name(&meta.name), v, version::canonical_arch(&meta.arch), slack::tar_ext(codec))
        }
        Format::Slp => format!("{}-{}.slp", version::deb_name(&meta.name), meta.version.replace(' ', "_")),
        Format::Solaris => format!("{}-{}.pkg", meta.name, meta.version),
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
        Format::Slack => format!("sudo upgradepkg --install-new {f}"),
        Format::Tar => format!("sudo tar -xpf {f} -C /"),
        Format::Slp => format!("sudo slpi {f}"),
        Format::Solaris => format!("pkgadd -d {f}"),
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
/// Avec `bundle` (et en ligne), les bibliothèques absentes de la cible sont cherchées dans
/// les dépôts de la distribution d'origine pour être fournies avec le paquet.
pub fn resolve_deps(
    pkg: &Package,
    elf: &ElfInfo,
    format: Format,
    distro: TargetDistro,
    online: bool,
    bundle_libs: bool,
    sys: &SystemInfo,
    progress: Option<Progress>,
) -> Vec<DepItem> {
    let ctx = ResolveCtx { format, distro, online, sys };
    let mut items = deps::resolve(&pkg.meta, elf, &ctx);
    if bundle_libs && online {
        let progress = progress.unwrap_or_else(|| std::sync::Arc::new(|_: &str, _: &str| {}));
        bundle::bundle_missing(&pkg.meta, elf, &mut items, &ctx, progress);
    }
    if format.is_generic() {
        // Formats sans dépendances déclarées : l'installateur .sh (ou l'utilisateur) vérifie
        // sur la machine cible ; une bibliothèque absente d'ici ne bloque pas la conversion.
        for it in items.iter_mut().filter(|i| i.status == deps::Status::Missing && i.origin == deps::Origin::Library) {
            it.status = deps::Status::Estimated;
        }
    }
    items
}

/// Convertit le paquet chargé vers le format demandé.
pub fn convert(pkg: &Package, elf: &ElfInfo, opts: &ConvertOptions, sys: &SystemInfo) -> Result<ConvertResult> {
    convert_with_progress(pkg, elf, opts, sys, None)
}

pub fn convert_with_progress(pkg: &Package, elf: &ElfInfo, opts: &ConvertOptions, sys: &SystemInfo, progress: Option<Progress>) -> Result<ConvertResult> {
    let fam = opts.distro.family(opts.format, sys);
    let mut meta = pkg.meta.clone();
    apply_override(&mut meta, &opts.meta);
    if meta.name.trim().is_empty() {
        bail!("err.empty_name");
    }
    // Archive sans informations : un résumé et une description plutôt que des champs vides.
    let origin_label = meta.origin_format.or(Some(pkg.format)).map(|f| f.label().to_string()).unwrap_or_default();
    if meta.summary.trim().is_empty() || meta.summary.trim() == meta.name.trim() {
        meta.summary = format!("Converted by TAMGA from a {origin_label} file");
    }
    if meta.description.trim().is_empty() || meta.description.trim() == meta.name.trim() {
        meta.description = format!(
            "This package was created by TAMGA from a {origin_label} file.\nIts files are those of the original archive.\nEdit the package information in TAMGA to describe it better."
        );
    }
    let started = std::time::Instant::now();
    crate::vlog!(
        "conversion",
        "{} {} → {:?} pour {:?} (famille {:?}) ; scripts : {}, fusion /usr : {}, en ligne : {}, bibliothèques fournies : {}",
        meta.name,
        meta.version,
        opts.format,
        opts.distro,
        fam,
        opts.include_scripts,
        opts.usrmerge,
        opts.online,
        opts.bundle
    );
    let mut warnings = Vec::new();

    // Dépendances (et bibliothèques à fournir avec le paquet)
    let (final_deps, bundled): (Vec<Dependency>, Vec<BundledLib>) = match &opts.deps {
        Some(d) => (deps::dedupe(d.iter().cloned()), if opts.bundle { opts.bundled.clone().unwrap_or_default() } else { vec![] }),
        None => {
            let items = resolve_deps(pkg, elf, opts.format, opts.distro, opts.online, opts.bundle, sys, progress);
            if deps::has_missing(&items) {
                warnings.push("warn.missing_deps".into());
            }
            (deps::final_deps(&items), items.iter().filter_map(|i| i.bundle.clone()).collect())
        }
    };

    let mut tree: Tree = pkg.tree.clone_shallow();
    if opts.usrmerge {
        let moved = fsdirs::apply_usrmerge(&mut tree, fam);
        crate::vlog!("conversion", "fusion /usr : {moved} entrées déplacées (bin → usr/bin, lib → usr/lib…)");
    }
    // Liens de documentation vers un autre paquet (habitude Debian) : cassés ailleurs, on les retire.
    let dangling: Vec<String> = tree
        .entries
        .iter()
        .filter(|(p, e)| p.starts_with("usr/share/doc/") && matches!(&e.kind, crate::tree::Kind::Symlink(t) if !t.starts_with('/') && t.starts_with("..")))
        .filter(|(p, e)| {
            let crate::tree::Kind::Symlink(t) = &e.kind else { return false };
            let joined = format!("{}/{}", crate::tree::parent(p).unwrap_or(""), t);
            crate::tree::normalize(&joined).map(|n| tree.get(&n).is_none()).unwrap_or(true)
        })
        .map(|(p, _)| p.clone())
        .collect();
    for p in &dangling {
        tree.entries.remove(p);
        crate::vlog!("conversion", "lien de documentation cassé retiré : /{p}");
    }
    // Bibliothèques absentes de la cible, rangées dans un dossier privé.
    let applied = bundle::apply(&mut tree, &meta, &bundled)?;
    if opts.fixperms {
        let n = fsdirs::fix_permissions(&mut tree, fam);
        crate::vlog!("conversion", "droits corrigés sur {n} fichiers et dossiers");
        if n > 0 {
            warnings.push("warn.fixperms_done".into());
        }
    }
    // Architecture choisie différente de celle des programmes du paquet.
    if let Some(m) = &elf.machine {
        let a = crate::version::canonical_arch(&meta.arch);
        if a != "noarch" && &a != m && pkg.meta.arch != meta.arch {
            warnings.push("warn.arch_forced".into());
        }
        if a == "noarch" && pkg.meta.arch != "noarch" {
            warnings.push("warn.noarch_binaries".into());
        }
    }

    // Bibliothèques rangées hors des dossiers connus de la cible → ld.so.conf.d
    let lib_dirs = if opts.format.is_generic() {
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
    let mut lib_dirs = lib_dirs;
    if let Some(a) = &applied {
        warnings.push("warn.bundled_libs".into());
        if a.needs_ld_conf {
            lib_dirs.push(a.dir.clone());
            if opts.format == Format::Tar {
                warnings.push("warn.tar_ldconfig".into());
            }
        }
    }
    if !lib_dirs.is_empty() {
        let conf: String = lib_dirs.iter().map(|d| format!("/{d}\n")).collect();
        let name = format!("etc/ld.so.conf.d/tamga-{}.conf", version::deb_name(&meta.name));
        tree.add_file_bytes(&name, conf.as_bytes(), 0o644, crate::tarutil::now())?;
        crate::vlog!("conversion", "fichier /{name} ajouté : {}", lib_dirs.join(", "));
        warnings.push("warn.ld_conf_added".into());
    }
    let has_libs = tree.entries.keys().any(|k| fsdirs::looks_like_shared_lib(k) && !k.contains("/python"));
    let integ = Integration::detect(&tree, !lib_dirs.is_empty() || has_libs);

    let same_family = meta.dep_family.map(|f| f == fam).unwrap_or(false);

    // Dossier de construction : les fichiers et la fiche, sans fabriquer le paquet.
    if opts.as_folder {
        // Comme pour un paquet : les liens entre paquets d'une autre famille ne sont pas repris.
        let mut fm = meta.clone();
        fm.provides = carry(&meta.provides, same_family, true);
        fm.conflicts = carry(&meta.conflicts, same_family, false);
        fm.replaces = carry(&meta.replaces, same_family, false);
        let dir = crate::builddir::write(&fm, &tree, opts.format, fam, &final_deps, opts.include_scripts, &opts.output)?;
        return Ok(ConvertResult {
            path: dir.to_string_lossy().to_string(),
            size: tree.total_size(),
            format: opts.format,
            install_command: format!("tamga convert {} --to {}", dir.display(), opts.format.id()),
            depends: final_deps.iter().map(|d| d.display_std()).collect(),
            warnings: vec!["warn.folder_written".into()],
            bundled: bundled.iter().map(|b| format!("{} ← {}", b.soname, b.source)).collect(),
        });
    }

    // Fichier de sortie
    let compression = opts.compression.as_deref();
    // « dossier/ » qui n'existe pas encore : on le crée.
    let as_text = opts.output.to_string_lossy();
    if (as_text.ends_with('/') || as_text.ends_with('\\')) && !opts.output.exists() {
        std::fs::create_dir_all(&opts.output)?;
    }
    let out = if opts.output.is_dir() {
        opts.output.join(default_file_name_with(&meta, opts.format, compression))
    } else {
        opts.output.clone()
    };
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    if opts.include_scripts && !meta.scripts.is_empty() {
        warnings.push("warn.scripts_included".into());
    }
    // Finitions selon les règles de la distribution (liens, copyright, journal, licence).
    let version_text = match opts.format {
        Format::Deb => version::deb_version(&meta),
        _ => meta.version.clone(),
    };
    warnings.extend(crate::polish::polish(opts.format, &mut tree, &meta, &version_text)?);

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
            // Bibliothèques à vérifier à l'installation : celles du paquet, sauf les fournies,
            // plus celles dont les fournies ont besoin.
            let given: std::collections::BTreeSet<&str> = bundled.iter().map(|b| b.soname.as_str()).collect();
            let mut libs: Vec<String> = elf.needed.keys().filter(|l| !given.contains(l.as_str())).cloned().collect();
            for b in &bundled {
                for n in &b.needs {
                    if !given.contains(n.as_str()) && !elf.provided.contains(n) && !libs.contains(n) {
                        libs.push(n.clone());
                    }
                }
            }
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
        Format::Slack => {
            let (v, r) = version::pacman_version(&meta);
            let (links, configs) = slack::slack_layout(&tree);
            let doinst = scripts::for_slack(&meta.scripts, opts.include_scripts, &format!("{v}-{r}"), &integ, &links, &configs);
            slack::write_slack(&meta, &tree, codec_for(Format::Slack, compression), doinst.as_deref(), &out)?;
        }
        Format::Tar => {
            slack::write_tar(&tree, codec_for(Format::Tar, compression), &out)?;
            warnings.push("warn.tar_no_uninstall".into());
            if opts.include_scripts && !meta.scripts.is_empty() {
                warnings.push("warn.tar_no_scripts".into());
            }
        }
        Format::Slp => {
            if slp::write(&meta, &tree, &out)? {
                warnings.push("warn.slp_arch".into());
            }
        }
        Format::Solaris => bail!("err.solaris_write"),
    }
    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    let file_name = out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    crate::vlog!(
        "conversion",
        "écrit : {} ({}) en {:.1} s\ndépendances : {}{}",
        out.display(),
        crate::journal::size(size),
        started.elapsed().as_secs_f64(),
        if final_deps.is_empty() { "aucune".into() } else { final_deps.iter().map(|d| d.display_std()).collect::<Vec<_>>().join(", ") },
        if warnings.is_empty() { String::new() } else { format!("\navertissements : {}", warnings.join(", ")) }
    );
    Ok(ConvertResult {
        path: out.to_string_lossy().to_string(),
        size,
        format: opts.format,
        install_command: install_command(opts.format, fam, &file_name),
        depends: final_deps.iter().map(|d| d.display_std()).collect(),
        warnings,
        bundled: if applied.is_some() { bundled.iter().map(|b| format!("{} ← {}", b.soname, b.source)).collect() } else { vec![] },
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
        assert_eq!(default_file_name(&m, Format::Slack), "my-app-1.2-x86_64-3.txz");
        assert_eq!(default_file_name_with(&m, Format::Slack, Some("gz")), "my-app-1.2-x86_64-3.tgz");
        assert_eq!(default_file_name(&m, Format::Tar), "my-app-1.2-3-x86_64.tar.gz");
        assert_eq!(default_file_name_with(&m, Format::Tar, Some("zst")), "my-app-1.2-3-x86_64.tar.zst");
        assert_eq!(default_file_name(&m, Format::Slp), "my-app-1.2.slp");
    }
}
