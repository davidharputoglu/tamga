//! Résolution des dépendances pour la distribution cible.
//! Dependency resolution for the target distribution.
//!
//! Sources d'information, dans l'ordre :
//!  1. les bibliothèques réellement utilisées par les binaires (analyse ELF) ;
//!  2. le système local, quand il appartient à la même famille que la cible
//!     (paquet installé ? disponible dans les dépôts ?) ;
//!  3. les tables de correspondance intégrées ;
//!  4. les règles de nommage Debian ;
//!  5. en ligne : Repology (seul le nom du paquet est envoyé), avec cache local.

use crate::distro::{self, LdCache, SystemInfo};
use crate::elf::{self, ElfInfo};
use crate::mapping;
use crate::model::{Dependency, Family, Format, Metadata, Op};
use crate::version::portable_dep_version;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

/// Statut d'une dépendance une fois résolue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Déjà installée sur cette machine.
    Installed,
    /// Disponible dans les dépôts de cette machine.
    Available,
    /// Traduite, mais non vérifiée (distribution différente de cette machine).
    Estimated,
    /// Introuvable.
    Missing,
    /// Inutile pour la cible (déjà couverte, ou propre à l'ancienne distribution).
    Skipped,
}

/// D'où vient la traduction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Bibliothèque détectée dans les binaires.
    Library,
    /// Dépendance déclarée par le paquet d'origine.
    Declared,
    /// Exigence de version de la glibc.
    Glibc,
}

/// Une ligne du tableau des dépendances.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DepItem {
    /// Texte d'origine (« libgtk-3-0 (>= 3.24) » ou « libgtk-3.so.0 »).
    pub source: String,
    pub origin: Origin,
    /// Dépendance finale pour la cible.
    pub target: Option<Dependency>,
    pub status: Status,
    /// Méthode utilisée : same, soname, local, table, convention, online, file, none.
    pub method: String,
    /// Précision (paquet qui fournit, version de la glibc locale…).
    pub detail: Option<String>,
    /// Incluse dans le paquet final ?
    pub keep: bool,
}

/// Distribution cible choisie par l'utilisateur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetDistro {
    /// Cette machine (vérifications locales possibles).
    ThisSystem,
    Debian,
    Fedora,
    Suse,
    Arch,
    /// Toutes distributions (installateur .sh).
    Any,
}

impl TargetDistro {
    pub fn parse(s: &str) -> Option<TargetDistro> {
        Some(match s.to_ascii_lowercase().as_str() {
            "auto" | "this" | "this_system" | "local" => TargetDistro::ThisSystem,
            "debian" | "ubuntu" | "mint" => TargetDistro::Debian,
            "fedora" | "rhel" => TargetDistro::Fedora,
            "suse" | "opensuse" => TargetDistro::Suse,
            "arch" | "manjaro" => TargetDistro::Arch,
            "any" | "all" | "generic" => TargetDistro::Any,
            _ => return None,
        })
    }

    /// Famille de nommage effective pour un format cible.
    pub fn family(self, format: Format, sys: &SystemInfo) -> Family {
        match self {
            TargetDistro::ThisSystem => match (format, sys.family) {
                (Format::Rpm, Some(Family::Suse)) => Family::Suse,
                (Format::Sh, Some(f)) => f,
                _ => format.family(),
            },
            TargetDistro::Debian => Family::Debian,
            TargetDistro::Fedora => Family::Fedora,
            TargetDistro::Suse => Family::Suse,
            TargetDistro::Arch => Family::Arch,
            TargetDistro::Any => match format {
                Format::Sh => Family::Generic,
                f => f.family(),
            },
        }
    }
}

/// Paramètres de résolution.
pub struct ResolveCtx<'a> {
    pub format: Format,
    pub distro: TargetDistro,
    pub online: bool,
    pub sys: &'a SystemInfo,
}

/// Vrai si l'on peut vérifier sur cette machine (même famille).
fn local_checks(ctx: &ResolveCtx, fam: Family) -> bool {
    ctx.distro == TargetDistro::ThisSystem && ctx.sys.family == Some(fam) && fam.format() == ctx.format
}

fn is_lib_package_name(name: &str, fam: Family) -> bool {
    let n = name.to_ascii_lowercase();
    match fam {
        Family::Debian => n.starts_with("lib") && n.chars().any(|c| c.is_ascii_digit()) && !n.ends_with("-dev") && !n.ends_with("-bin") && !n.ends_with("-tools"),
        Family::Fedora | Family::Suse => n.ends_with("-libs") || (n.starts_with("lib") && !n.ends_with("-devel")),
        Family::Arch => n.ends_with("-libs") || n == "glibc" || n == "gcc-libs",
        Family::Generic => false,
    }
}

