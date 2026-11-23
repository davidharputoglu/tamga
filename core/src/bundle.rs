//! Bibliothèques manquantes fournies avec le paquet converti.
//! Missing shared libraries bundled into the converted package.
//!
//! Quand une bibliothèque utilisée par les programmes n'existe pas sur la distribution
//! cible, TAMGA la cherche dans les dépôts officiels de la distribution d'origine du
//! paquet (voir `repos`), l'extrait et la range dans un dossier privé :
//! `/usr/lib/tamga/<paquet>/`. Les programmes du paquet sont lancés par un petit script
//! qui ajoute ce dossier à LD_LIBRARY_PATH : le reste du système n'est pas touché.
//! Ce que ces bibliothèques demandent à leur tour est vérifié de la même façon.

use crate::deps::{self, DepItem, Origin, ResolveCtx, Status, TargetDistro};
use crate::elf::{self, ElfInfo};
use crate::model::{Dependency, Family, Format, Metadata, Op};
use crate::repos::{Fetcher, Progress, Repo, RepoKind, RepoPackage};
use crate::tree::{basename, parent, Kind, Tree};
use crate::{convert, mapping, version};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Bibliothèques qui font partie de la glibc : on ne peut jamais les fournir à part.
const NEVER_BUNDLE: &[&str] = &[
    "libc.so.6", "libm.so.6", "libpthread.so.0", "libdl.so.2", "librt.so.1", "libutil.so.1", "libresolv.so.2",
    "libanl.so.1", "libnsl.so.1", "libmvec.so.1", "libBrokenLocale.so.1", "libcrypt.so.1", "libthread_db.so.1",
    "libGL.so.1", "libEGL.so.1", "libGLX.so.0", "libGLdispatch.so.0", "libvulkan.so.1", "libcuda.so.1",
    "libnvidia-ml.so.1", "libdrm.so.2", "libgbm.so.1",
];

/// Nombre maximal de bibliothèques fournies avec un paquet.
const MAX_BUNDLED: usize = 80;

/// Une bibliothèque fournie avec le paquet.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BundledLib {
    pub soname: String,
    /// Copie de la bibliothèque dans le cache de TAMGA.
    pub path: String,
    /// Provenance lisible : « Ubuntu 22.04 (jammy/main) · libicu70 70.1-2 ».
    pub source: String,
    /// Paquet d'origine.
    pub package: String,
    pub size: u64,
    /// Bibliothèques dont elle a besoin.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Fichiers de licence du paquet d'origine (copiés en cache).
    #[serde(default)]
    pub licenses: Vec<String>,
}

/// Dossier du cache où sont gardées les bibliothèques extraites.
pub fn libs_cache() -> PathBuf {
    deps::cache_dir().unwrap_or_else(|| std::env::temp_dir().join("tamga-cache")).join("libs")
}

/// Dossier privé du paquet converti.
pub fn bundle_dir(meta: &Metadata) -> String {
    format!("usr/lib/tamga/{}", version::deb_name(&meta.name))
}

pub fn can_bundle(soname: &str) -> bool {
    !NEVER_BUNDLE.contains(&soname) && !soname.starts_with("ld-linux") && !soname.starts_with("ld64.so") && soname.contains(".so")
}

