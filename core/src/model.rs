//! Modèle commun à tous les formats : métadonnées, dépendances, scripts.
//! Common data model shared by all formats.

use crate::tree::Tree;
use serde::{Deserialize, Serialize};

/// Formats de paquets pris en charge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Deb,
    Rpm,
    /// Paquet Arch Linux / pacman (.pkg.tar.zst)
    Arch,
    /// Installateur autonome TAMGA (.sh)
    Sh,
    /// Paquet Slackware (.txz, .tgz, .tbz, .tlz)
    Slack,
    /// Archive tar simple (.tar, .tar.gz, .tar.xz…), à extraire à la racine
    Tar,
    /// Paquet Stampede Linux (.slp)
    Slp,
    /// Paquet Solaris SVR4 (.pkg « datastream ») — lecture seule
    Solaris,
}

impl Format {
    /// Formats que TAMGA sait écrire (Solaris : lecture seule).
    pub fn all() -> [Format; 7] {
        [Format::Deb, Format::Rpm, Format::Arch, Format::Sh, Format::Slack, Format::Tar, Format::Slp]
    }
    pub fn id(self) -> &'static str {
        match self {
            Format::Deb => "deb",
            Format::Rpm => "rpm",
            Format::Arch => "arch",
            Format::Sh => "sh",
            Format::Slack => "slack",
            Format::Tar => "tar",
            Format::Slp => "slp",
            Format::Solaris => "solaris",
        }
    }
    pub fn parse(s: &str) -> Option<Format> {
        match s.trim().trim_start_matches('.').to_ascii_lowercase().as_str() {
            "deb" => Some(Format::Deb),
            "rpm" => Some(Format::Rpm),
            "arch" | "pkg" | "pacman" | "pkg.tar.zst" => Some(Format::Arch),
            "sh" | "run" => Some(Format::Sh),
            "slack" | "slackware" | "txz" | "tgz" | "tbz" | "tlz" => Some(Format::Slack),
            "tar" | "tar.gz" | "tar.xz" | "tar.bz2" | "tar.zst" => Some(Format::Tar),
            "slp" | "stampede" => Some(Format::Slp),
            "solaris" | "svr4" => Some(Format::Solaris),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Format::Deb => ".deb",
            Format::Rpm => ".rpm",
            Format::Arch => ".pkg.tar.zst",
            Format::Sh => ".sh",
            Format::Slack => ".txz",
            Format::Tar => ".tar",
            Format::Slp => ".slp",
            Format::Solaris => ".pkg",
        }
    }
    /// Famille de dépendances naturelle du format.
    pub fn family(self) -> Family {
        match self {
            Format::Deb => Family::Debian,
            Format::Rpm => Family::Fedora,
            Format::Arch => Family::Arch,
            _ => Family::Generic,
        }
    }
    /// Formats sans gestion de dépendances par nom de paquet (on vérifie les bibliothèques).
    pub fn is_generic(self) -> bool {
        matches!(self, Format::Sh | Format::Slack | Format::Tar | Format::Slp | Format::Solaris)
    }
    /// TAMGA sait-il écrire ce format ?
    pub fn writable(self) -> bool {
        self != Format::Solaris
    }
}

/// Famille de distributions (règles de nommage des paquets).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Debian,
    Fedora,
    Suse,
    Arch,
    Generic,
}

impl Family {
    pub fn id(self) -> &'static str {
        match self {
            Family::Debian => "debian",
            Family::Fedora => "fedora",
            Family::Suse => "suse",
            Family::Arch => "arch",
            Family::Generic => "generic",
        }
    }
    pub fn parse(s: &str) -> Option<Family> {
        match s.to_ascii_lowercase().as_str() {
            "debian" | "ubuntu" | "deb" | "mint" => Some(Family::Debian),
            "fedora" | "rhel" | "rpm" | "redhat" => Some(Family::Fedora),
            "suse" | "opensuse" => Some(Family::Suse),
            "arch" | "manjaro" | "pacman" => Some(Family::Arch),
            "generic" | "sh" | "all" => Some(Family::Generic),
            _ => None,
        }
    }
    pub fn format(self) -> Format {
        match self {
            Family::Debian => Format::Deb,
            Family::Fedora | Family::Suse => Format::Rpm,
            Family::Arch => Format::Arch,
            Family::Generic => Format::Sh,
        }
    }
}

