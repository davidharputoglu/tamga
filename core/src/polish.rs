//! Finitions propres à chaque format, pour que le paquet créé respecte les règles de la
//! distribution (et passe mieux lintian / namcap) : liens symboliques, fichier de copyright,
//! journal des modifications, licences.
//! Per-format finishing touches so converted packages follow distribution policy.

use crate::model::{Format, Metadata};
use crate::tree::{normalize, parent, Entry, Kind, Tree};
use anyhow::Result;
use std::io::Write;

/// Lien symbolique « au plus court » selon la règle Debian : relatif dans un même dossier de
/// premier niveau (/usr, /etc…), absolu entre deux dossiers de premier niveau.
pub fn debian_link(path: &str, target: &str) -> Option<String> {
    let dir = parent(path).unwrap_or("");
    let abs = if let Some(t) = target.strip_prefix('/') {
        normalize(t)?
    } else {
        normalize(&if dir.is_empty() { target.to_string() } else { format!("{dir}/{target}") })?
    };
    if abs.is_empty() {
        return None;
    }
    let top = |p: &str| p.split('/').next().unwrap_or("").to_string();
    if top(path) != top(&abs) {
        return Some(format!("/{abs}"));
    }
    let from: Vec<&str> = if dir.is_empty() { vec![] } else { dir.split('/').collect() };
    let to: Vec<&str> = abs.split('/').collect();
    let common = from.iter().zip(to.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(to[common..].iter().map(|s| s.to_string()));
    let rel = parts.join("/");
    Some(if rel.is_empty() { ".".into() } else { rel })
}

/// Date au format des journaux Debian : « Tue, 24 Nov 2026 08:00:00 +0000 ».
pub fn rfc2822(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if mo <= 2 { 1 } else { 0 };
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let mn = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(mo - 1) as usize];
    format!("{wd}, {d:02} {mn} {y} {:02}:{:02}:{:02} +0000", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn gzip(data: &[u8]) -> Result<Vec<u8>> {
    // Comme « gzip -9n » : sans nom de fichier ni date.
    let mut e = flate2::GzBuilder::new().mtime(0).write(Vec::new(), flate2::Compression::best());
    e.write_all(data)?;
    Ok(e.finish()?)
}

/// Textes des licences livrées avec le paquet (usr/share/licenses, copyright de la doc).
fn license_texts(tree: &Tree) -> String {
    let mut out = String::new();
    for (p, e) in tree.entries.iter() {
        let Kind::File(b) = &e.kind else { continue };
        let name = p.rsplit('/').next().unwrap_or("").to_ascii_uppercase();
        let is_lic = p.starts_with("usr/share/licenses/") || (p.starts_with("usr/share/doc/") && (name.starts_with("COPYING") || name.starts_with("LICENSE") || name == "COPYRIGHT"));
        if is_lic && b.size < 512 * 1024 && !p.ends_with(".gz") {
            if let Ok(t) = tree.store.read_all(b) {
                out.push_str(&format!("\n--- /{p} ---\n{}\n", String::from_utf8_lossy(&t)));
            }
        }
    }
    out
}

/// Retouches avant l'écriture du paquet. Retourne les clés d'avertissement éventuelles.
pub fn polish(format: Format, tree: &mut Tree, meta: &Metadata, version_text: &str) -> Result<Vec<String>> {
    let now = crate::tarutil::now();
    let origin = meta.origin_format.map(|f| f.label().to_string()).unwrap_or_else(|| "?".into());
    let maint = if meta.maintainer.trim().is_empty() { crate::default_maintainer() } else { meta.maintainer.trim().to_string() };
    match format {
        Format::Deb => {
            // 1. Liens symboliques au format Debian.
            let links: Vec<(String, String)> = tree
                .entries
                .iter()
                .filter_map(|(p, e)| match &e.kind {
                    Kind::Symlink(t) => Some((p.clone(), t.clone())),
                    _ => None,
                })
                .collect();
            let mut fixed = 0;
            for (p, t) in links {
                if let Some(n) = debian_link(&p, &t) {
                    if n != t {
                        if let Some(e) = tree.entries.get_mut(&p) {
                            e.kind = Kind::Symlink(n);
                            fixed += 1;
                        }
                    }
                }
            }
            crate::vlog!("finitions", "{fixed} lien(s) symbolique(s) mis au format Debian");
            // 2. Fichier de copyright et journal des modifications, s'ils manquent.
            let name = crate::version::deb_name(&meta.name);
            let doc = format!("usr/share/doc/{name}");
            if tree.get(&format!("{doc}/copyright")).is_none() {
                let text = format!(
                    "{name} — paquet converti par TAMGA depuis le format {origin} / converted by TAMGA from {origin}.\n\
                     Upstream: {url}\n\
                     License: {lic}\n\n\
                     Copyright notices and license texts shipped with the original package\n\
                     (avis de copyright et licences livrés avec le paquet d'origine) :\n{texts}",
                    url = if meta.url.is_empty() { "-" } else { &meta.url },
                    lic = if meta.license.is_empty() { "unknown" } else { &meta.license },
                    texts = { let t = license_texts(tree); if t.is_empty() { "\n(none / aucun)\n".to_string() } else { t } },
                );
                tree.add_file_bytes(&format!("{doc}/copyright"), text.as_bytes(), 0o644, now)?;
                crate::vlog!("finitions", "/{doc}/copyright ajouté");
            }
            let has_changelog = tree.entries.keys().any(|k| k.starts_with(&format!("{doc}/changelog")));
            if !has_changelog {
                let native = !version_text.contains('-');
                let file = if native { "changelog.gz" } else { "changelog.Debian.gz" };
                let text = format!(
                    "{name} ({version_text}) unstable; urgency=medium\n\n  * Converted from a {origin} package by TAMGA {}.\n\n -- {maint}  {}\n",
                    crate::TAMGA_VERSION,
                    rfc2822(now)
                );
                tree.add_file_bytes(&format!("{doc}/{file}"), &gzip(text.as_bytes())?, 0o644, now)?;
                crate::vlog!("finitions", "/{doc}/{file} ajouté");
            }
            // Le dossier de documentation doit exister avec de bons droits.
            if let Some(e) = tree.entries.get_mut(&doc) {
                e.mode = 0o755;
            } else {
                tree.insert(&doc, Entry::dir(0o755, now));
            }
        }
        Format::Arch => {
            // Licence non reconnue : Arch veut le texte dans /usr/share/licenses/<paquet>/.
            let name = crate::version::pacman_name(&meta.name);
            let dir = format!("usr/share/licenses/{name}");
            let has = tree.entries.keys().any(|k| k.starts_with(&format!("{dir}/")));
            if !has {
                let texts = license_texts(tree);
                let text = if texts.is_empty() {
                    format!("{name}: license unknown / licence inconnue.\nSee the original package or {}.\n", if meta.url.is_empty() { "its author" } else { &meta.url })
                } else {
                    texts
                };
                tree.add_file_bytes(&format!("{dir}/LICENSE"), text.as_bytes(), 0o644, now)?;
                crate::vlog!("finitions", "/{dir}/LICENSE ajouté");
            }
        }
        _ => {}
    }
    Ok(vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        assert_eq!(debian_link("usr/lib/libx.so", "/usr/lib/libx.so.1").as_deref(), Some("libx.so.1"));
        assert_eq!(debian_link("usr/bin/app", "../../usr/lib/app/app").as_deref(), Some("../lib/app/app"));
        assert_eq!(debian_link("usr/bin/app", "/opt/app/app").as_deref(), Some("/opt/app/app"));
        assert_eq!(debian_link("etc/x", "../usr/share/x").as_deref(), Some("/usr/share/x"));
        assert_eq!(debian_link("usr/share/doc/a/changelog", "../b/changelog").as_deref(), Some("../b/changelog"));
        assert_eq!(rfc2822(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(rfc2822(1_795_478_400), "Tue, 24 Nov 2026 00:00:00 +0000");
    }
}
