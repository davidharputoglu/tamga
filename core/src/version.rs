//! Noms, versions et architectures : conversions entre les règles de chaque format.
//! Names, versions and architectures across formats.

use crate::model::Metadata;

/// Architecture canonique (vocabulaire TAMGA ≈ uname -m) à partir d'un nom quelconque.
pub fn canonical_arch(a: &str) -> String {
    match a.trim().to_ascii_lowercase().as_str() {
        "amd64" | "x86_64" | "x86-64" | "x64" => "x86_64",
        "arm64" | "aarch64" => "aarch64",
        "i386" | "i486" | "i586" | "i686" | "x86" => "i686",
        "armhf" | "armv7hl" | "armv7h" | "armv7l" | "armv7" => "armv7",
        "riscv64" => "riscv64",
        "ppc64el" | "ppc64le" => "ppc64le",
        "s390x" => "s390x",
        "all" | "noarch" | "any" | "" => "noarch",
        other => return other.to_string(),
    }
    .to_string()
}

pub fn deb_arch(a: &str) -> String {
    match canonical_arch(a).as_str() {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "i686" => "i386",
        "armv7" => "armhf",
        "ppc64le" => "ppc64el",
        "noarch" => "all",
        other => return other.to_string(),
    }
    .to_string()
}

pub fn rpm_arch(a: &str) -> String {
    match canonical_arch(a).as_str() {
        "armv7" => "armv7hl",
        other => other,
    }
    .to_string()
}

pub fn pacman_arch(a: &str) -> String {
    match canonical_arch(a).as_str() {
        "armv7" => "armv7h",
        "noarch" => "any",
        other => other,
    }
    .to_string()
}

/// Architecture de la machine actuelle (canonique).
pub fn host_arch() -> String {
    canonical_arch(std::env::consts::ARCH)
}

fn clean(s: &str, allowed: impl Fn(char) -> bool, repl: char) -> String {
    let mut out: String = s.chars().map(|c| if allowed(c) { c } else { repl }).collect();
    while out.contains(&format!("{repl}{repl}")) {
        out = out.replace(&format!("{repl}{repl}"), &repl.to_string());
    }
    out.trim_matches(repl).to_string()
}

/// Nom de paquet valide pour Debian : [a-z0-9][a-z0-9+.-]+
pub fn deb_name(n: &str) -> String {
    let mut s = clean(&n.to_ascii_lowercase(), |c| c.is_ascii_alphanumeric() || "+.-".contains(c), '-');
    while s.starts_with(|c: char| !c.is_ascii_alphanumeric()) {
        s.remove(0);
    }
    if s.len() < 2 {
        s = format!("pkg-{s}");
    }
    s
}

pub fn rpm_name(n: &str) -> String {
    let s = clean(n, |c| c.is_ascii_alphanumeric() || "+._-".contains(c), '-');
    if s.is_empty() { "package".into() } else { s }
}

pub fn pacman_name(n: &str) -> String {
    let mut s = clean(&n.to_ascii_lowercase(), |c| c.is_ascii_alphanumeric() || "@._+-".contains(c), '-');
    while s.starts_with(['-', '.']) {
        s.remove(0);
    }
    if s.is_empty() { "package".into() } else { s }
}

