//! Dépôts officiels des distributions : listes de paquets et téléchargements.
//! Official distribution repositories: package indexes and downloads.
//!
//! Sert à retrouver une bibliothèque qui manque sur la distribution cible, dans les
//! dépôts de la distribution d'origine du paquet :
//!  - Debian / Ubuntu : fichiers « Packages » (dists/<version>/<section>/binary-<arch>/) ;
//!  - Fedora, AlmaLinux, EPEL, openSUSE : métadonnées « rpm-md » (repodata/primary.xml) ;
//!  - Arch Linux : bases « <dépôt>.db ».
//! Uniquement en HTTPS, depuis les serveurs officiels ; chaque paquet téléchargé est
//! comparé à l'empreinte SHA-256 publiée par le dépôt.

use crate::compress;
use crate::elf::version_cmp;
use crate::model::Family;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Durée de vie des listes de paquets en cache.
const INDEX_TTL: u64 = 3 * 24 * 3600;
/// Taille maximale d'un téléchargement.
const MAX_DOWNLOAD: u64 = 600 * 1024 * 1024;

/// Un paquet d'un dépôt.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepoPackage {
    pub name: String,
    pub version: String,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size: u64,
}

/// Type de dépôt.
#[derive(Clone, Debug)]
pub enum RepoKind {
    /// Dépôt Debian / Ubuntu : base, version (« noble-updates »), section (« universe »).
    Deb { suite: String, component: String, arch: String },
    /// Dépôt rpm-md (dossier qui contient repodata/).
    RpmMd { arches: Vec<String> },
    /// Dépôt Arch : nom du dépôt (« extra »).
    Pacman { repo: String },
}

/// Un dépôt à interroger. `bases` : adresses à essayer dans l'ordre (miroir principal, archives).
#[derive(Clone, Debug)]
pub struct Repo {
    pub label: String,
    pub bases: Vec<String>,
    pub kind: RepoKind,
    /// Version de la glibc de cette version de la distribution (pour la compatibilité).
    pub glibc: Option<String>,
}

/// Liste des paquets d'un dépôt.
#[derive(Default, Serialize, Deserialize)]
pub struct RepoIndex {
    pub packages: Vec<RepoPackage>,
    /// nom de paquet (ou nom fourni) → paquet
    pub by_name: HashMap<String, usize>,
    /// bibliothèque fournie (« libfoo.so.1 », « libfoo.so.1|32 » en 32 bits) → paquets
    pub by_soname: HashMap<String, Vec<usize>>,
}

impl RepoIndex {
    fn push(&mut self, p: RepoPackage, aliases: &[String], sonames: &[String]) {
        let i = self.packages.len();
        self.by_name.insert(p.name.clone(), i);
        for a in aliases {
            self.by_name.entry(a.clone()).or_insert(i);
        }
        for s in sonames {
            self.by_soname.entry(s.clone()).or_default().push(i);
        }
        self.packages.push(p);
    }

    pub fn get(&self, name: &str) -> Option<&RepoPackage> {
        self.by_name.get(name).map(|i| &self.packages[*i])
    }

    pub fn providers(&self, soname: &str, is64: bool) -> Vec<&RepoPackage> {
        let key = if is64 { soname.to_string() } else { format!("{soname}|32") };
        let mut v: Vec<&RepoPackage> = self.by_soname.get(&key).map(|l| l.iter().map(|i| &self.packages[*i]).collect()).unwrap_or_default();
        if v.is_empty() && !is64 {
            // dépôts 32 bits (i386, armhf) : pas de suffixe
            v = self.by_soname.get(soname).map(|l| l.iter().map(|i| &self.packages[*i]).collect()).unwrap_or_default();
        }
        v
    }

    /// Paquets dont le nom commence par `prefix` (recherche de secours par le nom).
    pub fn names_starting_with(&self, prefix: &str) -> Vec<&RepoPackage> {
        let mut v: Vec<&RepoPackage> = self.packages.iter().filter(|p| p.name.starts_with(prefix)).collect();
        v.sort_by_key(|p| p.name.len());
        v
    }
}

// ---------------------------------------------------------------------------
// Analyse des listes de paquets
// ---------------------------------------------------------------------------