/// Garde une contrainte de version portable (≥ uniquement) lors d'une traduction.
fn portable(dep: &Dependency, new_name: &str) -> Dependency {
    match (dep.op, dep.version.as_deref().and_then(portable_dep_version)) {
        (Some(Op::Ge) | Some(Op::Gt), Some(v)) => Dependency::with(new_name, Op::Ge, &v),
        _ => Dependency::simple(new_name),
    }
}

/// Résout toutes les dépendances d'un paquet pour la cible.
pub fn resolve(meta: &Metadata, elf: &ElfInfo, ctx: &ResolveCtx) -> Vec<DepItem> {
    let fam = ctx.distro.family(ctx.format, ctx.sys);
    let local = local_checks(ctx, fam);
    let ld = if local || ctx.format == Format::Sh { LdCache::load() } else { LdCache::default() };
    let mut online = if ctx.online { Some(Repology::new()) } else { None };
    let mut items: Vec<DepItem> = Vec::new();

    // --- 1. Bibliothèques réellement utilisées
    let has_elf = !elf.needed.is_empty();
    for (soname, versions) in &elf.needed {
        let mut item = DepItem {
            source: soname.clone(),
            origin: Origin::Library,
            target: None,
            status: Status::Estimated,
            method: "none".into(),
            detail: None,
            keep: true,
        };
        match ctx.format {
            Format::Rpm => {
                // rpm comprend directement les bibliothèques : c'est la méthode la plus fiable.
                item.target = Some(Dependency::simple(&elf::rpm_soname(soname, None, elf.is64)));
                item.method = "soname".into();
                if local {
                    if ld.find(soname, elf.is64).is_some() {
                        item.status = Status::Installed;
                        item.detail = ld.find(soname, elf.is64).and_then(|p| distro::owner_of(ctx.sys, &p));
                    } else if let Some(p) = distro::available_in_repos(ctx.sys, item.target.as_ref().unwrap().name.as_str()) {
                        item.status = Status::Available;
                        item.detail = Some(p);
                    } else {
                        item.status = Status::Missing;
                    }
                }
                // Versions de symboles : une dépendance par famille (GLIBC_, GLIBCXX_…)
                // Seules les versions de la glibc et de GCC sont standardisées entre distributions.
                let std_vers: std::collections::BTreeSet<String> = versions
                    .iter()
                    .filter(|v| ["GLIBC_", "GLIBCXX_", "CXXABI_", "GCC_"].iter().any(|p| v.starts_with(p)))
                    .cloned()
                    .collect();
                for v in elf::max_symbol_versions(&std_vers) {
                    items.push(DepItem {
                        source: format!("{soname} ({v})"),
                        origin: Origin::Library,
                        target: Some(Dependency::simple(&elf::rpm_soname(soname, Some(&v), elf.is64))),
                        status: item.status,
                        method: "soname".into(),
                        detail: None,
                        keep: true,
                    });
                }
            }
            Format::Sh => {
                // Vérifiée par l'installateur lui-même sur la machine cible.
                item.method = "soname".into();
                item.target = Some(Dependency::simple(soname));
                item.detail = mapping::hint_line(soname);
                if ld.find(soname, elf.is64).is_some() {
                    item.status = Status::Installed;
                }
            }
            Format::Deb | Format::Arch => {
                let mut name: Option<String> = None;
                if local {
                    if let Some(path) = ld.find(soname, elf.is64) {
                        if let Some(owner) = distro::owner_of(ctx.sys, &path) {
                            name = Some(owner.clone());
                            item.status = Status::Installed;
                            item.method = "local".into();
                            item.detail = Some(path);
                        }
                    }
                }
                if name.is_none() {
                    if let Some(n) = mapping::package_for_soname(soname, fam) {
                        name = Some(n.to_string());
                        item.method = "table".into();
                    } else if fam == Family::Debian {
                        name = mapping::debian_convention(soname);
                        item.method = "convention".into();
                    }
                    if let (Some(n), true) = (&name, local) {
                        if distro::is_installed(ctx.sys, n) {
                            item.status = Status::Installed;
                        } else if let Some(p) = distro::available_in_repos(ctx.sys, n) {
                            item.status = Status::Available;
                            if &p != n {
                                item.detail = Some(p.clone());
                                name = Some(p);
                            }
                        } else {
                            item.status = Status::Missing;
                        }
                    }
                }
                match name {
                    Some(n) => item.target = Some(Dependency::simple(&n)),
                    None => {
                        item.status = Status::Missing;
                        item.keep = false;
                    }
                }
            }
        }
        items.push(item);
    }

    // --- 2. glibc minimale (Debian / Arch : contrainte de version sur le paquet glibc)
    if let Some(g) = &elf.glibc {
        if matches!(ctx.format, Format::Deb | Format::Arch) {
            let pkg = if fam == Family::Debian { "libc6" } else { "glibc" };
            let ok = ctx.sys.glibc.as_deref().map(|l| elf::version_cmp(l, g).is_ge());
            let status = match (local, ok) {
                (true, Some(true)) => Status::Installed,
                (true, Some(false)) => Status::Missing,
                _ => Status::Estimated,
            };
            items.push(DepItem {
                source: format!("glibc ≥ {g}"),
                origin: Origin::Glibc,
                target: Some(Dependency::with(pkg, Op::Ge, g)),
                status,
                method: "soname".into(),
                detail: ctx.sys.glibc.clone(),
                keep: true,
            });
        }
    }

    // --- 3. Dépendances déclarées par le paquet d'origine
    let from = meta.dep_family.unwrap_or(Family::Generic);
    let mut seen_targets: BTreeSet<String> = items.iter().filter_map(|i| i.target.as_ref().map(|t| t.name.clone())).collect();
    for dep in &meta.depends {
        let source = dep.display();
        let mut item = DepItem {
            source: source.clone(),
            origin: Origin::Declared,
            target: None,
            status: Status::Estimated,
            method: "none".into(),
            detail: None,
            keep: true,
        };
        let name = dep.name.as_str();

        // Même famille (ex. .sh créé depuis un .deb, reconverti en .deb) : on garde tel quel.
        if from == fam && from != Family::Generic {
            item.target = Some(dep.clone());
            item.method = "same".into();
            if local {
                item.status = if distro::is_installed(ctx.sys, name) {
                    Status::Installed
                } else if distro::available_in_repos(ctx.sys, name).is_some() {
                    Status::Available
                } else {
                    Status::Missing
                };
            }
            items.push(item);
            continue;
        }
        if mapping::is_family_specific(name) || name.starts_with('(') {
            item.status = Status::Skipped;
            item.keep = false;
            item.method = "skip".into();
            items.push(item);
            continue;
        }
        // Bibliothèque rpm « libfoo.so.1()(64bit) » : déjà couverte par l'analyse ELF.
        if name.contains(".so") && (name.contains("()") || name.ends_with(')') || !name.contains(' ')) && name.starts_with("lib") {
            let so = name.split('(').next().unwrap_or(name);
            if elf.needed.contains_key(so) || elf.provided.contains(so) || ctx.format == Format::Sh {
                item.status = Status::Skipped;
                item.keep = false;
                item.method = "soname".into();
                items.push(item);
                continue;
            }
            // Pas de binaire dans le paquet : on traduit la bibliothèque.
            if ctx.format == Format::Rpm {
                item.target = Some(Dependency::simple(name));
                item.method = "soname".into();
            } else if let Some(n) = mapping::package_for_soname(so, fam) {
                item.target = Some(Dependency::simple(n));
                item.method = "table".into();
            } else if fam == Family::Debian {
                item.target = mapping::debian_convention(so).map(|n| Dependency::simple(&n));
                item.method = "convention".into();
            }
        }
        // Commande requise par chemin : « /usr/bin/python3 »
        else if name.starts_with('/') {
            if ctx.format == Format::Rpm {
                item.target = Some(Dependency::simple(name));
                item.method = "file".into();
            } else if let Some(n) = mapping::package_for_file(name, fam) {
                item.target = Some(Dependency::simple(n));
                item.method = "file".into();
            } else {
                item.status = Status::Skipped;
                item.keep = false;
                item.method = "skip".into();
                items.push(item);
                continue;
            }
        }
        // Paquet de bibliothèque déjà couvert par l'analyse des binaires
        else if has_elf
            && mapping::sonames_for_package(name, from).iter().any(|so| elf.needed.contains_key(*so) || elf.provided.contains(*so))
        {
            item.status = Status::Skipped;
            item.keep = false;
            item.method = "soname".into();
            items.push(item);
            continue;
        } else if has_elf
            && is_lib_package_name(name, from)
            && (ctx.format == Format::Rpm || mapping::translate(name, from, fam).map(|t| seen_targets.contains(t)).unwrap_or(true))
        {
            item.status = Status::Skipped;
            item.keep = false;
            item.method = "soname".into();
            items.push(item);
            continue;
        } else if let Some(t) = mapping::translate(name, from, fam) {
            item.target = Some(portable(dep, t));
            item.method = "table".into();
        } else if let Some(t) = dep.alternatives.iter().find_map(|a| mapping::translate(&a.name, from, fam)) {
            item.target = Some(Dependency::simple(t));
            item.method = "table".into();
        } else if ctx.format == Format::Sh {
            // L'installateur .sh ne gère que les bibliothèques ; on garde l'information.
            item.status = Status::Skipped;
            item.keep = false;
            item.method = "skip".into();
            items.push(item);
            continue;
        } else if local && (distro::is_installed(ctx.sys, name) || distro::available_in_repos(ctx.sys, name).is_some()) {
            // Même nom sur cette machine
            item.target = Some(portable(dep, name));
            item.method = "local".into();
        } else if let Some(rep) = online.as_mut() {
            match rep.translate(name, from, fam, ctx.distro, ctx.sys) {
                Some(t) => {
                    item.target = Some(portable(dep, &t));
                    item.method = "online".into();
                }
                None => {
                    item.method = "online".into();
                }
            }
        }

        match &item.target {
            None => {
                item.status = Status::Missing;
                item.keep = false;
            }
            Some(t) => {
                if seen_targets.contains(&t.name) && t.version.is_none() {
                    item.status = Status::Skipped;
                    item.keep = false;
                    item.detail = Some(t.name.clone());
                    items.push(item);
                    continue;
                }
                seen_targets.insert(t.name.clone());
                if local {
                    item.status = if distro::is_installed(ctx.sys, &t.name) {
                        Status::Installed
                    } else if let Some(p) = distro::available_in_repos(ctx.sys, &t.name) {
                        item.detail = Some(p);
                        Status::Available
                    } else {
                        Status::Missing
                    };
                }
            }
        }
        items.push(item);
    }
    if let Some(r) = online {
        r.save();
    }
    // Un paquet ne dépend pas de lui-même (ex. « curl » dont libcurl est fourni par « curl » sur Arch).
    let own: Vec<String> = vec![
        meta.name.clone(),
        crate::version::deb_name(&meta.name),
        crate::version::rpm_name(&meta.name),
        crate::version::pacman_name(&meta.name),
    ];
    for it in items.iter_mut() {
        if it.target.as_ref().map(|t| own.contains(&t.name)).unwrap_or(false) {
            it.status = Status::Skipped;
            it.keep = false;
            it.method = "self".into();
        }
    }
    items
}

