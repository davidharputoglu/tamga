//! Installation directe d'un paquet converti (comme « alien -i » / AlienGUI).
//! Direct installation of a converted package with the system's package manager.

use crate::distro::{which, SystemInfo};
use crate::model::{Family, Format};
use anyhow::{bail, Result};
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

/// Comment obtenir les droits administrateur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// Fenêtre graphique de mot de passe (interface).
    Pkexec,
    /// Mot de passe dans le terminal (mode texte).
    Sudo,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstallPlan {
    /// Commande complète à lancer.
    pub argv: Vec<String>,
    /// Même commande, lisible (affichée à l'utilisateur).
    pub display: String,
}

fn is_root() -> bool {
    std::env::var("USER").map(|u| u == "root").unwrap_or(false)
        || Command::new("id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0").unwrap_or(false)
}

fn exists(p: &str) -> bool {
    Path::new(p).exists()
}

/// Ce système sait-il installer ce format ? Retourne la commande (sans élévation).
pub fn native_command(format: Format, file: &str, sys: &SystemInfo) -> Option<Vec<String>> {
    let s = |v: &[&str]| -> Vec<String> { v.iter().map(|x| x.to_string()).collect() };
    let fam = sys.family;
    match format {
        Format::Deb if fam == Some(Family::Debian) || (fam.is_none() && which("dpkg").is_some()) => {
            if which("apt-get").is_some() {
                Some(s(&["env", "DEBIAN_FRONTEND=noninteractive", "apt-get", "install", "-y", file]))
            } else {
                Some(s(&["dpkg", "-i", file]))
            }
        }
        Format::Rpm if matches!(fam, Some(Family::Fedora) | Some(Family::Suse)) || (fam.is_none() && which("rpm").is_some() && which("dpkg").is_none()) => {
            if which("dnf").is_some() {
                Some(s(&["dnf", "install", "-y", file]))
            } else if which("zypper").is_some() {
                Some(s(&["zypper", "--non-interactive", "install", "--allow-unsigned-rpm", file]))
            } else if which("yum").is_some() {
                Some(s(&["yum", "install", "-y", file]))
            } else {
                Some(s(&["rpm", "-Uvh", file]))
            }
        }
        Format::Arch if which("pacman").is_some() && fam != Some(Family::Debian) => Some(s(&["pacman", "-U", "--noconfirm", file])),
        Format::Slack if exists("/sbin/upgradepkg") => Some(s(&["/sbin/upgradepkg", "--install-new", file])),
        Format::Slack if exists("/sbin/installpkg") => Some(s(&["/sbin/installpkg", file])),
        Format::Sh => Some(s(&["sh", file, "--yes"])),
        _ => None,
    }
}

/// Formats installables directement sur cette machine.
pub fn installable_formats(sys: &SystemInfo) -> Vec<Format> {
    Format::all().into_iter().filter(|f| native_command(*f, "/x", sys).is_some()).collect()
}

/// Prépare la commande d'installation.
pub fn plan(format: Format, file: &Path, sys: &SystemInfo, elevation: Elevation) -> Result<InstallPlan> {
    let abs = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let abs = abs.to_string_lossy().to_string();
    let Some(mut argv) = native_command(format, &abs, sys) else { bail!("err.install_not_here") };
    if !is_root() {
        match elevation {
            Elevation::Pkexec => {
                if which("pkexec").is_none() {
                    bail!("err.no_pkexec");
                }
                argv.insert(0, "pkexec".into());
            }
            Elevation::Sudo => argv.insert(0, "sudo".into()),
        }
    }
    let display = argv.iter().map(|a| if a.contains(' ') { format!("'{a}'") } else { a.clone() }).collect::<Vec<_>>().join(" ");
    Ok(InstallPlan { argv, display })
}

/// Lance l'installation ; chaque ligne affichée est transmise à `on_line`.
/// Retourne le code de sortie.
pub fn run(plan: &InstallPlan, mut on_line: impl FnMut(String) + Send + 'static) -> Result<i32> {
    let mut child = Command::new(&plan.argv[0])
        .args(&plan.argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let mut threads = Vec::new();
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        threads.push(std::thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        }));
    }
    if let Some(err) = child.stderr.take() {
        let tx = tx.clone();
        threads.push(std::thread::spawn(move || {
            for l in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        }));
    }
    drop(tx);
    for l in rx {
        on_line(l);
    }
    for t in threads {
        let _ = t.join();
    }
    let status = child.wait()?;
    let code = status.code().unwrap_or(-1);
    // pkexec : 126 = fenêtre fermée / refus, 127 = mot de passe incorrect.
    if plan.argv.first().map(|a| a == "pkexec").unwrap_or(false) && (code == 126 || code == 127) {
        bail!("err.install_cancelled");
    }
    Ok(code)
}
