//! Le système sur lequel TAMGA tourne : distribution, outils de paquets,
//! bibliothèques présentes, et interrogation des dépôts.
//! Host system detection and package-manager queries.

use crate::model::Family;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Default)]
pub struct SystemInfo {
    pub id: String,
    pub id_like: String,
    pub name: String,
    pub version_id: String,
    pub family: Option<Family>,
    pub arch: String,
    pub glibc: Option<String>,
    /// Outils trouvés : dpkg-query, apt-cache, rpm, dnf, zypper, pacman, ldconfig.
    pub tools: Vec<String>,
}

fn parse_os_release(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').trim_matches('\'').to_string()))
        .collect()
}

pub fn family_of(id: &str, id_like: &str) -> Option<Family> {
    let words: Vec<String> = std::iter::once(id).chain(id_like.split_whitespace()).map(|s| s.to_lowercase()).collect();
    for w in &words {
        let f = match w.as_str() {
            "debian" | "ubuntu" | "linuxmint" | "pop" | "elementary" | "zorin" | "kali" | "raspbian" | "deepin"
            | "neon" | "mx" | "devuan" | "pureos" | "parrot" | "kubuntu" | "xubuntu" | "lubuntu" => Family::Debian,
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "ol" | "nobara" | "ultramarine" | "amzn" | "mageia" => Family::Fedora,
            "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" | "suse" | "opensuse-slowroll" => Family::Suse,
            "arch" | "manjaro" | "endeavouros" | "garuda" | "cachyos" | "artix" | "arcolinux" => Family::Arch,
            _ => continue,
        };
        return Some(f);
    }
    if words.iter().any(|w| w.contains("suse")) {
        return Some(Family::Suse);
    }
    None
}