/// Opérateur de comparaison de version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl Op {
    pub fn parse(s: &str) -> Option<Op> {
        match s.trim() {
            "<<" | "<" => Some(Op::Lt),
            "<=" => Some(Op::Le),
            "=" | "==" => Some(Op::Eq),
            ">=" => Some(Op::Ge),
            ">>" | ">" => Some(Op::Gt),
            _ => None,
        }
    }
    pub fn deb(self) -> &'static str {
        match self {
            Op::Lt => "<<",
            Op::Le => "<=",
            Op::Eq => "=",
            Op::Ge => ">=",
            Op::Gt => ">>",
        }
    }
    pub fn std(self) -> &'static str {
        match self {
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Eq => "=",
            Op::Ge => ">=",
            Op::Gt => ">",
        }
    }
    /// Drapeaux RPMSENSE_*.
    pub fn rpm_flags(self) -> u32 {
        match self {
            Op::Lt => 2,
            Op::Le => 2 | 8,
            Op::Eq => 8,
            Op::Ge => 4 | 8,
            Op::Gt => 4,
        }
    }
    pub fn from_rpm_flags(f: u32) -> Option<Op> {
        match f & 0x0e {
            2 => Some(Op::Lt),
            10 => Some(Op::Le),
            8 => Some(Op::Eq),
            12 => Some(Op::Ge),
            4 => Some(Op::Gt),
            _ => None,
        }
    }
}

/// Une dépendance : nom + contrainte de version éventuelle (+ alternatives).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    #[serde(default)]
    pub op: Option<Op>,
    #[serde(default)]
    pub version: Option<String>,
    /// Alternatives acceptées (« a | b » en Debian).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Dependency>,
}

impl Dependency {
    pub fn simple(name: &str) -> Self {
        Dependency { name: name.to_string(), op: None, version: None, alternatives: vec![] }
    }
    pub fn with(name: &str, op: Op, version: &str) -> Self {
        Dependency { name: name.to_string(), op: Some(op), version: Some(version.to_string()), alternatives: vec![] }
    }

    /// Analyse « nom », « nom (>= 1.0) », « nom>=1.0 » ou « nom >= 1.0 ».
    pub fn parse_any(s: &str) -> Option<Dependency> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        // Style Debian : nom (op version)
        if let Some(open) = s.find('(') {
            if s.ends_with(')') && !s.starts_with('(') {
                let name = s[..open].trim();
                let inner = s[open + 1..s.len() - 1].trim();
                // « libfoo.so.1()(64bit) » : c'est un nom rpm, pas une contrainte.
                if !inner.is_empty() && !name.contains(".so") {
                    let (op, ver) = split_op(inner)?;
                    return Some(Dependency::with(name, op, ver));
                }
                return Some(Dependency::simple(s));
            }
        }
        for op_s in [">=", "<=", "==", ">>", "<<", "=", ">", "<"] {
            if let Some(i) = s.find(op_s) {
                let name = s[..i].trim();
                let ver = s[i + op_s.len()..].trim();
                if !name.is_empty() && !ver.is_empty() && !name.contains('(') {
                    return Some(Dependency::with(name, Op::parse(op_s)?, ver));
                }
            }
        }
        Some(Dependency::simple(s))
    }

    /// Texte lisible, style Debian.
    pub fn display(&self) -> String {
        let mut s = match (&self.op, &self.version) {
            (Some(op), Some(v)) => format!("{} ({} {})", self.name, op.deb(), v),
            _ => self.name.clone(),
        };
        for a in &self.alternatives {
            s.push_str(" | ");
            s.push_str(&a.display());
        }
        s
    }

    /// Texte style rpm/pacman : « nom >= version ».
    pub fn display_std(&self) -> String {
        match (&self.op, &self.version) {
            (Some(op), Some(v)) => format!("{} {} {}", self.name, op.std(), v),
            _ => self.name.clone(),
        }
    }
}