/// Fichier « Packages » de Debian / Ubuntu.
pub fn parse_debian_packages(text: &str, base: &str) -> RepoIndex {
    let mut idx = RepoIndex::default();
    for stanza in text.split("\n\n") {
        let mut f: HashMap<&str, String> = HashMap::new();
        let mut last: Option<&str> = None;
        for line in stanza.lines() {
            if line.starts_with(' ') || line.starts_with('\t') {
                if let Some(k) = last {
                    if let Some(v) = f.get_mut(k) {
                        v.push(' ');
                        v.push_str(line.trim());
                    }
                }
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim();
                f.insert(k, v.trim().to_string());
                last = Some(match k {
                    "Provides" => "Provides",
                    "Depends" => "Depends",
                    _ => "",
                });
            }
        }
        let (Some(name), Some(file)) = (f.get("Package"), f.get("Filename")) else { continue };
        let aliases: Vec<String> = f
            .get("Provides")
            .map(|p| p.split(',').filter_map(|x| x.split_whitespace().next()).map(String::from).collect())
            .unwrap_or_default();
        idx.push(
            RepoPackage {
                name: name.clone(),
                version: f.get("Version").cloned().unwrap_or_default(),
                url: format!("{}/{}", base.trim_end_matches('/'), file),
                sha256: f.get("SHA256").cloned(),
                size: f.get("Size").and_then(|s| s.parse().ok()).unwrap_or(0),
            },
            &aliases,
            &[],
        );
    }
    idx
}

/// Nom de bibliothèque d'une capacité rpm : « libfoo.so.1()(64bit) » → (« libfoo.so.1 », 64 bits).
fn rpm_soname_provide(p: &str) -> Option<(String, bool)> {
    if !p.starts_with("lib") && !p.starts_with("ld-") {
        return None;
    }
    if let Some(so) = p.strip_suffix("()(64bit)") {
        return so.contains(".so").then(|| (so.to_string(), true));
    }
    if let Some(so) = p.strip_suffix("()") {
        return so.contains(".so").then(|| (so.to_string(), false));
    }
    if !p.contains('(') && p.contains(".so.") {
        return Some((p.to_string(), false));
    }
    None
}