/// Version amont « propre » : commence par un chiffre si possible.
fn upstream_base(v: &str) -> String {
    let v = v.trim();
    let v = v.strip_prefix('v').filter(|r| r.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(v);
    if v.is_empty() { "0".into() } else { v.to_string() }
}

/// Version complète Debian : [epoch:]upstream[-revision]
pub fn deb_version(m: &Metadata) -> String {
    let mut up = clean(&upstream_base(&m.version), |c| c.is_ascii_alphanumeric() || ".+~-".contains(c), '.');
    if !up.starts_with(|c: char| c.is_ascii_digit()) {
        up = format!("0~{up}");
    }
    let rel = clean(&m.release, |c| c.is_ascii_alphanumeric() || ".+~".contains(c), '.');
    // Un « - » dans la version amont exige une révision.
    let rel = if rel.is_empty() && up.contains('-') { "1".to_string() } else { rel };
    let mut s = String::new();
    if let Some(e) = m.epoch.filter(|e| *e > 0) {
        s.push_str(&format!("{e}:"));
    }
    s.push_str(&up);
    if !rel.is_empty() {
        s.push('-');
        s.push_str(&rel);
    }
    s
}

/// (version, release) valides pour rpm : pas de « - » ni de « : ».
pub fn rpm_version(m: &Metadata) -> (String, String) {
    let v = clean(&upstream_base(&m.version), |c| c.is_ascii_alphanumeric() || "._+~^".contains(c), '_');
    let r = clean(&m.release, |c| c.is_ascii_alphanumeric() || "._+~^".contains(c), '_');
    (if v.is_empty() { "0".into() } else { v }, if r.is_empty() { "1".into() } else { r })
}

/// (pkgver, pkgrel) valides pour pacman.
pub fn pacman_version(m: &Metadata) -> (String, String) {
    let v = clean(&upstream_base(&m.version), |c| c.is_ascii_alphanumeric() || "._+~".contains(c), '_');
    let digits: String = m.release.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rel = if digits.is_empty() || digits.chars().all(|c| c == '0') { "1".to_string() } else { digits };
    (if v.is_empty() { "0".into() } else { v }, rel)
}

/// Découpe une version Debian complète en (epoch, upstream, revision).
pub fn split_deb_version(v: &str) -> (Option<u32>, String, String) {
    let (epoch, rest) = match v.split_once(':') {
        Some((e, r)) if e.chars().all(|c| c.is_ascii_digit()) && !e.is_empty() => (e.parse().ok(), r),
        _ => (None, v),
    };
    match rest.rsplit_once('-') {
        Some((up, rev)) => (epoch, up.to_string(), rev.to_string()),
        None => (epoch, rest.to_string(), String::new()),
    }
}

/// Découpe « [epoch:]pkgver-pkgrel » (Arch).
pub fn split_pacman_version(v: &str) -> (Option<u32>, String, String) {
    split_deb_version(v)
}

/// Garde la partie « amont » numérique d'une version de dépendance
/// (ex. « 2.34-0ubuntu3 » → « 2.34 », « 1:3.0.13 » → « 3.0.13 »).
pub fn portable_dep_version(v: &str) -> Option<String> {
    let (_, up, _) = split_deb_version(v);
    let core: String = up
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .trim_end_matches('.')
        .to_string();
    if core.is_empty() || !core.contains(|c: char| c.is_ascii_digit()) { None } else { Some(core) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: &str, r: &str, e: Option<u32>) -> Metadata {
        Metadata { version: v.into(), release: r.into(), epoch: e, ..Default::default() }
    }

    #[test]
    fn versions() {
        assert_eq!(deb_version(&m("1.2.3", "1ubuntu2", Some(1))), "1:1.2.3-1ubuntu2");
        assert_eq!(deb_version(&m("v2.0", "", None)), "2.0");
        assert_eq!(deb_version(&m("2.0-beta", "", None)), "2.0-beta-1");
        assert_eq!(rpm_version(&m("2.0-beta", "1ubuntu2", None)), ("2.0_beta".into(), "1ubuntu2".into()));
        assert_eq!(pacman_version(&m("1.0:rc1", "3ubuntu1", None)), ("1.0_rc1".into(), "3".into()));
        assert_eq!(split_deb_version("1:2.3-4"), (Some(1), "2.3".into(), "4".into()));
        assert_eq!(portable_dep_version("2.34-0ubuntu3"), Some("2.34".into()));
        assert_eq!(portable_dep_version("1:3.0.13"), Some("3.0.13".into()));
        assert_eq!(portable_dep_version("${binary:Version}"), None);
    }

    #[test]
    fn names_and_arch() {
        assert_eq!(deb_name("My App_2"), "my-app-2");
        assert_eq!(pacman_name("-Foo"), "foo");
        assert_eq!(deb_arch("x86_64"), "amd64");
        assert_eq!(rpm_arch("amd64"), "x86_64");
        assert_eq!(pacman_arch("all"), "any");
        assert_eq!(canonical_arch("armhf"), "armv7");
    }
}