fn split_op(s: &str) -> Option<(Op, &str)> {
    for op_s in [">=", "<=", ">>", "<<", "==", "=", ">", "<"] {
        if let Some(rest) = s.strip_prefix(op_s) {
            return Some((Op::parse(op_s)?, rest.trim()));
        }
    }
    None
}

/// Style d'origine des scripts d'installation (ils n'ont pas les mêmes arguments).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScriptStyle {
    #[default]
    None,
    Deb,
    Rpm,
    /// Fichier .INSTALL d'Arch (fonctions pre_install, post_install…)
    Arch,
    /// Scripts simples lancés depuis « / » sans argument particulier
    /// (doinst.sh de Slackware, postinstall de Solaris).
    Plain,
}

/// Scripts exécutés à l'installation / la désinstallation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scripts {
    #[serde(default)]
    pub style: ScriptStyle,
    #[serde(default)]
    pub pre_install: Option<String>,
    #[serde(default)]
    pub post_install: Option<String>,
    #[serde(default)]
    pub pre_remove: Option<String>,
    #[serde(default)]
    pub post_remove: Option<String>,
    /// Fichier .INSTALL complet (style Arch).
    #[serde(default)]
    pub arch_install: Option<String>,
}

impl Scripts {
    pub fn is_empty(&self) -> bool {
        self.pre_install.is_none()
            && self.post_install.is_none()
            && self.pre_remove.is_none()
            && self.post_remove.is_none()
            && self.arch_install.is_none()
    }
}

/// Métadonnées d'un paquet, indépendantes du format.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    pub version: String,
    /// Révision (Debian), release (rpm), pkgrel (Arch).
    #[serde(default)]
    pub release: String,
    #[serde(default)]
    pub epoch: Option<u32>,
    /// Architecture canonique : x86_64, aarch64, i686, armv7, riscv64, ppc64le, s390x, noarch…
    pub arch: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub maintainer: String,
    #[serde(default)]
    pub section: String,
    #[serde(default)]
    pub depends: Vec<Dependency>,
    #[serde(default)]
    pub provides: Vec<Dependency>,
    #[serde(default)]
    pub conflicts: Vec<Dependency>,
    #[serde(default)]
    pub replaces: Vec<Dependency>,
    /// Famille d'origine des noms de dépendances.
    pub dep_family: Option<Family>,
    #[serde(default)]
    pub scripts: Scripts,
    /// Format d'origine (avant toute conversion).
    pub origin_format: Option<Format>,
}

/// Un paquet chargé en mémoire : métadonnées + arbre de fichiers.
pub struct Package {
    pub meta: Metadata,
    pub tree: Tree,
    /// Format du fichier lu.
    pub format: Format,
    /// Avertissements rencontrés pendant la lecture (clés de traduction).
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_deps() {
        let d = Dependency::parse_any("libc6 (>= 2.34)").unwrap();
        assert_eq!(d.name, "libc6");
        assert_eq!(d.op, Some(Op::Ge));
        assert_eq!(d.version.as_deref(), Some("2.34"));
        let d = Dependency::parse_any("glibc>=2.34").unwrap();
        assert_eq!((d.name.as_str(), d.op), ("glibc", Some(Op::Ge)));
        let d = Dependency::parse_any("libgtk-3.so.0()(64bit)").unwrap();
        assert_eq!(d.name, "libgtk-3.so.0()(64bit)");
        assert_eq!(d.op, None);
        let d = Dependency::parse_any("python3 >= 3.8").unwrap();
        assert_eq!((d.name.as_str(), d.version.as_deref()), ("python3", Some("3.8")));
    }
}