fn norm(s: &str) -> String {
    s.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// Noms de paquets candidats (Debian, Arch) pour une bibliothèque, du plus probable au moins probable.
/// `versioned_names` : les noms de paquets contiennent le numéro de la bibliothèque (Debian),
/// contrairement à Arch (« libjpeg-turbo » fournit libjpeg.so.8).
fn name_candidates(soname: &str, hints: &[String], index: &crate::repos::RepoIndex, versioned_names: bool) -> Vec<RepoPackage> {
    let stem = soname.split(".so").next().unwrap_or(soname);
    let major = soname.split_once(".so.").map(|(_, v)| v.split('.').next().unwrap_or(v).to_string()).unwrap_or_default();
    let nstem = norm(stem);
    let need = nstem.len().min(6).max(4);
    let plausible = |name: &str| -> bool {
        let n = norm(name);
        common_prefix(&n, &nstem) >= need && (major.is_empty() || n.contains(&major))
    };
    let mut names: Vec<String> = Vec::new();
    if let Some(c) = mapping::debian_convention(soname) {
        names.push(format!("{c}t64"));
        names.push(c);
    }
    for h in hints {
        let n = norm(h);
        if plausible(h) || (!versioned_names && common_prefix(&n, &nstem) >= need) {
            names.push(h.clone());
        }
    }
    let mut out: Vec<RepoPackage> = Vec::new();
    for n in names {
        if let Some(p) = index.get(&n) {
            if !out.iter().any(|o| o.name == p.name) {
                out.push(p.clone());
            }
        }
    }
    // Secours : paquets dont le nom ressemble à celui de la bibliothèque.
    let prefix: String = stem.chars().take(stem.len().min(6)).collect::<String>().to_ascii_lowercase();
    for p in index.names_starting_with(&prefix) {
        if out.len() >= 5 {
            break;
        }
        let n = p.name.as_str();
        let secondary = ["-dev", "-dbg", "-doc", "-bin", "-tools", "-utils", "-devel", "-static", "-dbgsym"].iter().any(|s| n.ends_with(s));
        if !secondary && plausible(n) && !out.iter().any(|o| o.name == p.name) {
            out.push(p.clone());
        }
    }
    out
}

/// Retrouve la bibliothèque `soname` dans un paquet téléchargé et la copie dans le cache.
fn extract_lib(pkg_file: &Path, p: &RepoPackage, soname: &str, machine: &str, is64: bool) -> Result<(PathBuf, Vec<String>)> {
    let pkg = convert::load(pkg_file)?;
    let tree = &pkg.tree;
    // Le fichier (ou le lien) qui porte ce nom, de préférence dans un dossier de bibliothèques.
    let mut cands: Vec<&String> = tree.entries.keys().filter(|k| basename(k) == soname).collect();
    cands.sort_by_key(|k| (!k.contains("lib"), k.len()));
    let Some(start) = cands.first() else { bail!("{soname} absent de {}", p.name) };
    let mut cur = (*start).clone();
    let mut hops = 0;
    let blob = loop {
        match tree.get(&cur).map(|e| &e.kind) {
            Some(Kind::File(b)) => break b.clone(),
            Some(Kind::Symlink(t)) if hops < 8 => {
                hops += 1;
                let joined = if t.starts_with('/') { t.trim_start_matches('/').to_string() } else { format!("{}/{}", parent(&cur).unwrap_or(""), t) };
                cur = crate::tree::normalize(&joined).unwrap_or_default();
            }
            _ => bail!("{soname} : lien cassé dans {}", p.name),
        }
    };
    let data = tree.store.read_all(&blob)?;
    let parsed = goblin::elf::Elf::parse(&data).map_err(|e| anyhow::anyhow!("{soname} n'est pas un ELF : {e}"))?;
    if parsed.is_64 != is64 || elf::machine_of(parsed.header.e_machine).map(|m| m != machine).unwrap_or(true) {
        bail!("{soname} : mauvaise architecture dans {}", p.name);
    }
    let dir = libs_cache().join(format!("{}-{}", safe(&p.name), safe(&p.version)));
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(soname);
    std::fs::write(&dest, &data)?;
    // Licences du paquet d'origine (Debian : copyright ; rpm et Arch : usr/share/licenses).
    let mut licenses = Vec::new();
    let lic_dir = dir.join("licenses");
    for (k, e) in tree.entries.iter() {
        let Kind::File(b) = &e.kind else { continue };
        let is_lic = (k.starts_with("usr/share/doc/") && (k.ends_with("/copyright") || basename(k).to_ascii_uppercase().starts_with("LICENSE") || basename(k).to_ascii_uppercase().starts_with("COPYING")))
            || k.starts_with("usr/share/licenses/");
        if is_lic && b.size < 1024 * 1024 && licenses.len() < 12 {
            std::fs::create_dir_all(&lic_dir)?;
            let name = k.trim_start_matches("usr/share/").replace('/', "_");
            let d = lic_dir.join(safe(&name));
            std::fs::write(&d, tree.store.read_all(b)?)?;
            licenses.push(d.to_string_lossy().to_string());
        }
    }
    Ok((dest, licenses))
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || "._-+".contains(c) { c } else { '_' }).collect()
}