/// Dépendances finales (gardées) d'une liste résolue, sans doublons.
pub fn final_deps(items: &[DepItem]) -> Vec<Dependency> {
    dedupe(items.iter().filter(|i| i.keep).filter_map(|i| i.target.clone()))
}

/// Fusionne les doublons : un seul exemplaire par nom, avec la contrainte « ≥ » la plus forte.
pub fn dedupe(list: impl IntoIterator<Item = Dependency>) -> Vec<Dependency> {
    let mut out: Vec<Dependency> = Vec::new();
    for t in list {
        if let Some(existing) = out.iter_mut().find(|d| d.name == t.name) {
            if existing.version.is_none() && t.version.is_some() {
                *existing = t;
            } else if let (Some(a), Some(b)) = (&existing.version, &t.version) {
                if elf::version_cmp(b, a).is_gt() {
                    *existing = t;
                }
            }
            continue;
        }
        out.push(t);
    }
    out
}

/// Vrai si au moins une dépendance gardée est introuvable.
pub fn has_missing(items: &[DepItem]) -> bool {
    items.iter().any(|i| i.status == Status::Missing && (i.keep || i.target.is_none()))
}

// ---------------------------------------------------------------------------
// Repology (en ligne) + cache local
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default)]
struct CacheFile {
    entries: HashMap<String, (Option<String>, u64)>,
}

