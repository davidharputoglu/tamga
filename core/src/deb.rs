//! Paquets Debian / Ubuntu (.deb) : lecture et écriture.
//! Debian package (.deb) reader and writer.

use crate::ar;
use crate::compress::{self, Codec};
use crate::model::{Dependency, Family, Format, Metadata, Package, ScriptStyle, Scripts};
use crate::tarutil::{self, Sink, TarOpts};
use crate::tree::{Kind, Tree};
use crate::version;
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Champs d'un fichier « control » (ordre conservé).
pub fn parse_control(text: &str) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = fields.last_mut() {
                last.1.push('\n');
                last.1.push_str(line);
            }
        } else if let Some((k, v)) = line.split_once(':') {
            fields.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    fields
}

fn field<'a>(f: &'a [(String, String)], k: &str) -> Option<&'a str> {
    f.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
}

/// Analyse une liste de dépendances Debian : « a (>= 1), b | c, d:any ».
pub fn parse_dep_list(s: &str) -> Vec<Dependency> {
    let mut out = Vec::new();
    for group in s.split(',') {
        let mut alts: Vec<Dependency> = group
            .split('|')
            .filter_map(|a| {
                let a = a.trim();
                // Retire les restrictions d'architecture / profils : « foo [amd64] <!nocheck> »
                // (uniquement hors des parenthèses, pour garder « (<< 2.0) »).
                let a = strip_restrictions(a);
                // « foo (>= 1) » ; on enlève le qualificatif « :any » du nom.
                let mut d = Dependency::parse_any(&a)?;
                if let Some((n, _)) = d.name.split_once(':') {
                    d.name = n.to_string();
                }
                if d.name.starts_with("${") {
                    return None;
                }
                Some(d)
            })
            .collect();
        if alts.is_empty() {
            continue;
        }
        let mut first = alts.remove(0);
        first.alternatives = alts;
        out.push(first);
    }
    out
}