/// La copie en cache est-elle bien une bibliothèque de la bonne architecture ?
fn check_elf(path: &Path, machine: &str, is64: bool) -> bool {
    let Ok(data) = std::fs::read(path) else { return false };
    match goblin::elf::Elf::parse(&data) {
        Ok(e) => e.is_64 == is64 && elf::machine_of(e.header.e_machine) == Some(machine),
        Err(_) => false,
    }
}

/// Analyse ELF d'une bibliothèque seule.
pub fn scan_lib(path: &Path) -> Result<ElfInfo> {
    let mut t = Tree::new()?;
    let data = std::fs::read(path)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "lib.so".into());
    t.add_file_bytes(&format!("usr/lib/tamga-scan/{name}"), &data, 0o755, 0)?;
    Ok(elf::scan(&t))
}

/// Cherche `soname` dans les dépôts, dans l'ordre. `target_glibc` : version de la glibc
/// de la cible, si on la connaît (les bibliothèques plus exigeantes sont écartées).
pub fn find_library(
    f: &mut Fetcher,
    repos: &[Repo],
    soname: &str,
    is64: bool,
    machine: &str,
    hints: &[String],
    target_glibc: Option<&str>,
) -> Option<(BundledLib, ElfInfo)> {
    // Deux passes : d'abord les versions de distribution compatibles avec la glibc cible.
    for pass in 0..2 {
        for repo in repos {
            if f.offline() {
                return None;
            }
            let compatible = match (target_glibc, repo.glibc.as_deref()) {
                (Some(t), Some(g)) => elf::version_cmp(g, t).is_le(),
                _ => true,
            };
            if (pass == 0) != compatible {
                continue;
            }
            let Some(index) = f.index(repo) else { continue };
            let mut cands: Vec<RepoPackage> = index.providers(soname, is64).into_iter().cloned().collect();
            crate::vlog!("bibliothèques", "{soname} : {} candidat(s) dans {} par la liste des bibliothèques", cands.len(), repo.label);
            if matches!(repo.kind, RepoKind::Deb { .. } | RepoKind::Pacman { .. }) {
                for c in name_candidates(soname, hints, &index, matches!(repo.kind, RepoKind::Deb { .. })) {
                    if !cands.iter().any(|x| x.name == c.name) {
                        cands.push(c);
                    }
                }
            }
            if !cands.is_empty() {
                crate::vlog!("bibliothèques", "{soname} : essai de {}", cands.iter().take(4).map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "));
            }
            for p in cands.iter().take(4) {
                if p.size > 300 * 1024 * 1024 {
                    continue;
                }
                // Déjà extraite (même paquet, autre bibliothèque, ou analyse précédente) ?
                let dir = libs_cache().join(format!("{}-{}", safe(&p.name), safe(&p.version)));
                let (path, licenses) = if dir.join(soname).is_file() && check_elf(&dir.join(soname), machine, is64) {
                    let lic = std::fs::read_dir(dir.join("licenses"))
                        .map(|rd| rd.flatten().map(|e| e.path().to_string_lossy().to_string()).collect())
                        .unwrap_or_default();
                    (dir.join(soname), lic)
                } else {
                    let Ok(file) = f.package(p, &repo.label) else { continue };
                    match extract_lib(&file, p, soname, machine, is64) {
                        Ok(r) => r,
                        Err(e) => {
                            crate::vlog!("bibliothèques", "{soname} : pas dans {} ({e:#})", p.name);
                            continue;
                        }
                    }
                };
                let Ok(info) = scan_lib(&path) else { continue };
                if let (Some(t), Some(g)) = (target_glibc, info.glibc.as_deref()) {
                    if elf::version_cmp(g, t).is_gt() {
                        crate::vlog!("bibliothèques", "{soname} de {} demande glibc {g}, la cible a {t} : écartée", p.name);
                        continue; // trop récente pour la cible
                    }
                }
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let lib = BundledLib {
                    soname: soname.to_string(),
                    path: path.to_string_lossy().to_string(),
                    source: format!("{} · {} {}", repo.label, p.name, p.version),
                    package: p.name.clone(),
                    size,
                    needs: info.needed.keys().cloned().collect(),
                    licenses,
                };
                return Some((lib, info));
            }
        }
    }
    None
}

/// Nom de la bibliothèque d'une ligne du tableau (« libfoo.so.1 (FOO_1.0) » → « libfoo.so.1 »).
fn item_soname(it: &DepItem) -> &str {
    it.source.split(" (").next().unwrap_or(&it.source)
}

/// Glibc de la cible, quand on la connaît (cette machine).
fn target_glibc(ctx: &ResolveCtx) -> Option<String> {
    if ctx.distro == TargetDistro::ThisSystem {
        ctx.sys.glibc.clone()
    } else {
        None
    }
}

/// Complète la résolution : cherche les bibliothèques introuvables sur la cible et les
/// ajoute au paquet. Les lignes concernées passent au statut « fournie avec le paquet ».
pub fn bundle_missing(meta: &Metadata, elf_info: &ElfInfo, items: &mut Vec<DepItem>, ctx: &ResolveCtx, progress: Progress) -> Vec<BundledLib> {
    let mut bundled: Vec<BundledLib> = Vec::new();
    if !ctx.online || ctx.format == Format::Solaris {
        return bundled;
    }
    let queue_of = |items: &[DepItem], done: &BTreeSet<String>| -> Vec<String> {
        let mut q: Vec<String> = items
            .iter()
            .filter(|i| i.origin == Origin::Library && i.status == Status::Missing)
            .map(|i| item_soname(i).to_string())
            .filter(|s| can_bundle(s) && !done.contains(s))
            .collect();
        q.dedup();
        q
    };
    let mut done: BTreeSet<String> = BTreeSet::new();
    let machine = elf_info.machine.clone().unwrap_or_else(|| version::canonical_arch(&meta.arch));
    let is64 = elf_info.is64;
    let mut fetcher = Fetcher::new(Arc::clone(&progress));
    // Cible distante (Fedora, openSUSE, Debian/Ubuntu choisie dans la liste) : on vérifie
    // dans ses dépôts que les bibliothèques y existent.
    let mut remote = RemoteTarget::new(ctx, &machine);
    if let Some(r) = remote.as_mut() {
        r.mark(items, 0, &mut fetcher, is64);
    }
    let mut queue = queue_of(items, &done);
    if queue.is_empty() {
        return bundled;
    }
    let family = meta.dep_family.unwrap_or(Family::Generic);
    let repos = crate::repos::repos_for(family, &meta.version, &meta.release, &meta.maintainer, &machine, elf_info.glibc.as_deref());
    crate::vlog!("bibliothèques", "à chercher : {} — distribution d'origine {:?}, {} dépôt(s) possibles", queue.join(", "), family, repos.len());
    if repos.is_empty() {
        return bundled;
    }
    let tg = target_glibc(ctx);
    let hints: Vec<String> = meta.depends.iter().flat_map(|d| std::iter::once(d.name.clone()).chain(d.alternatives.iter().map(|a| a.name.clone()))).collect();
    let mut known: BTreeSet<String> = elf_info.needed.keys().cloned().collect();
    known.extend(elf_info.provided.iter().cloned());
    let mut max_glibc = elf_info.glibc.clone();
    let mut extra_libc_versions: BTreeSet<String> = BTreeSet::new();

    let mut rounds = 0;
    while !queue.is_empty() && rounds < 8 && bundled.len() < MAX_BUNDLED {
        rounds += 1;
        let mut new_needed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut needed_by: BTreeMap<String, String> = BTreeMap::new();
        for soname in std::mem::take(&mut queue) {
            if !done.insert(soname.clone()) {
                continue;
            }
            progress("bundle.search", &soname);
            let Some((lib, info)) = find_library(&mut fetcher, &repos, &soname, is64, &machine, &hints, tg.as_deref()) else {
                crate::vlog!("bibliothèques", "{soname} : introuvable dans les dépôts d'origine");
                continue;
            };
            crate::vlog!(
                "bibliothèques",
                "{soname} fournie avec le paquet ← {} ({}) ; elle a besoin de : {}",
                lib.source,
                crate::journal::size(lib.size),
                if lib.needs.is_empty() { "rien".to_string() } else { lib.needs.join(", ") }
            );
            for it in items.iter_mut().filter(|i| i.origin == Origin::Library && item_soname(i) == soname) {
                it.status = Status::Bundled;
                it.method = "bundled".into();
                it.detail = Some(lib.source.clone());
                it.keep = false;
                if !it.source.contains(" (") {
                    it.bundle = Some(lib.clone());
                }
            }
            if let Some(g) = &info.glibc {
                if max_glibc.as_deref().map(|m| elf::version_cmp(g, m).is_gt()).unwrap_or(true) {
                    max_glibc = Some(g.clone());
                }
            }
            for (n, vers) in &info.needed {
                if n == "libc.so.6" {
                    extra_libc_versions.extend(vers.iter().filter(|v| v.starts_with("GLIBC_")).cloned());
                }
                if known.contains(n) || done.contains(n) || bundled.iter().any(|b| &b.soname == n) || n == &soname {
                    continue;
                }
                known.insert(n.clone());
                new_needed.insert(n.clone(), vers.clone());
                needed_by.entry(n.clone()).or_insert_with(|| soname.clone());
            }
            bundled.push(lib);
        }
        // Ce dont les bibliothèques ajoutées ont besoin : dépendance normale si la cible
        // l'a, sinon fournie elle aussi (tour suivant).
        if !new_needed.is_empty() {
            let mini = ElfInfo { elf_files: 1, is64, machine: Some(machine.clone()), needed: new_needed, ..Default::default() };
            let mini_meta = Metadata { name: meta.name.clone(), dep_family: meta.dep_family, ..Default::default() };
            let start = items.len();
            for mut it in deps::resolve(&mini_meta, &mini, ctx) {
                if it.origin != Origin::Library {
                    continue;
                }
                it.needed_by = needed_by.get(item_soname(&it)).cloned();
                items.push(it);
            }
            if let Some(r) = remote.as_mut() {
                r.mark(items, start, &mut fetcher, is64);
            }
        }
        queue = queue_of(items, &done);
    }

    // Dépendances déclarées qui désignent une bibliothèque désormais fournie
    // (« libicu70 », « libicuuc.so.70()(64bit) ») : elles ne sont plus nécessaires.
    for it in items.iter_mut().filter(|i| i.origin == Origin::Declared && i.status == Status::Missing) {
        let Some(name) = it.target.as_ref().map(|t| t.name.clone()).or_else(|| it.source.split_whitespace().next().map(String::from)) else { continue };
        if let Some(lib) = bundled.iter().find(|b| covers(&name, b)) {
            it.status = Status::Bundled;
            it.method = "bundled".into();
            it.detail = Some(lib.soname.clone());
            it.keep = false;
        }
    }

    // glibc demandée par les bibliothèques ajoutées.
    if let (Some(g), false) = (&max_glibc, bundled.is_empty()) {
        match ctx.format {
            Format::Deb | Format::Arch => {
                if let Some(it) = items.iter_mut().find(|i| i.origin == Origin::Glibc) {
                    let cur = it.target.as_ref().and_then(|t| t.version.clone());
                    if cur.as_deref().map(|c| elf::version_cmp(g, c).is_gt()).unwrap_or(true) {
                        let pkg = it.target.as_ref().map(|t| t.name.clone()).unwrap_or_else(|| "libc6".into());
                        it.target = Some(Dependency::with(&pkg, Op::Ge, g));
                        it.source = format!("glibc ≥ {g}");
                        if let (Some(local), true) = (ctx.sys.glibc.as_deref(), it.status == Status::Installed) {
                            if elf::version_cmp(local, g).is_lt() {
                                it.status = Status::Missing;
                            }
                        }
                    }
                }
            }
            Format::Rpm => {
                let have: BTreeSet<String> = elf_info.needed.get("libc.so.6").cloned().unwrap_or_default();
                let mut all = have.clone();
                all.extend(extra_libc_versions.iter().cloned());
                for v in elf::max_symbol_versions(&all) {
                    if have.contains(&v) {
                        continue;
                    }
                    items.push(DepItem {
                        source: format!("libc.so.6 ({v})"),
                        origin: Origin::Library,
                        target: Some(Dependency::simple(&elf::rpm_soname("libc.so.6", Some(&v), is64))),
                        status: Status::Estimated,
                        method: "soname".into(),
                        detail: None,
                        keep: true,
                        bundle: None,
                        needed_by: Some(bundled.iter().map(|b| b.soname.clone()).next().unwrap_or_default()),
                    });
                }
            }
            _ => {}
        }
    }
    bundled
}

/// Distribution cible distante, vérifiée par ses dépôts officiels.
struct RemoteTarget {
    repos: Vec<Repo>,
    debian: bool,
}

impl RemoteTarget {
    fn new(ctx: &ResolveCtx, machine: &str) -> Option<RemoteTarget> {
        let fam = match (ctx.distro, ctx.format) {
            (TargetDistro::ThisSystem | TargetDistro::Any, _) => return None,
            (TargetDistro::Fedora, Format::Rpm) => Family::Fedora,
            (TargetDistro::Suse, Format::Rpm) => Family::Suse,
            (TargetDistro::Debian, Format::Deb) => Family::Debian,
            _ => return None,
        };
        let repos = crate::repos::target_repos(fam, machine);
        (!repos.is_empty()).then_some(RemoteTarget { repos, debian: fam == Family::Debian })
    }

    /// La bibliothèque existe-t-elle sur la cible ? `None` : impossible à savoir (hors ligne).
    fn has(&self, f: &mut Fetcher, soname: &str, name: Option<&str>, is64: bool) -> Option<bool> {
        let mut checked = false;
        for repo in &self.repos {
            let Some(idx) = f.index(repo) else { continue };
            checked = true;
            if !idx.providers(soname, is64).is_empty() {
                return Some(true);
            }
            if self.debian {
                if let Some(n) = name {
                    if idx.get(n).is_some() || idx.get(&format!("{n}t64")).is_some() {
                        return Some(true);
                    }
                }
                if !name_candidates(soname, &[], &idx, true).is_empty() {
                    return Some(true);
                }
            }
        }
        checked.then_some(false)
    }

    /// Passe à « introuvable » les bibliothèques absentes de la cible (lignes `from..`).
    fn mark(&mut self, items: &mut [DepItem], from: usize, f: &mut Fetcher, is64: bool) {
        let mut verdict: BTreeMap<String, bool> = BTreeMap::new();
        for it in items.iter_mut().skip(from) {
            if it.origin != Origin::Library || it.status != Status::Estimated {
                continue;
            }
            let so = item_soname(it).to_string();
            if !can_bundle(&so) {
                continue;
            }
            let present = match verdict.get(&so) {
                Some(v) => *v,
                None => {
                    let name = it.target.as_ref().map(|t| t.name.clone());
                    let Some(v) = self.has(f, &so, name.as_deref(), is64) else { return };
                    verdict.insert(so.clone(), v);
                    v
                }
            };
            if !present {
                crate::vlog!("dépendances", "{so} absente de la cible ({})", self.repos.iter().map(|r| r.label.as_str()).collect::<Vec<_>>().join(", "));
                it.status = Status::Missing;
                it.detail = self.repos.first().map(|r| r.label.clone());
            }
        }
    }
}

/// Cette dépendance déclarée désigne-t-elle la bibliothèque fournie ?
fn covers(dep: &str, lib: &BundledLib) -> bool {
    if dep == lib.package {
        return true;
    }
    let base = dep.split(['(', '=', ' ']).next().unwrap_or(dep);
    if base == lib.soname || format!("{base}.{}", dep.split_once('=').map(|(_, v)| v.split('-').next().unwrap_or(v)).unwrap_or("")) == lib.soname {
        return true;
    }
    mapping::debian_convention(&lib.soname).map(|c| dep == c || dep == format!("{c}t64")).unwrap_or(false)
}

/// Le chemin vient-il bien du cache de TAMGA ? (les chemins arrivent de l'interface)
fn from_cache(p: &str) -> Option<PathBuf> {
    let real = std::fs::canonicalize(p).ok()?;
    let root = std::fs::canonicalize(libs_cache()).ok()?;
    real.starts_with(&root).then_some(real)
}

/// Programme ELF dynamique (avec un chargeur), à lancer par un script d'enveloppe.
fn is_dynamic_program(tree: &Tree, path: &str) -> bool {
    let Some(e) = tree.get(path) else { return false };
    let Kind::File(b) = &e.kind else { return false };
    if e.mode & 0o111 == 0 || e.mode & 0o6000 != 0 || b.size < 64 {
        return false; // pas exécutable, ou setuid/setgid (les enveloppes n'y fonctionnent pas)
    }
    let Ok(file) = tree.store.open(b) else { return false };
    // SAFETY : fichier privé du dossier temporaire de TAMGA, non modifié pendant la lecture.
    let Ok(map) = (unsafe { memmap2::Mmap::map(&file) }) else { return false };
    if !map.starts_with(b"\x7fELF") {
        return false;
    }
    match goblin::elf::Elf::parse(&map) {
        Ok(elf) => elf.interpreter.is_some() && !crate::fsdirs::looks_like_shared_lib(path),
        Err(_) => false,
    }
}

/// Script qui lance le vrai programme avec le dossier des bibliothèques fournies.
fn wrapper_script(lib_dir: &str, real: &str) -> String {
    format!(
        "#!/bin/sh\n\
         # Créé par TAMGA : ce programme utilise des bibliothèques fournies avec le paquet,\n\
         # rangées dans {lib_dir}. Le vrai programme est {real}.\n\
         LD_LIBRARY_PATH=\"{lib_dir}${{LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}}\"\n\
         export LD_LIBRARY_PATH\n\
         if command -v bash >/dev/null 2>&1; then\n\
         \texec bash -c 'exec -a \"$0\" \"$@\"' \"$0\" \"{real}\" \"$@\"\n\
         fi\n\
         exec \"{real}\" \"$@\"\n"
    )
}

/// Résultat de l'ajout des bibliothèques dans l'arbre.
pub struct Applied {
    /// Dossier privé (sans « / » initial).
    pub dir: String,
    /// Programmes enveloppés.
    pub wrapped: usize,
    /// Aucun programme à envelopper : il faut un fichier ld.so.conf.d.
    pub needs_ld_conf: bool,
}

/// Range les bibliothèques fournies dans l'arbre du paquet et enveloppe ses programmes.
pub fn apply(tree: &mut Tree, meta: &Metadata, libs: &[BundledLib]) -> Result<Option<Applied>> {
    if libs.is_empty() {
        return Ok(None);
    }
    let dir = bundle_dir(meta);
    let mtime = crate::tarutil::now();
    let mut added = 0;
    for lib in libs {
        let Some(src) = from_cache(&lib.path) else { continue };
        if !can_bundle(&lib.soname) || lib.soname.contains('/') {
            continue;
        }
        let data = std::fs::read(&src)?;
        tree.add_file_bytes(&format!("{dir}/{}", lib.soname), &data, 0o755, mtime)?;
        added += 1;
        for l in &lib.licenses {
            if let Some(src) = from_cache(l) {
                let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                tree.add_file_bytes(&format!("{dir}/licenses/{}/{name}", safe(&lib.package)), &std::fs::read(&src)?, 0o644, mtime)?;
            }
        }
    }
    if added == 0 {
        return Ok(None);
    }
    let note = format!(
        "Bibliothèques ajoutées par TAMGA parce qu'elles manquent sur la distribution cible.\n\
         Libraries added by TAMGA because the target distribution lacks them.\n\n{}",
        libs.iter().map(|l| format!("{}  ←  {}\n", l.soname, l.source)).collect::<String>()
    );
    tree.add_file_bytes(&format!("{dir}/README.TAMGA"), note.as_bytes(), 0o644, mtime)?;

    // Programmes du paquet : le vrai fichier est renommé « .<nom>.tamga » dans le même
    // dossier ($ORIGIN reste valable), un script prend sa place.
    let programs: Vec<String> = tree.entries.keys().filter(|k| !k.starts_with(&format!("{dir}/"))).filter(|k| is_dynamic_program(tree, k)).cloned().collect();
    let lib_dir_abs = format!("/{dir}");
    for p in &programs {
        let Some(entry) = tree.entries.remove(p) else { continue };
        let dirname = parent(p).unwrap_or("");
        let real = if dirname.is_empty() { format!(".{}.tamga", basename(p)) } else { format!("{dirname}/.{}.tamga", basename(p)) };
        crate::vlog!("conversion", "programme enveloppé : /{p} → lance /{real} avec {lib_dir_abs}");
        let mode = entry.mode;
        tree.entries.insert(real.clone(), entry);
        tree.add_file_bytes(p, wrapper_script(&lib_dir_abs, &format!("/{real}")).as_bytes(), mode & 0o7777 | 0o755, mtime)?;
    }
    Ok(Some(Applied { dir, wrapped: programs.len(), needs_ld_conf: programs.is_empty() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repos::RepoIndex;

    #[test]
    fn candidates_by_name() {
        let text = "Package: libicu70\nVersion: 70.1-2\nFilename: pool/main/i/icu/libicu70_70.1-2_amd64.deb\n\nPackage: libicu-dev\nVersion: 70.1-2\nFilename: pool/x.deb\n\nPackage: libjpeg-turbo8\nVersion: 2.1.2\nFilename: pool/j.deb\n\nPackage: libjpeg8\nVersion: 8c\nFilename: pool/j8.deb\n\nPackage: libssl1.1\nVersion: 1.1.1f\nFilename: pool/s.deb\n";
        let idx: RepoIndex = crate::repos::parse_debian_packages(text, "https://x");
        let c = name_candidates("libicuuc.so.70", &[], &idx, true);
        assert_eq!(c.first().map(|p| p.name.as_str()), Some("libicu70"), "{c:?}");
        let c = name_candidates("libjpeg.so.8", &["libjpeg-turbo8".to_string()], &idx, true);
        assert_eq!(c.first().map(|p| p.name.as_str()), Some("libjpeg8"));
        assert!(c.iter().any(|p| p.name == "libjpeg-turbo8"));
        let c = name_candidates("libssl.so.1.1", &[], &idx, true);
        assert_eq!(c.first().map(|p| p.name.as_str()), Some("libssl1.1"));
        assert!(!can_bundle("libc.so.6"));
        let lib = BundledLib { soname: "libicuuc.so.70".into(), path: String::new(), source: String::new(), package: "libicu70".into(), size: 0, needs: vec![], licenses: vec![] };
        assert!(covers("libicu70", &lib));
        assert!(covers("libicuuc.so.70()(64bit)", &lib));
        assert!(!covers("libicu74", &lib));
        let j = BundledLib { soname: "libjpeg.so.8".into(), package: "libjpeg-turbo".into(), ..lib.clone() };
        assert!(covers("libjpeg.so=8-64", &j));
        assert!(covers("libjpeg8", &j));
        assert!(can_bundle("libicuuc.so.70"));
    }

    #[test]
    fn wrapper() {
        let s = wrapper_script("/usr/lib/tamga/app", "/usr/bin/.app.tamga");
        assert!(s.contains("LD_LIBRARY_PATH=\"/usr/lib/tamga/app${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}\""));
        assert!(s.contains("exec \"/usr/bin/.app.tamga\" \"$@\""));
    }
}