pub struct Repology {
    cache: CacheFile,
    path: Option<PathBuf>,
    dirty: bool,
    last_request: Option<std::time::Instant>,
    failures: u32,
}

#[derive(Deserialize)]
struct RepPkg {
    repo: String,
    #[serde(default)]
    binname: Option<String>,
    #[serde(default)]
    binnames: Option<Vec<String>>,
}

const CACHE_TTL: u64 = 30 * 24 * 3600;

pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("tamga"))
}

/// Supprime le cache local des recherches.
pub fn clear_cache() -> std::io::Result<()> {
    if let Some(d) = cache_dir() {
        let f = d.join("repology.json");
        if f.exists() {
            std::fs::remove_file(f)?;
        }
    }
    Ok(())
}

fn repo_for(f: Family, distro: TargetDistro, sys: &SystemInfo) -> &'static str {
    // Pour « cette machine », on choisit le dépôt Repology le plus proche.
    let _ = (distro, sys);
    match f {
        Family::Debian => "debian_unstable",
        Family::Fedora => "fedora_rawhide",
        Family::Suse => "opensuse_tumbleweed",
        Family::Arch => "arch",
        Family::Generic => "",
    }
}

impl Repology {
    pub fn new() -> Repology {
        let path = cache_dir().map(|d| d.join("repology.json"));
        let cache = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Repology { cache, path, dirty: false, last_request: None, failures: 0 }
    }