/// Enlève « [arch] » et « <profil> » situés hors des parenthèses.
fn strip_restrictions(a: &str) -> String {
    let mut out = String::new();
    let (mut paren, mut skip_until) = (0i32, None::<char>);
    for c in a.chars() {
        if let Some(end) = skip_until {
            if c == end {
                skip_until = None;
            }
            continue;
        }
        match c {
            '(' => paren += 1,
            ')' => paren -= 1,
            '[' if paren == 0 => {
                skip_until = Some(']');
                continue;
            }
            '<' if paren == 0 => {
                skip_until = Some('>');
                continue;
            }
            _ => {}
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// Lit un fichier .deb.
pub fn read(path: &Path) -> Result<Package> {
    let mut f = File::open(path).with_context(|| format!("ouverture de {}", path.display()))?;
    let members = ar::list(&mut f)?;
    if members.first().map(|m| m.name.as_str()) != Some("debian-binary") {
        bail!("ce fichier n'est pas un paquet .deb valide");
    }
    let control_m = members.iter().find(|m| m.name.starts_with("control.tar")).context("control.tar manquant")?;
    let data_m = members.iter().find(|m| m.name.starts_with("data.tar")).context("data.tar manquant")?;

    let mut tree = Tree::new()?;
    let mut warnings = Vec::new();

    // --- control.tar : métadonnées et scripts
    f.seek(SeekFrom::Start(control_m.offset))?;
    let ctl_reader = BufReader::new((&mut f).take(control_m.size));
    let mut ctl_dec = compress::decoder(Codec::from_name(&control_m.name), ctl_reader)?;
    let mut dummy = Tree::new()?;
    let (ctl_files, _) = tarutil::read_tar_into(&mut ctl_dec, &mut dummy, |_| Sink::Keep)?;
    drop(ctl_dec);
    let get = |n: &str| ctl_files.iter().find(|(p, _)| p == n).map(|(_, d)| String::from_utf8_lossy(d).to_string());
    let control = get("control").context("fichier control manquant")?;
    let fields = parse_control(&control);

    // --- data.tar : fichiers installés
    f.seek(SeekFrom::Start(data_m.offset))?;
    let data_reader = BufReader::with_capacity(256 * 1024, (&mut f).take(data_m.size));
    let mut data_dec = compress::decoder(Codec::from_name(&data_m.name), data_reader)?;
    let (_, special) = tarutil::read_tar_into(&mut data_dec, &mut tree, |_| Sink::Tree)?;
    if special > 0 {
        warnings.push("warn.special_files".into());
    }

    // Fichiers de configuration
    if let Some(conf) = get("conffiles") {
        for line in conf.lines() {
            let p = line.trim().trim_start_matches("remove-on-upgrade ").trim_start_matches('/');
            if let Some(e) = tree.entries.get_mut(p) {
                e.config = true;
            }
        }
    }

    let (epoch, up, rev) = version::split_deb_version(field(&fields, "Version").unwrap_or("0"));
    let desc = field(&fields, "Description").unwrap_or("");
    let (summary, long) = match desc.split_once('\n') {
        Some((s, l)) => (s.trim().to_string(), clean_long_description(l)),
        None => (desc.trim().to_string(), String::new()),
    };
    let mut depends = Vec::new();
    for k in ["Pre-Depends", "Depends"] {
        if let Some(v) = field(&fields, k) {
            depends.extend(parse_dep_list(v));
        }
    }
    let scripts = Scripts {
        style: ScriptStyle::Deb,
        pre_install: get("preinst"),
        post_install: get("postinst"),
        pre_remove: get("prerm"),
        post_remove: get("postrm"),
        arch_install: None,
    };
    if get("templates").is_some() || get("config").is_some() {
        warnings.push("warn.debconf".into());
    }
    let meta = Metadata {
        name: field(&fields, "Package").unwrap_or("package").to_string(),
        version: up,
        release: rev,
        epoch,
        arch: version::canonical_arch(field(&fields, "Architecture").unwrap_or("all")),
        summary,
        description: long,
        license: String::new(),
        url: field(&fields, "Homepage").unwrap_or("").to_string(),
        maintainer: field(&fields, "Maintainer").unwrap_or("").to_string(),
        section: field(&fields, "Section").unwrap_or("").to_string(),
        depends,
        provides: field(&fields, "Provides").map(parse_dep_list).unwrap_or_default(),
        conflicts: field(&fields, "Conflicts").map(parse_dep_list).unwrap_or_default(),
        replaces: field(&fields, "Replaces").map(parse_dep_list).unwrap_or_default(),
        dep_family: Some(Family::Debian),
        scripts,
        origin_format: Some(Format::Deb),
    };
    Ok(Package { meta, tree, format: Format::Deb, warnings })
}

fn clean_long_description(l: &str) -> String {
    l.lines()
        .map(|x| {
            let t = x.strip_prefix(' ').unwrap_or(x);
            if t.trim() == "." { "" } else { t }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Ce que l'écrivain .deb doit produire, en plus des métadonnées.
pub struct DebWriteOpts {
    /// Dépendances finales, déjà traduites (style Debian).
    pub depends: Vec<Dependency>,
    pub provides: Vec<Dependency>,
    pub conflicts: Vec<Dependency>,
    pub replaces: Vec<Dependency>,
    /// Scripts finaux (déjà adaptés au style Debian).
    pub scripts: Scripts,
    /// Déclencher ldconfig après installation.
    pub ldconfig_trigger: bool,
}

fn format_description(summary: &str, description: &str, name: &str) -> String {
    let sum = summary.lines().next().unwrap_or("").trim();
    let sum = if sum.is_empty() { name } else { sum };
    let mut s = sum.to_string();
    let long = description.trim();
    if !long.is_empty() {
        for line in long.lines() {
            let l = line.trim_end();
            if l.trim().is_empty() {
                s.push_str("\n .");
            } else {
                s.push_str("\n ");
                s.push_str(l);
            }
        }
    }
    s
}

fn single_line(s: &str) -> String {
    s.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

fn join_deps(d: &[Dependency]) -> String {
    d.iter().map(|x| x.display()).collect::<Vec<_>>().join(", ")
}

/// Génère le texte du fichier control.
pub fn control_text(meta: &Metadata, tree: &Tree, o: &DebWriteOpts) -> String {
    let installed_kb = (tree.total_size() + 1023) / 1024 + tree.entries.len() as u64;
    let mut c = String::new();
    c.push_str(&format!("Package: {}\n", version::deb_name(&meta.name)));
    c.push_str(&format!("Version: {}\n", version::deb_version(meta)));
    c.push_str(&format!("Architecture: {}\n", version::deb_arch(&meta.arch)));
    let maint = if meta.maintainer.trim().is_empty() { crate::DEFAULT_MAINTAINER.to_string() } else { single_line(&meta.maintainer) };
    c.push_str(&format!("Maintainer: {maint}\n"));
    c.push_str(&format!("Installed-Size: {installed_kb}\n"));
    if !o.depends.is_empty() {
        c.push_str(&format!("Depends: {}\n", join_deps(&o.depends)));
    }
    if !o.provides.is_empty() {
        c.push_str(&format!("Provides: {}\n", join_deps(&o.provides)));
    }
    if !o.conflicts.is_empty() {
        c.push_str(&format!("Conflicts: {}\n", join_deps(&o.conflicts)));
    }
    if !o.replaces.is_empty() {
        c.push_str(&format!("Replaces: {}\n", join_deps(&o.replaces)));
    }
    // La section n'a de sens que pour un paquet venant de Debian.
    let section = if meta.dep_family == Some(Family::Debian) && !meta.section.trim().is_empty() && !meta.section.contains(' ') {
        meta.section.to_lowercase()
    } else {
        "misc".to_string()
    };
    c.push_str(&format!("Section: {section}\n"));
    c.push_str("Priority: optional\n");
    if !meta.url.trim().is_empty() {
        c.push_str(&format!("Homepage: {}\n", single_line(&meta.url)));
    }
    c.push_str(&format!("Description: {}\n", format_description(&meta.summary, &meta.description, &meta.name)));
    c
}

/// Écrit un .deb.
pub fn write(meta: &Metadata, tree: &Tree, o: &DebWriteOpts, out: &Path) -> Result<()> {
    let mtime = tarutil::now();
    let opts = TarOpts { dot_prefix: true, root_entry: true, numeric_owner_fallback: true };

    // --- control.tar.xz
    let mut ctl = Vec::new();
    {
        let enc = compress::encoder(Codec::Xz, &mut ctl, Some(6))?;
        let mut b = tar::Builder::new(enc);
        tarutil::append_root(&mut b, mtime)?;
        tarutil::append_bytes(&mut b, "./control", control_text(meta, tree, o).as_bytes(), 0o644, mtime)?;
        let mut md5sums = String::new();
        for (p, e) in tree.entries.iter() {
            if let Kind::File(blob) = &e.kind {
                md5sums.push_str(&format!("{}  {}\n", blob.md5_hex(), p));
            }
        }
        tarutil::append_bytes(&mut b, "./md5sums", md5sums.as_bytes(), 0o644, mtime)?;
        let conffiles: Vec<String> =
            tree.entries.iter().filter(|(_, e)| e.config && e.is_file()).map(|(p, _)| format!("/{p}\n")).collect();
        if !conffiles.is_empty() {
            tarutil::append_bytes(&mut b, "./conffiles", conffiles.concat().as_bytes(), 0o644, mtime)?;
        }
        if o.ldconfig_trigger {
            tarutil::append_bytes(&mut b, "./triggers", b"activate-noawait ldconfig\n", 0o644, mtime)?;
        }
        for (name, s) in [
            ("preinst", &o.scripts.pre_install),
            ("postinst", &o.scripts.post_install),
            ("prerm", &o.scripts.pre_remove),
            ("postrm", &o.scripts.post_remove),
        ] {
            if let Some(text) = s {
                tarutil::append_bytes(&mut b, &format!("./{name}"), text.as_bytes(), 0o755, mtime)?;
            }
        }
        let enc = b.into_inner()?;
        enc.finish_box()?;
    }

    // --- data.tar.xz (dans un fichier temporaire pour connaître sa taille)
    let mut data_tmp = tempfile::tempfile()?;
    {
        let enc = compress::encoder(Codec::Xz, std::io::BufWriter::new(&mut data_tmp), None)?;
        let mut b = tar::Builder::new(enc);
        tarutil::append_root(&mut b, mtime)?;
        for (p, e) in tree.entries.iter() {
            tarutil::append_entry(&mut b, tree, p, e, &opts)?;
        }
        let enc = b.into_inner()?;
        enc.finish_box()?;
    }
    let data_size = data_tmp.seek(SeekFrom::End(0))?;
    data_tmp.seek(SeekFrom::Start(0))?;

    // --- assemblage ar
    let file = File::create(out).with_context(|| format!("création de {}", out.display()))?;
    let mut w = ar::Writer::new(std::io::BufWriter::new(file))?;
    w.add_bytes("debian-binary", b"2.0\n", mtime)?;
    w.add_bytes("control.tar.xz", &ctl, mtime)?;
    w.add_reader("data.tar.xz", &mut data_tmp, data_size, mtime)?;
    let mut inner = w.finish()?;
    inner.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dep_list() {
        let d = parse_dep_list("libc6 (>= 2.34), libgtk-3-0 | libgtk-4-1, python3:any, ${misc:Depends}, foo [amd64], bar (<< 2.9) <!nocheck>");
        assert_eq!(d.len(), 5);
        assert_eq!(d[4].name, "bar");
        assert_eq!(d[4].op, Some(crate::model::Op::Lt));
        assert_eq!(d[4].version.as_deref(), Some("2.9"));
        assert_eq!(d[1].alternatives[0].name, "libgtk-4-1");
        assert_eq!(d[2].name, "python3");
        assert_eq!(d[3].name, "foo");
    }

    #[test]
    fn control_parse() {
        let f = parse_control("Package: hello\nDescription: short\n long line\n .\n more\n");
        assert_eq!(field(&f, "package"), Some("hello"));
        let desc = field(&f, "Description").unwrap();
        let (_, l) = desc.split_once('\n').unwrap();
        assert_eq!(clean_long_description(l), "long line\n\nmore");
    }
}