/// Cherche un exécutable dans le PATH et les dossiers sbin.
pub fn which(cmd: &str) -> Option<String> {
    let path = std::env::var("PATH").unwrap_or_default();
    let mut dirs: Vec<String> = path.split(':').map(String::from).collect();
    for d in ["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        dirs.push(d.to_string());
    }
    for d in dirs {
        let p = std::path::Path::new(&d).join(cmd);
        if p.is_file() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

impl SystemInfo {
    pub fn detect() -> SystemInfo {
        let text = std::fs::read_to_string("/etc/os-release")
            .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
            .unwrap_or_default();
        let m = parse_os_release(&text);
        let get = |k: &str| m.get(k).cloned().unwrap_or_default();
        let id = get("ID");
        let id_like = get("ID_LIKE");
        let tools = ["dpkg-query", "apt-cache", "rpm", "dnf", "zypper", "pacman", "ldconfig"]
            .iter()
            .filter(|t| which(t).is_some())
            .map(|t| t.to_string())
            .collect();
        let glibc = run("getconf", &["GNU_LIBC_VERSION"], 5)
            .and_then(|o| o.split_whitespace().nth(1).map(String::from));
        SystemInfo {
            family: family_of(&id, &id_like),
            id,
            id_like,
            name: if get("PRETTY_NAME").is_empty() { get("NAME") } else { get("PRETTY_NAME") },
            version_id: get("VERSION_ID"),
            arch: crate::version::host_arch(),
            glibc,
            tools,
        }
    }

    pub fn has(&self, tool: &str) -> bool {
        self.tools.iter().any(|t| t == tool)
    }
}

/// Lance une commande avec délai maximal (secondes). Retourne la sortie si succès.
pub fn run(cmd: &str, args: &[&str], timeout_s: u64) -> Option<String> {
    run_status(cmd, args, timeout_s).and_then(|(ok, out)| if ok { Some(out) } else { None })
}

/// Comme `run` mais retourne aussi le statut.
pub fn run_status(cmd: &str, args: &[&str], timeout_s: u64) -> Option<(bool, String)> {
    let exe = which(cmd)?;
    let mut child = Command::new(exe)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(timeout_s) {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            Err(_) => break None,
        }
    };
    let out = reader.join().unwrap_or_default();
    status.map(|s| (s.success(), out))
}

/// Cache `ldconfig -p` : soname → chemins (avec l'étiquette d'architecture).
#[derive(Default)]
pub struct LdCache {
    map: HashMap<String, Vec<(String, String)>>,
}

impl LdCache {
    pub fn load() -> LdCache {
        let mut c = LdCache::default();
        let Some(out) = run("ldconfig", &["-p"], 15) else { return c };
        for line in out.lines().skip(1) {
            // « 	libz.so.1 (libc6,x86-64) => /lib/x86_64-linux-gnu/libz.so.1 »
            let line = line.trim();
            let Some((left, path)) = line.split_once(" => ") else { continue };
            let Some((so, flags)) = left.split_once(" (") else { continue };
            c.map.entry(so.trim().to_string()).or_default().push((flags.trim_end_matches(')').to_string(), path.trim().to_string()));
        }
        c
    }

    /// Chemin d'une bibliothèque pour la bonne largeur (64 / 32 bits).
    pub fn find(&self, soname: &str, is64: bool) -> Option<String> {
        let list = self.map.get(soname)?;
        let want64 = |f: &str| f.contains("64") || f.contains("AArch64");
        list.iter().find(|(f, _)| want64(f) == is64).or_else(|| list.first()).map(|(_, p)| p.clone())
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Chemins à essayer pour retrouver le propriétaire d'un fichier (fusion /usr).
fn path_variants(p: &str) -> Vec<String> {
    let mut v = vec![p.to_string()];
    if let Ok(real) = std::fs::canonicalize(p) {
        v.push(real.to_string_lossy().to_string());
    }
    for (a, b) in [("/usr/lib/", "/lib/"), ("/usr/lib64/", "/lib64/"), ("/usr/bin/", "/bin/")] {
        if let Some(rest) = p.strip_prefix(a) {
            v.push(format!("{b}{rest}"));
        } else if let Some(rest) = p.strip_prefix(b) {
            v.push(format!("{a}{rest}"));
        }
    }
    v.dedup();
    v
}

/// Paquet installé qui fournit ce fichier, selon le gestionnaire local.
pub fn owner_of(sys: &SystemInfo, path: &str) -> Option<String> {
    for p in path_variants(path) {
        let r = match sys.family {
            Some(Family::Debian) if sys.has("dpkg-query") => run("dpkg-query", &["-S", &p], 10).and_then(|o| {
                o.lines().next().and_then(|l| l.split_once(": ")).map(|(pk, _)| pk.split(',').next().unwrap_or(pk).trim().to_string())
            }),
            Some(Family::Fedora) | Some(Family::Suse) if sys.has("rpm") => {
                run("rpm", &["-qf", "--qf", "%{NAME}\\n", &p], 10).and_then(|o| o.lines().next().map(String::from))
            }
            Some(Family::Arch) if sys.has("pacman") => run("pacman", &["-Qqo", &p], 10).and_then(|o| o.lines().next().map(String::from)),
            _ => None,
        };
        if let Some(mut name) = r.filter(|s| !s.is_empty() && !s.contains("not owned") && !s.contains("no path found")) {
            if let Some((n, _)) = name.split_once(':') {
                name = n.to_string(); // « libssl3:amd64 » → « libssl3 »
            }
            return Some(name);
        }
    }
    None
}

/// Paquet installé ? (nom exact ou capacité fournie)
pub fn is_installed(sys: &SystemInfo, name: &str) -> bool {
    match sys.family {
        Some(Family::Debian) => run("dpkg-query", &["-W", "-f=${Status}", name], 10)
            .map(|s| s.contains("install ok installed"))
            .unwrap_or(false),
        Some(Family::Fedora) | Some(Family::Suse) => run_status("rpm", &["-q", "--whatprovides", name], 10).map(|(ok, _)| ok).unwrap_or(false),
        Some(Family::Arch) => run_status("pacman", &["-T", name], 10).map(|(ok, _)| ok).unwrap_or(false),
        _ => false,
    }
}

/// Le dépôt de la distribution propose-t-il ce paquet / cette capacité ?
/// Retourne le nom du paquet qui la fournit.
pub fn available_in_repos(sys: &SystemInfo, name: &str) -> Option<String> {
    match sys.family {
        Some(Family::Debian) if sys.has("apt-cache") => {
            let out = run("apt-cache", &["policy", name], 20)?;
            let cand = out.lines().find_map(|l| l.trim().strip_prefix("Candidate:").map(|c| c.trim().to_string()))?;
            if cand.is_empty() || cand == "(none)" {
                // Paquet virtuel : apt-cache showpkg liste les fournisseurs.
                let sp = run("apt-cache", &["showpkg", name], 20)?;
                let mut lines = sp.lines().skip_while(|l| !l.starts_with("Reverse Provides:")).skip(1);
                return lines.next().and_then(|l| l.split_whitespace().next().map(String::from));
            }
            Some(name.to_string())
        }
        Some(Family::Fedora) if sys.has("dnf") => {
            let q = |extra: &[&str]| {
                let mut args = vec!["-q", "repoquery"];
                args.extend_from_slice(extra);
                args.extend_from_slice(&["--whatprovides", name, "--qf", "%{name}\\n"]);
                run("dnf", &args, 90)
            };
            let out = q(&["--cacheonly"]).filter(|o| !o.trim().is_empty()).or_else(|| q(&[]))?;
            out.lines().map(|l| l.trim().trim_end_matches("\\n")).find(|l| !l.is_empty()).map(String::from)
        }
        Some(Family::Suse) if sys.has("zypper") => {
            let out = run("zypper", &["--non-interactive", "--no-refresh", "-q", "search", "--provides", "--match-exact", "-t", "package", name], 60)?;
            out.lines()
                .filter(|l| l.contains('|'))
                .filter_map(|l| l.split('|').nth(1).map(|s| s.trim().to_string()))
                .find(|s| !s.is_empty() && s != "Name")
        }
        Some(Family::Arch) if sys.has("pacman") => {
            let out = run("pacman", &["-Sp", "--print-format", "%n", name], 30)?;
            out.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("::")).map(String::from)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families() {
        assert_eq!(family_of("linuxmint", "ubuntu debian"), Some(Family::Debian));
        assert_eq!(family_of("fedora", ""), Some(Family::Fedora));
        assert_eq!(family_of("opensuse-tumbleweed", "opensuse suse"), Some(Family::Suse));
        assert_eq!(family_of("endeavouros", "arch"), Some(Family::Arch));
        assert_eq!(family_of("weird", ""), None);
    }
}