    pub fn save(&self) {
        if !self.dirty {
            return;
        }
        if let Some(p) = &self.path {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Ok(b) = serde_json::to_vec(&self.cache) {
                let _ = std::fs::write(p, b);
            }
        }
    }

    fn fetch(&mut self, from_repo: &str, name: &str) -> Option<Vec<RepPkg>> {
        if self.failures >= 2 {
            return None; // hors ligne : on n'insiste pas
        }
        // Repology demande au plus une requête par seconde.
        if let Some(t) = self.last_request {
            let el = t.elapsed();
            if el < std::time::Duration::from_millis(1100) {
                std::thread::sleep(std::time::Duration::from_millis(1100) - el);
            }
        }
        self.last_request = Some(std::time::Instant::now());
        let url = format!(
            "https://repology.org/tools/project-by?repo={}&name_type=binname&target_page=api_v1_project&name={}",
            from_repo,
            urlencode(name)
        );
        let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(12)).redirects(5).build();
        match agent.get(&url).set("User-Agent", &format!("TAMGA/{} (Linux package converter)", crate::TAMGA_VERSION)).call() {
            Ok(resp) => {
                self.failures = 0;
                resp.into_json::<Vec<RepPkg>>().ok()
            }
            Err(ureq::Error::Status(404, _)) => Some(vec![]),
            Err(_) => {
                self.failures += 1;
                None
            }
        }
    }

    /// Traduit un nom de paquet d'une famille vers une autre via Repology.
    pub fn translate(&mut self, name: &str, from: Family, to: Family, distro: TargetDistro, sys: &SystemInfo) -> Option<String> {
        let from_repo = repo_for(from, distro, sys);
        let to_repo = repo_for(to, distro, sys);
        if from_repo.is_empty() || to_repo.is_empty() {
            return None;
        }
        let key = format!("{from_repo}|{name}|{to_repo}");
        let now = crate::tarutil::now();
        if let Some((v, t)) = self.cache.entries.get(&key) {
            if now.saturating_sub(*t) < CACHE_TTL {
                return v.clone();
            }
        }
        let pkgs = self.fetch(from_repo, name)?;
        let mut cands: Vec<String> = pkgs
            .iter()
            .filter(|p| p.repo == to_repo)
            .flat_map(|p| {
                let mut v = p.binnames.clone().unwrap_or_default();
                if let Some(b) = &p.binname {
                    v.push(b.clone());
                }
                v
            })
            .filter(|n| !is_secondary_package(n))
            .collect();
        cands.sort();
        cands.dedup();
        let best = choose_candidate(name, &cands);
        self.cache.entries.insert(key, (best.clone(), now));
        self.dirty = true;
        best
    }
}

impl Default for Repology {
    fn default() -> Self {
        Self::new()
    }
}

fn is_secondary_package(n: &str) -> bool {
    [
        "-devel", "-dev", "-doc", "-docs", "-debuginfo", "-debugsource", "-dbg", "-static", "-tests", "-test", "-examples",
        "-lang", "-man", "-src",
    ]
    .iter()
    .any(|s| n.ends_with(s))
}

