//! Vérification d'un paquet créé (option « --test » d'Alien).
//! Package check: TAMGA re-reads the package, then the distribution's own checker
//! (lintian for .deb, rpmlint for .rpm, namcap for Arch) reviews it when installed.

use crate::distro::{self, SystemInfo};
use crate::model::{Family, Format};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Une ligne du rapport. `key` + `params` : texte traduit par l'interface ; sinon `text` brut.
#[derive(Clone, Debug, Serialize)]
pub struct CheckLine {
    /// « error », « warning », « info » ou « ok ».
    pub level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct CheckReport {
    pub lines: Vec<CheckLine>,
    /// Outil de la distribution utilisé (lintian, rpmlint, namcap).
    pub tool: Option<String>,
    /// Outil absent : commande pour l'installer.
    pub tool_missing: Option<String>,
    pub errors: usize,
    pub warnings: usize,
}

impl CheckReport {
    fn msg(&mut self, level: &str, key: &str, params: &[(&str, String)]) {
        let params: BTreeMap<String, String> = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let text = format!("{key} {}", params.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "));
        self.push(CheckLine { level: level.into(), key: Some(key.into()), params, text });
    }

    fn raw(&mut self, level: &str, text: &str) {
        self.push(CheckLine { level: level.into(), key: None, params: BTreeMap::new(), text: text.into() });
    }

    fn push(&mut self, l: CheckLine) {
        match l.level.as_str() {
            "error" => self.errors += 1,
            "warning" => self.warnings += 1,
            _ => {}
        }
        crate::vlog!("vérification", "[{}] {}", l.level, l.text);
        self.lines.push(l);
    }
}

/// Niveau d'une ligne de lintian / rpmlint / namcap : « E: », « W: », « I: »…
fn level_of(line: &str) -> Option<&'static str> {
    let l = line.trim_start();
    let tag = |t: &str| l.starts_with(&format!("{t}: ")) || l.contains(&format!(": {t}: ")) || l.contains(&format!(" {t}: "));
    if tag("E") {
        Some("error")
    } else if tag("W") {
        Some("warning")
    } else if tag("I") || tag("N") || tag("P") || tag("X") || tag("O") || tag("C") || tag("M") {
        Some("info")
    } else {
        None
    }
}

/// Commande pour installer l'outil de contrôle sur cette machine.
fn install_hint(tool: &str, sys: &SystemInfo) -> Option<String> {
    let pm = match sys.family {
        Some(Family::Debian) => "sudo apt install",
        Some(Family::Fedora) => "sudo dnf install",
        Some(Family::Suse) => "sudo zypper install",
        Some(Family::Arch) => "sudo pacman -S",
        _ => return None,
    };
    // namcap et lintian n'existent que sur leur famille d'origine.
    match (tool, sys.family) {
        ("namcap", Some(Family::Arch)) | ("lintian", Some(Family::Debian)) | ("lintian", Some(Family::Arch)) => {}
        ("rpmlint", _) => {}
        ("namcap", _) | ("lintian", _) => return None,
        _ => {}
    }
    Some(format!("{pm} {tool}"))
}

/// Vérifie le paquet `path` au format `format`.
pub fn check(path: &Path, format: Format, sys: &SystemInfo) -> CheckReport {
    let mut r = CheckReport::default();
    crate::vlog!("vérification", "paquet {} ({:?})", path.display(), format);

    // 1. TAMGA relit le paquet avec ses propres lecteurs.
    match crate::convert::load(path) {
        Ok(pkg) if pkg.format == format || (format == Format::Tar && pkg.format == Format::Slack) => {
            let m = &pkg.meta;
            r.msg(
                "ok",
                "check.reopened",
                &[
                    ("files", pkg.tree.file_count().to_string()),
                    ("name", m.name.clone()),
                    ("version", m.version.clone()),
                    ("arch", m.arch.clone()),
                ],
            );
            if pkg.tree.file_count() == 0 {
                r.msg("warning", "check.empty", &[]);
            }
        }
        Ok(pkg) => r.msg("error", "check.wrong_format", &[("format", format.label().into()), ("found", pkg.format.label().into())]),
        Err(e) => r.msg("error", "check.reopen_failed", &[("error", format!("{e:#}"))]),
    }
    let p = path.to_string_lossy().to_string();

    // 2. Contrôles rapides des outils de base, s'ils sont là.
    match format {
        Format::Deb if distro::which("dpkg-deb").is_some() => match distro::run_status("dpkg-deb", &["--info", &p], 60) {
            Some((true, _)) => r.msg("ok", "check.native_ok", &[("tool", "dpkg-deb".into())]),
            _ => r.msg("error", "check.native_failed", &[("tool", "dpkg-deb".into())]),
        },
        Format::Rpm if distro::which("rpm").is_some() => match distro::run_status("rpm", &["-K", "--nosignature", &p], 60) {
            Some((true, _)) => r.msg("ok", "check.digest_ok", &[]),
            _ => r.msg("error", "check.digest_bad", &[]),
        },
        _ => {}
    }

    // 3. L'outil de contrôle officiel de la distribution.
    let tool = match format {
        Format::Deb => Some("lintian"),
        Format::Rpm => Some("rpmlint"),
        Format::Arch => Some("namcap"),
        _ => None,
    };
    if let Some(tool) = tool {
        if distro::which(tool).is_none() {
            r.tool_missing = install_hint(tool, sys);
            r.msg("info", "check.tool_missing", &[("tool", tool.into()), ("cmd", r.tool_missing.clone().unwrap_or_default())]);
        } else {
            r.tool = Some(tool.to_string());
            let args: Vec<&str> = match tool {
                "lintian" => vec!["--no-tag-display-limit", &p],
                _ => vec![&p],
            };
            match distro::run_full(tool, &args, 300) {
                Some((code, out, err)) => {
                    let found = out.lines().any(|l| level_of(l).is_some());
                    if found || code == Some(0) {
                        let before = (r.errors, r.warnings);
                        for line in out.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()) {
                            r.raw(level_of(line).unwrap_or("info"), line);
                        }
                        if (r.errors, r.warnings) == before {
                            r.msg("ok", "check.tool_ok", &[("tool", tool.into())]);
                        }
                    } else {
                        // L'outil lui-même a échoué (mal installé, plantage…) : on le dit, sans conclure.
                        let tail: Vec<&str> = err.lines().chain(out.lines()).filter(|l| !l.trim().is_empty()).collect();
                        let tail = tail[tail.len().saturating_sub(3)..].join(" | ");
                        r.msg("warning", "check.tool_failed", &[("tool", tool.into()), ("error", tail)]);
                    }
                }
                None => r.msg("warning", "check.tool_failed", &[("tool", tool.into()), ("error", String::new())]),
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(level_of("E: foo: no-copyright-file"), Some("error"));
        assert_eq!(level_of("W: foo: binary-without-manpage usr/bin/x"), Some("warning"));
        assert_eq!(level_of("foo.x86_64: E: no-changelogname-tag"), Some("error"));
        assert_eq!(level_of("foo W: Dependency included and not needed ('glibc')"), Some("warning"));
        assert_eq!(level_of("1 packages and 0 specfiles checked; 0 errors, 2 warnings."), None);
        assert_eq!(level_of("I: foo: hardening-no-bindnow"), Some("info"));
    }

    #[test]
    fn reopen_check() {
        let mut t = crate::tree::Tree::new().unwrap();
        t.add_file_bytes("usr/bin/hi", b"#!/bin/sh\necho hi\n", 0o755, 1).unwrap();
        let m = crate::model::Metadata { name: "hi".into(), version: "1.0".into(), release: "1".into(), arch: "noarch".into(), summary: "Hi".into(), ..Default::default() };
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hi.tar.gz");
        crate::slack::write_tar(&t, crate::compress::Codec::Gzip, &p).unwrap();
        let _ = m;
        let r = check(&p, Format::Tar, &SystemInfo::default());
        assert_eq!(r.errors, 0, "{:?}", r.lines);
        assert_eq!(r.lines[0].key.as_deref(), Some("check.reopened"));
    }
}