/// Métadonnées rpm-md « primary.xml » (lecture en continu : le fichier est gros).
pub fn parse_rpm_primary<R: BufRead>(r: R, base: &str, arches: &[String]) -> Result<RepoIndex> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(r);
    let mut buf = Vec::new();
    let mut idx = RepoIndex::default();
    #[derive(Default)]
    struct Cur {
        name: String,
        arch: String,
        version: String,
        href: String,
        sha: Option<String>,
        size: u64,
        sonames: Vec<String>,
    }
    let mut cur: Option<Cur> = None;
    let mut field: Option<&'static str> = None;
    let mut in_provides = false;
    let mut sha_is256 = false;
    loop {
        let ev = reader.read_event_into(&mut buf).context("lecture de primary.xml")?;
        match ev {
            Event::Eof => break,
            Event::Start(ref e) | Event::Empty(ref e) => {
                let is_empty = matches!(ev, Event::Empty(_));
                let name = e.name();
                let local = name.as_ref();
                let attr = |k: &[u8]| -> Option<String> {
                    e.attributes().flatten().find(|a| a.key.as_ref() == k).and_then(|a| a.unescape_value().ok().map(|v| v.to_string()))
                };
                match local {
                    b"package" => cur = Some(Cur::default()),
                    b"name" if cur.is_some() && !in_provides => field = Some("name"),
                    b"arch" if cur.is_some() => field = Some("arch"),
                    b"version" => {
                        if let Some(c) = cur.as_mut() {
                            let ep = attr(b"epoch").filter(|e| e != "0").map(|e| format!("{e}:")).unwrap_or_default();
                            c.version = format!("{ep}{}-{}", attr(b"ver").unwrap_or_default(), attr(b"rel").unwrap_or_default());
                        }
                    }
                    b"checksum" if cur.is_some() => {
                        sha_is256 = attr(b"type").as_deref() == Some("sha256");
                        field = Some("checksum");
                    }
                    b"size" => {
                        if let Some(c) = cur.as_mut() {
                            c.size = attr(b"package").and_then(|s| s.parse().ok()).unwrap_or(0);
                        }
                    }
                    b"location" => {
                        if let Some(c) = cur.as_mut() {
                            c.href = attr(b"href").unwrap_or_default();
                        }
                    }
                    b"rpm:provides" if !is_empty => in_provides = true,
                    b"rpm:entry" if in_provides => {
                        if let (Some(c), Some(n)) = (cur.as_mut(), attr(b"name")) {
                            if let Some((so, is64)) = rpm_soname_provide(&n) {
                                c.sonames.push(if is64 { so } else { format!("{so}|32") });
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(t) => {
                if let (Some(f), Some(c)) = (field, cur.as_mut()) {
                    let v = t.unescape().map(|s| s.trim().to_string()).unwrap_or_default();
                    match f {
                        "name" => c.name = v,
                        "arch" => c.arch = v,
                        "checksum" if sha_is256 => c.sha = Some(v),
                        _ => {}
                    }
                }
            }
            Event::End(ref e) => {
                field = None;
                match e.name().as_ref() {
                    b"rpm:provides" => in_provides = false,
                    b"package" => {
                        if let Some(c) = cur.take() {
                            let arch_ok = arches.iter().any(|a| *a == c.arch) || c.arch == "noarch";
                            if arch_ok && !c.sonames.is_empty() && !c.href.is_empty() {
                                let mut sonames = c.sonames.clone();
                                sonames.dedup();
                                idx.push(
                                    RepoPackage {
                                        name: c.name,
                                        version: c.version,
                                        url: format!("{}/{}", base.trim_end_matches('/'), c.href),
                                        sha256: c.sha,
                                        size: c.size,
                                    },
                                    &[],
                                    &sonames,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(idx)
}

/// Emplacement de primary.xml dans repomd.xml : (href, sha256).
pub fn parse_repomd(xml: &str) -> Option<(String, Option<String>)> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut in_primary = false;
    let mut in_checksum = false;
    let mut href = None;
    let mut sha = None;
    loop {
        match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let attr = |k: &[u8]| e.attributes().flatten().find(|a| a.key.as_ref() == k).and_then(|a| a.unescape_value().ok().map(|v| v.to_string()));
                match e.name().as_ref() {
                    b"data" => in_primary = attr(b"type").as_deref() == Some("primary"),
                    b"location" if in_primary => href = attr(b"href"),
                    b"checksum" if in_primary => in_checksum = attr(b"type").as_deref() == Some("sha256"),
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if in_checksum => {
                sha = t.unescape().ok().map(|s| s.trim().to_string());
                in_checksum = false;
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"data" => {
                if in_primary && href.is_some() {
                    return href.map(|h| (h, sha));
                }
                in_primary = false;
            }
            _ => {}
        }
    }
    href.map(|h| (h, sha))
}

/// Capacité de bibliothèque Arch : « libfoo.so=1-64 » → « libfoo.so.1 » (64 bits).
fn pacman_soname_provide(p: &str) -> Option<String> {
    let (name, ver) = p.split_once('=')?;
    if !name.ends_with(".so") {
        return None;
    }
    let (v, bits) = ver.rsplit_once('-')?;
    let so = format!("{name}.{v}");
    Some(if bits == "64" { so } else { format!("{so}|32") })
}

/// Base de paquets Arch (archive tar de dossiers contenant un fichier « desc »).
pub fn parse_pacman_db<R: Read>(r: R, base: &str) -> Result<RepoIndex> {
    let mut idx = RepoIndex::default();
    let dec = compress::auto_decoder(r)?;
    let mut ar = tar::Archive::new(dec);
    for e in ar.entries()? {
        let mut e = e?;
        let path = e.path()?.to_string_lossy().to_string();
        if !path.ends_with("/desc") {
            continue;
        }
        let mut s = String::new();
        e.read_to_string(&mut s)?;
        let mut sections: HashMap<String, Vec<String>> = HashMap::new();
        let mut key = String::new();
        for line in s.lines() {
            if line.starts_with('%') && line.ends_with('%') {
                key = line.trim_matches('%').to_string();
            } else if !line.is_empty() {
                sections.entry(key.clone()).or_default().push(line.to_string());
            }
        }
        let one = |k: &str| sections.get(k).and_then(|v| v.first()).cloned();
        let (Some(name), Some(file)) = (one("NAME"), one("FILENAME")) else { continue };
        let provides = sections.get("PROVIDES").cloned().unwrap_or_default();
        let sonames: Vec<String> = provides.iter().filter_map(|p| pacman_soname_provide(p)).collect();
        let aliases: Vec<String> = provides.iter().map(|p| p.split(['=', '<', '>']).next().unwrap_or(p).to_string()).collect();
        idx.push(
            RepoPackage {
                name,
                version: one("VERSION").unwrap_or_default(),
                url: format!("{}/{}", base.trim_end_matches('/'), file),
                sha256: one("SHA256SUM"),
                size: one("CSIZE").and_then(|s| s.parse().ok()).unwrap_or(0),
            },
            &aliases,
            &sonames,
        );
    }
    Ok(idx)
}

// ---------------------------------------------------------------------------
// Dépôts à interroger selon la distribution d'origine
// ---------------------------------------------------------------------------

fn deb_arch(machine: &str) -> Option<&'static str> {
    Some(match machine {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "i686" => "i386",
        "armv7" => "armhf",
        "riscv64" => "riscv64",
        "ppc64le" => "ppc64el",
        "s390x" => "s390x",
        _ => return None,
    })
}

/// Ordre de recherche : d'abord la version dont la glibc correspond à celle demandée par
/// le paquet (sa version de construction probable), puis les plus récentes, puis les plus anciennes.
fn order_by_glibc<T: Clone>(list: &[(T, &str)], pkg_glibc: Option<&str>) -> Vec<T> {
    let mut sorted: Vec<(T, &str)> = list.to_vec();
    sorted.sort_by(|a, b| version_cmp(a.1, b.1));
    let start = pkg_glibc.and_then(|g| sorted.iter().position(|(_, v)| version_cmp(v, g).is_ge())).unwrap_or(sorted.len().saturating_sub(1));
    let mut out = Vec::new();
    for item in sorted.iter().skip(start) {
        out.push(item.0.clone());
    }
    for item in sorted.iter().take(start).rev() {
        out.push(item.0.clone());
    }
    out
}

fn ubuntu_repos(machine: &str, pkg_glibc: Option<&str>) -> Vec<Repo> {
    let Some(arch) = deb_arch(machine) else { return vec![] };
    let base = if matches!(arch, "amd64" | "i386") { "https://archive.ubuntu.com/ubuntu" } else { "https://ports.ubuntu.com/ubuntu-ports" };
    let releases: [((&str, &str), &str); 5] = [
        (("resolute", "26.04"), "2.42"),
        (("noble", "24.04"), "2.39"),
        (("jammy", "22.04"), "2.35"),
        (("focal", "20.04"), "2.31"),
        (("bionic", "18.04"), "2.27"),
    ];
    let mut out = Vec::new();
    for (code, ver) in order_by_glibc(&releases.map(|(r, g)| (r, g)), pkg_glibc) {
        let glibc = releases.iter().find(|(r, _)| r.0 == code).map(|(_, g)| g.to_string());
        for suite in [code.to_string(), format!("{code}-updates")] {
            for comp in ["main", "universe"] {
                out.push(Repo {
                    label: format!("Ubuntu {ver} ({suite}/{comp})"),
                    bases: vec![base.to_string(), "https://old-releases.ubuntu.com/ubuntu".to_string()],
                    kind: RepoKind::Deb { suite: suite.clone(), component: comp.into(), arch: arch.into() },
                    glibc: glibc.clone(),
                });
            }
        }
    }
    out
}

fn debian_repos(machine: &str, pkg_glibc: Option<&str>) -> Vec<Repo> {
    let Some(arch) = deb_arch(machine) else { return vec![] };
    let releases: [((&str, &str), &str); 4] =
        [(("forky", "14"), "2.42"), (("trixie", "13"), "2.41"), (("bookworm", "12"), "2.36"), (("bullseye", "11"), "2.31")];
    let mut out = Vec::new();
    for (code, ver) in order_by_glibc(&releases.map(|(r, g)| (r, g)), pkg_glibc) {
        let glibc = releases.iter().find(|(r, _)| r.0 == code).map(|(_, g)| g.to_string());
        out.push(Repo {
            label: format!("Debian {ver} ({code})"),
            bases: vec!["https://deb.debian.org/debian".into(), "https://archive.debian.org/debian".into()],
            kind: RepoKind::Deb { suite: code.into(), component: "main".into(), arch: arch.into() },
            glibc,
        });
    }
    out
}

fn rpm_arches(machine: &str) -> Option<Vec<String>> {
    match machine {
        "x86_64" => Some(vec!["x86_64".into()]),
        "aarch64" | "ppc64le" | "s390x" => Some(vec![machine.into()]),
        _ => None,
    }
}

fn fedora_repos(machine: &str, pkg_glibc: Option<&str>, release_hint: Option<u32>) -> Vec<Repo> {
    let Some(arches) = rpm_arches(machine) else { return vec![] };
    let releases: [(u32, &str); 7] = [(45, "2.44"), (44, "2.43"), (43, "2.42"), (42, "2.41"), (41, "2.40"), (40, "2.39"), (39, "2.38")];
    let mut order = order_by_glibc(&releases.map(|(n, g)| (n, g)), pkg_glibc);
    if let Some(h) = release_hint {
        order.retain(|n| *n != h);
        order.insert(0, h);
    }
    let mut out = Vec::new();
    for n in order {
        let glibc = releases.iter().find(|(r, _)| *r == n).map(|(_, g)| g.to_string());
        let a = &arches[0];
        out.push(Repo {
            label: format!("Fedora {n}"),
            bases: vec![
                format!("https://dl.fedoraproject.org/pub/fedora/linux/releases/{n}/Everything/{a}/os/"),
                format!("https://archives.fedoraproject.org/pub/archive/fedora/linux/releases/{n}/Everything/{a}/os/"),
            ],
            kind: RepoKind::RpmMd { arches: arches.clone() },
            glibc: glibc.clone(),
        });
        out.push(Repo {
            label: format!("Fedora {n} (updates)"),
            bases: vec![
                format!("https://dl.fedoraproject.org/pub/fedora/linux/updates/{n}/Everything/{a}/"),
                format!("https://archives.fedoraproject.org/pub/archive/fedora/linux/updates/{n}/Everything/{a}/"),
            ],
            kind: RepoKind::RpmMd { arches: arches.clone() },
            glibc,
        });
    }
    out
}

fn el_repos(machine: &str, pkg_glibc: Option<&str>, release_hint: Option<u32>) -> Vec<Repo> {
    let Some(arches) = rpm_arches(machine) else { return vec![] };
    let releases: [(u32, &str); 3] = [(10, "2.39"), (9, "2.34"), (8, "2.28")];
    let mut order = order_by_glibc(&releases.map(|(n, g)| (n, g)), pkg_glibc);
    if let Some(h) = release_hint {
        order.retain(|n| *n != h);
        order.insert(0, h);
    }
    let a = &arches[0];
    let mut out = Vec::new();
    for n in order {
        let glibc = releases.iter().find(|(r, _)| *r == n).map(|(_, g)| g.to_string());
        for part in ["BaseOS", "AppStream", "CRB"] {
            let crb = if n == 8 && part == "CRB" { "PowerTools" } else { part };
            out.push(Repo {
                label: format!("AlmaLinux {n} ({crb})"),
                bases: vec![
                    format!("https://repo.almalinux.org/almalinux/{n}/{crb}/{a}/os/"),
                    format!("https://vault.almalinux.org/{n}/{crb}/{a}/os/"),
                ],
                kind: RepoKind::RpmMd { arches: arches.clone() },
                glibc: glibc.clone(),
            });
        }
        out.push(Repo {
            label: format!("EPEL {n}"),
            bases: vec![
                format!("https://dl.fedoraproject.org/pub/epel/{n}/Everything/{a}/"),
                format!("https://archives.fedoraproject.org/pub/archive/epel/{n}/Everything/{a}/"),
            ],
            kind: RepoKind::RpmMd { arches: arches.clone() },
            glibc,
        });
    }
    out
}

fn opensuse_repos(machine: &str, pkg_glibc: Option<&str>) -> Vec<Repo> {
    let Some(arches) = rpm_arches(machine) else { return vec![] };
    let releases: [(&str, &str); 3] = [("tumbleweed", "2.42"), ("16.0", "2.40"), ("15.6", "2.38")];
    let mut out = Vec::new();
    for r in order_by_glibc(&releases, pkg_glibc) {
        let glibc = releases.iter().find(|(n, _)| *n == r).map(|(_, g)| g.to_string());
        let (label, base) = if r == "tumbleweed" {
            ("openSUSE Tumbleweed".to_string(), "https://download.opensuse.org/tumbleweed/repo/oss/".to_string())
        } else {
            (format!("openSUSE Leap {r}"), format!("https://download.opensuse.org/distribution/leap/{r}/repo/oss/"))
        };
        out.push(Repo { label, bases: vec![base], kind: RepoKind::RpmMd { arches: arches.clone() }, glibc });
    }
    out
}

fn arch_repos(machine: &str) -> Vec<Repo> {
    if machine != "x86_64" {
        return vec![];
    }
    ["core", "extra"]
        .iter()
        .map(|r| Repo {
            label: format!("Arch Linux ({r})"),
            bases: vec![
                format!("https://geo.mirror.pkgbuild.com/{r}/os/x86_64"),
                format!("https://mirrors.kernel.org/archlinux/{r}/os/x86_64"),
            ],
            kind: RepoKind::Pacman { repo: r.to_string() },
            glibc: None,
        })
        .collect()
}

/// Dépôts de la distribution cible (quand ce n'est pas cette machine), pour vérifier
/// qu'une bibliothèque y existe : les versions les plus récentes de la famille.
pub fn target_repos(family: Family, machine: &str) -> Vec<Repo> {
    match family {
        Family::Fedora => fedora_repos(machine, None, None).into_iter().take(4).collect(),
        Family::Suse => opensuse_repos(machine, None).into_iter().take(2).collect(),
        Family::Debian => {
            let d = debian_repos(machine, Some("2.41")).into_iter().take(1);
            let u = ubuntu_repos(machine, Some("2.39")).into_iter().filter(|r| !r.label.contains("-updates")).take(2);
            d.chain(u).collect()
        }
        _ => vec![],
    }
}

/// Indices de version dans le numéro de version d'un paquet rpm : « 1.2-3.fc41 » → Fedora 41.
fn rpm_release_hint(release: &str) -> (Option<u32>, Option<u32>, bool) {
    let r = release.to_ascii_lowercase();
    let num_after = |tag: &str| -> Option<u32> {
        let i = r.find(tag)? + tag.len();
        let digits: String = r[i..].chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    };
    let fc = num_after(".fc").or_else(|| if r.starts_with("fc") { num_after("fc") } else { None });
    let el = num_after(".el").or_else(|| num_after("el"));
    let suse = r.contains("lp15") || r.contains("lp16") || r.contains("suse") || r.contains(".sle");
    (fc, el.filter(|n| (7..=12).contains(n)), suse)
}

/// Dépôts à interroger, dans l'ordre, pour un paquet de cette famille.
pub fn repos_for(family: Family, version: &str, release: &str, maintainer: &str, machine: &str, pkg_glibc: Option<&str>) -> Vec<Repo> {
    match family {
        Family::Debian => {
            let ubuntu_first = version.contains("ubuntu") || maintainer.to_ascii_lowercase().contains("ubuntu") || release.contains("ubuntu");
            let (u, d) = (ubuntu_repos(machine, pkg_glibc), debian_repos(machine, pkg_glibc));
            if ubuntu_first {
                u.into_iter().chain(d).collect()
            } else {
                d.into_iter().chain(u).collect()
            }
        }
        Family::Fedora => {
            let (fc, el, _) = rpm_release_hint(release);
            if el.is_some() && fc.is_none() {
                el_repos(machine, pkg_glibc, el).into_iter().chain(fedora_repos(machine, pkg_glibc, None)).collect()
            } else {
                fedora_repos(machine, pkg_glibc, fc).into_iter().chain(el_repos(machine, pkg_glibc, None)).collect()
            }
        }
        Family::Suse => {
            let (_, el, _) = rpm_release_hint(release);
            opensuse_repos(machine, pkg_glibc).into_iter().chain(fedora_repos(machine, pkg_glibc, None)).chain(el_repos(machine, pkg_glibc, el)).collect()
        }
        Family::Arch => arch_repos(machine).into_iter().chain(debian_repos(machine, pkg_glibc)).collect(),
        // Origine inconnue (tar, Slackware…) : Debian puis Ubuntu, les plus complets.
        Family::Generic => debian_repos(machine, pkg_glibc).into_iter().chain(ubuntu_repos(machine, pkg_glibc)).collect(),
    }
}

// ---------------------------------------------------------------------------
// Téléchargements (HTTPS) avec cache
// ---------------------------------------------------------------------------

/// Messages de progression : (clé de traduction, précision).
pub type Progress = Arc<dyn Fn(&str, &str) + Send + Sync>;

pub struct Fetcher {
    agent: ureq::Agent,
    pub cache: PathBuf,
    progress: Progress,
    indexes: HashMap<String, Option<Arc<RepoIndex>>>,
    /// Échecs réseau par serveur (un serveur injoignable n'est plus interrogé).
    host_failures: HashMap<String, u32>,
    /// Réponses reçues : s'il n'y en a aucune et que plusieurs serveurs échouent, on est hors ligne.
    successes: u32,
}

fn host_of(url: &str) -> String {
    url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or("").to_string()
}

fn safe_name(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect()
}

impl Fetcher {
    pub fn new(progress: Progress) -> Fetcher {
        let cache = crate::deps::cache_dir().unwrap_or_else(|| std::env::temp_dir().join("tamga-cache"));
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(60))
            .redirects(8)
            .try_proxy_from_env(true)
            .user_agent(&format!("TAMGA/{} (Linux package converter)", crate::TAMGA_VERSION))
            .build();
        Fetcher { agent, cache, progress, indexes: HashMap::new(), host_failures: HashMap::new(), successes: 0 }
    }

    pub fn offline(&self) -> bool {
        self.successes == 0 && self.host_failures.len() >= 3
    }

    fn say(&self, key: &str, detail: &str) {
        (self.progress)(key, detail);
    }

    /// Télécharge `url` dans `dest`. Erreur « 404 » si absent.
    fn download_to(&mut self, url: &str, dest: &Path) -> Result<()> {
        if !url.starts_with("https://") {
            bail!("adresse non sécurisée refusée : {url}");
        }
        let host = host_of(url);
        if self.offline() || self.host_failures.get(&host).copied().unwrap_or(0) >= 2 {
            bail!("hors ligne ({host})");
        }
        let resp = match self.agent.get(url).call() {
            Ok(r) => {
                self.successes += 1;
                r
            }
            Err(ureq::Error::Status(code, _)) => {
                self.successes += 1;
                bail!("HTTP {code}");
            }
            Err(e) => {
                *self.host_failures.entry(host).or_default() += 1;
                bail!("réseau : {e}");
            }
        };
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = dest.with_extension("part");
        let mut out = std::fs::File::create(&tmp)?;
        let mut reader = resp.into_reader().take(MAX_DOWNLOAD + 1);
        let n = std::io::copy(&mut reader, &mut out)?;
        if n > MAX_DOWNLOAD {
            let _ = std::fs::remove_file(&tmp);
            bail!("fichier trop gros");
        }
        out.flush()?;
        drop(out);
        std::fs::rename(&tmp, dest)?;
        Ok(())
    }

    /// Fichier en cache, retéléchargé s'il est plus vieux que `ttl` (0 = jamais).
    fn cached(&mut self, url: &str, ttl: u64) -> Result<PathBuf> {
        let digest = hex::encode(&Sha256::digest(url.as_bytes())[..8]);
        let file = url.rsplit('/').next().unwrap_or("x");
        let dest = self.cache.join("repos").join(format!("{digest}-{}", safe_name(file)));
        if let Ok(m) = std::fs::metadata(&dest) {
            let age = m.modified().ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs()).unwrap_or(u64::MAX);
            if ttl == 0 || age < ttl {
                return Ok(dest);
            }
        }
        match self.download_to(url, &dest) {
            Ok(()) => Ok(dest),
            // Hors ligne : une ancienne copie vaut mieux que rien.
            Err(e) if dest.exists() && self.offline() => {
                let _ = e;
                Ok(dest)
            }
            Err(e) => Err(e),
        }
    }

    /// Charge (ou télécharge) la liste des paquets d'un dépôt.
    pub fn index(&mut self, repo: &Repo) -> Option<Arc<RepoIndex>> {
        let key = format!("{}|{:?}", repo.bases.first().cloned().unwrap_or_default(), repo.kind);
        if let Some(v) = self.indexes.get(&key) {
            return v.clone();
        }
        self.say("bundle.index", &repo.label);
        let mut result = None;
        for base in &repo.bases {
            match self.load_index(base, &repo.kind) {
                Ok(idx) => {
                    result = Some(Arc::new(idx));
                    break;
                }
                Err(e) => {
                    log_debug(&format!("{} : {e:#}", repo.label));
                    if self.offline() {
                        break;
                    }
                }
            }
        }
        self.indexes.insert(key, result.clone());
        result
    }

    fn load_index(&mut self, base: &str, kind: &RepoKind) -> Result<RepoIndex> {
        let base = base.trim_end_matches('/');
        match kind {
            RepoKind::Deb { suite, component, arch } => {
                let url = format!("{base}/dists/{suite}/{component}/binary-{arch}/Packages.xz");
                let path = self.cached(&url, INDEX_TTL)?;
                let mut text = String::new();
                compress::auto_decoder(std::fs::File::open(&path)?)?.read_to_string(&mut text)?;
                Ok(parse_debian_packages(&text, base))
            }
            RepoKind::RpmMd { arches } => {
                let repomd_url = format!("{base}/repodata/repomd.xml");
                let repomd = std::fs::read_to_string(self.cached(&repomd_url, INDEX_TTL)?)?;
                let (href, sha) = parse_repomd(&repomd).context("repomd.xml sans primary")?;
                // Index réduit déjà calculé pour ce primary.xml (son nom contient son empreinte).
                let reduced = self.cache.join("repos").join(format!("{}-{}.json", hex::encode(&Sha256::digest(base.as_bytes())[..8]), safe_name(href.rsplit('/').next().unwrap_or("primary"))));
                if let Ok(b) = std::fs::read(&reduced) {
                    if let Ok(idx) = serde_json::from_slice::<RepoIndex>(&b) {
                        return Ok(idx);
                    }
                }
                let url = format!("{base}/{href}");
                let path = self.cached(&url, 0)?;
                if let Some(expected) = sha {
                    if file_sha256(&path)? != expected {
                        let _ = std::fs::remove_file(&path);
                        bail!("empreinte de primary.xml incorrecte");
                    }
                }
                let dec = compress::auto_decoder(std::fs::File::open(&path)?)?;
                let idx = parse_rpm_primary(BufReader::with_capacity(1 << 20, dec), base, arches)?;
                // Le gros fichier n'est plus utile : on garde l'index réduit.
                let _ = std::fs::remove_file(&path);
                if let Ok(b) = serde_json::to_vec(&idx) {
                    let _ = std::fs::write(&reduced, b);
                }
                Ok(idx)
            }
            RepoKind::Pacman { repo } => {
                let url = format!("{base}/{repo}.db");
                let path = self.cached(&url, INDEX_TTL)?;
                parse_pacman_db(std::fs::File::open(&path)?, base)
            }
        }
    }

    /// Télécharge un paquet (vérifié par son empreinte SHA-256) ; les paquets restent en cache.
    pub fn package(&mut self, p: &RepoPackage, label: &str) -> Result<PathBuf> {
        self.say("bundle.download", &format!("{} {} — {label}", p.name, p.version));
        let path = self.cached(&p.url, 0)?;
        if let Some(expected) = &p.sha256 {
            if !file_sha256(&path)?.eq_ignore_ascii_case(expected) {
                let _ = std::fs::remove_file(&path);
                bail!("empreinte SHA-256 incorrecte pour {}", p.name);
            }
        }
        Ok(path)
    }
}

pub fn file_sha256(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

fn log_debug(msg: &str) {
    if std::env::var_os("TAMGA_DEBUG").is_some() {
        eprintln!("[tamga] {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debian_packages() {
        let text = "Package: libicu70\nVersion: 70.1-2\nArchitecture: amd64\nProvides: libicu70-x\nFilename: pool/main/i/icu/libicu70_70.1-2_amd64.deb\nSize: 10\nSHA256: abc\nDescription: x\n continued line\n\nPackage: other\nFilename: pool/o/other.deb\n";
        let idx = parse_debian_packages(text, "https://archive.ubuntu.com/ubuntu/");
        let p = idx.get("libicu70").unwrap();
        assert_eq!(p.url, "https://archive.ubuntu.com/ubuntu/pool/main/i/icu/libicu70_70.1-2_amd64.deb");
        assert_eq!(p.sha256.as_deref(), Some("abc"));
        assert_eq!(idx.get("libicu70-x").unwrap().name, "libicu70");
        assert_eq!(idx.names_starting_with("libicu").len(), 1);
    }

    #[test]
    fn rpm_primary() {
        let xml = r#"<?xml version="1.0"?>
<metadata xmlns="http://linux.duke.edu/metadata/common" xmlns:rpm="http://linux.duke.edu/metadata/rpm" packages="2">
<package type="rpm">
  <name>libicu</name><arch>x86_64</arch>
  <version epoch="0" ver="74.2" rel="1.fc41"/>
  <checksum type="sha256" pkgid="YES">deadbeef</checksum>
  <size package="1234" installed="1" archive="1"/>
  <location href="Packages/l/libicu-74.2-1.fc41.x86_64.rpm"/>
  <format>
    <rpm:provides>
      <rpm:entry name="libicu" flags="EQ" epoch="0" ver="74.2" rel="1.fc41"/>
      <rpm:entry name="libicuuc.so.74()(64bit)"/>
      <rpm:entry name="libicuuc.so.74(ICU_74)(64bit)"/>
    </rpm:provides>
    <rpm:requires><rpm:entry name="libc.so.6()(64bit)"/></rpm:requires>
  </format>
</package>
<package type="rpm">
  <name>libicu</name><arch>i686</arch>
  <version epoch="0" ver="74.2" rel="1.fc41"/>
  <location href="Packages/l/libicu-74.2-1.fc41.i686.rpm"/>
  <format><rpm:provides><rpm:entry name="libicuuc.so.74"/></rpm:provides></format>
</package>
</metadata>"#;
        let idx = parse_rpm_primary(xml.as_bytes(), "https://dl.example/os/", &["x86_64".to_string()]).unwrap();
        let p = idx.providers("libicuuc.so.74", true);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].url, "https://dl.example/os/Packages/l/libicu-74.2-1.fc41.x86_64.rpm");
        assert_eq!(p[0].version, "74.2-1.fc41");
        assert_eq!(p[0].sha256.as_deref(), Some("deadbeef"));
        assert!(idx.providers("libc.so.6", true).is_empty());
        assert_eq!(idx.packages.len(), 1); // i686 ignoré
        let repomd = r#"<repomd><data type="filelists"><location href="repodata/f.xml.zst"/></data><data type="primary"><checksum type="sha256">aa11</checksum><location href="repodata/p-primary.xml.zst"/></data></repomd>"#;
        assert_eq!(parse_repomd(repomd), Some(("repodata/p-primary.xml.zst".to_string(), Some("aa11".to_string()))));
    }

    #[test]
    fn pacman_db() {
        let mut b = tar::Builder::new(Vec::new());
        let desc = "%FILENAME%\nlibjpeg-turbo-3.0.4-1-x86_64.pkg.tar.zst\n\n%NAME%\nlibjpeg-turbo\n\n%VERSION%\n3.0.4-1\n\n%SHA256SUM%\nff00\n\n%PROVIDES%\nlibjpeg=8.0.0\nlibjpeg.so=8-64\nlibturbojpeg.so=0-64\n\n";
        let mut h = tar::Header::new_gnu();
        h.set_size(desc.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, "libjpeg-turbo-3.0.4-1/desc", desc.as_bytes()).unwrap();
        let data = b.into_inner().unwrap();
        let idx = parse_pacman_db(&data[..], "https://geo.mirror.pkgbuild.com/extra/os/x86_64").unwrap();
        let p = idx.providers("libjpeg.so.8", true);
        assert_eq!(p[0].name, "libjpeg-turbo");
        assert_eq!(p[0].url, "https://geo.mirror.pkgbuild.com/extra/os/x86_64/libjpeg-turbo-3.0.4-1-x86_64.pkg.tar.zst");
        assert_eq!(idx.get("libjpeg").unwrap().name, "libjpeg-turbo");
    }

    #[test]
    fn release_hints_and_order() {
        assert_eq!(rpm_release_hint("1.fc41"), (Some(41), None, false));
        assert_eq!(rpm_release_hint("3.el9_4"), (None, Some(9), false));
        assert!(rpm_release_hint("lp156.2.1").2);
        let list = [("a", "2.31"), ("b", "2.35"), ("c", "2.39"), ("d", "2.42")];
        assert_eq!(order_by_glibc(&list, Some("2.34")), vec!["b", "c", "d", "a"]);
        assert_eq!(order_by_glibc(&list, None), vec!["d", "c", "b", "a"]);
        let r = repos_for(Family::Debian, "1.0-1ubuntu1", "", "", "x86_64", Some("2.34"));
        assert!(r[0].label.starts_with("Ubuntu 22.04"), "{}", r[0].label);
        let f = repos_for(Family::Fedora, "1", "2.fc40", "", "x86_64", None);
        assert_eq!(f[0].label, "Fedora 40");
    }
}