/// Choisit le meilleur candidat : nom identique, sinon même préfixe, sinon le plus court.
pub fn choose_candidate(source: &str, cands: &[String]) -> Option<String> {
    if cands.is_empty() {
        return None;
    }
    if let Some(c) = cands.iter().find(|c| c.as_str() == source) {
        return Some(c.clone());
    }
    let prefix = source.split('-').next().unwrap_or(source);
    let mut same: Vec<&String> = cands.iter().filter(|c| c.starts_with(prefix)).collect();
    if same.is_empty() {
        same = cands.iter().collect();
    }
    same.sort_by_key(|c| (c.len(), c.to_string()));
    same.first().map(|c| c.to_string())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Dependency;
    use std::collections::BTreeMap;

    fn sys_none() -> SystemInfo {
        SystemInfo { family: None, ..Default::default() }
    }

    #[test]
    fn deb_to_rpm_with_elf() {
        let meta = Metadata {
            depends: vec![
                Dependency::with("libc6", Op::Ge, "2.34"),
                Dependency::simple("libgtk-3-0"),
                Dependency::simple("xdg-utils"),
                Dependency::simple("debconf"),
                Dependency::simple("python3-gi"),
            ],
            dep_family: Some(Family::Debian),
            ..Default::default()
        };
        let mut needed = BTreeMap::new();
        needed.insert("libgtk-3.so.0".to_string(), BTreeSet::new());
        needed.insert("libc.so.6".to_string(), ["GLIBC_2.34".to_string()].into_iter().collect());
        let elf = ElfInfo { elf_files: 1, is64: true, needed, glibc: Some("2.34".into()), ..Default::default() };
        let sys = sys_none();
        let ctx = ResolveCtx { format: Format::Rpm, distro: TargetDistro::Fedora, online: false, sys: &sys };
        let items = resolve(&meta, &elf, &ctx);
        let fin: Vec<String> = final_deps(&items).iter().map(|d| d.display_std()).collect();
        assert!(fin.contains(&"libgtk-3.so.0()(64bit)".to_string()), "{fin:?}");
        assert!(fin.contains(&"libc.so.6(GLIBC_2.34)(64bit)".to_string()), "{fin:?}");
        assert!(fin.contains(&"xdg-utils".to_string()), "{fin:?}");
        assert!(fin.contains(&"python3-gobject".to_string()), "{fin:?}");
        assert!(!fin.iter().any(|d| d.contains("libgtk-3-0") || d.contains("debconf") || d.contains("libc6")), "{fin:?}");
    }

    #[test]
    fn rpm_to_deb_and_arch() {
        let meta = Metadata {
            depends: vec![
                Dependency::simple("libgtk-3.so.0()(64bit)"),
                Dependency::simple("/bin/sh"),
                Dependency::simple("/usr/bin/python3"),
                Dependency::simple("rpmlib(CompressedFileNames)"),
                Dependency::simple("hicolor-icon-theme"),
            ],
            dep_family: Some(Family::Fedora),
            ..Default::default()
        };
        let mut needed = BTreeMap::new();
        needed.insert("libgtk-3.so.0".to_string(), BTreeSet::new());
        needed.insert("libfoo.so.3".to_string(), BTreeSet::new());
        let elf = ElfInfo { elf_files: 1, is64: true, needed, glibc: Some("2.17".into()), ..Default::default() };
        let sys = sys_none();
        for (fmt, distro, expect) in [
            (Format::Deb, TargetDistro::Debian, vec!["libgtk-3-0", "libfoo3", "python3", "hicolor-icon-theme", "libc6 >= 2.17"]),
            (Format::Arch, TargetDistro::Arch, vec!["gtk3", "python", "hicolor-icon-theme", "glibc >= 2.17"]),
        ] {
            let ctx = ResolveCtx { format: fmt, distro, online: false, sys: &sys };
            let items = resolve(&meta, &elf, &ctx);
            let fin: Vec<String> = final_deps(&items).iter().map(|d| d.display_std()).collect();
            for e in expect {
                assert!(fin.contains(&e.to_string()), "{fmt:?}: {e} absent de {fin:?}");
            }
            assert!(!fin.iter().any(|d| d.contains("rpmlib") || d.contains("/bin/sh")), "{fin:?}");
        }
    }

    #[test]
    fn candidates() {
        let c = vec!["python3-gobject".to_string(), "python3-gobject-base".to_string()];
        assert_eq!(choose_candidate("python3-gi", &c).as_deref(), Some("python3-gobject"));
        assert_eq!(choose_candidate("x", &[]), None);
        assert!(is_secondary_package("gtk3-devel"));
    }
}
