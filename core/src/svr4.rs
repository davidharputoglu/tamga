//! Paquets Solaris au format SVR4 « datastream » (.pkg, .pkg.gz) — lecture seule.
//! Solaris SVR4 package datastream reader.
//!
//! Structure (écrite par pkgtrans) :
//!   « # PaCkAgE DaTaStReAm »                     en-tête texte
//!   « <paquet> <nb parties> <taille max> »       une ligne par paquet
//!   « # end of header »                         puis bourrage jusqu'à 512 octets
//!   archive cpio : <paquet>/pkginfo, <paquet>/pkgmap (de tous les paquets)
//!   pour chaque paquet, une archive cpio par partie : pkginfo, pkgmap, install/,
//!   reloc/ (chemins relatifs à BASEDIR), root/ (chemins absolus), archive/
//! Chaque archive cpio est complétée jusqu'à un multiple de 512 octets.
//!
//! Note : les programmes Solaris ne fonctionnent pas sous Linux ; seuls les
//! scripts, documents et données sont réellement utilisables.

use crate::compress;
use crate::cpio::{self, Counting};
use crate::model::{Family, Format, Metadata, Package, ScriptStyle, Scripts};
use crate::tree::{normalize, parent, Entry, Kind, Tree};
use crate::version;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub const MAGIC: &str = "# PaCkAgE DaTaStReAm";

pub fn is_datastream(head: &[u8]) -> bool {
    head.starts_with(MAGIC.as_bytes())
}

/// Avance jusqu'au prochain multiple de 512 octets.
fn align512(c: &mut Counting) -> Result<()> {
    let rem = c.count % 512;
    if rem != 0 {
        let n = 512 - rem;
        let mut buf = vec![0u8; n as usize];
        // Fin de fichier possible juste après la dernière archive.
        let mut got = 0;
        while got < buf.len() {
            let k = c.read(&mut buf[got..])?;
            if k == 0 {
                break;
            }
            got += k;
        }
    }
    Ok(())
}

/// Lit une ligne de texte (jusqu'à « \n »).
fn read_line(c: &mut Counting) -> Result<String> {
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    loop {
        if c.read(&mut b)? == 0 {
            break;
        }
        if b[0] == b'\n' {
            break;
        }
        line.push(b[0]);
        if line.len() > 4096 {
            bail!("en-tête Solaris invalide");
        }
    }
    Ok(String::from_utf8_lossy(&line).to_string())
}

/// pkginfo : lignes CLÉ=valeur (valeurs parfois entre guillemets).
pub fn parse_pkginfo(text: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for l in text.lines() {
        if l.trim_start().starts_with('#') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            m.insert(k.trim().to_string(), v);
        }
    }
    m
}

/// Une ligne de pkgmap.
#[derive(Debug, Clone)]
pub struct MapEntry {
    pub part: u32,
    pub ftype: char,
    pub class: String,
    pub path: String,
    pub target: Option<String>,
    pub mode: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mtime: u64,
}

pub fn parse_pkgmap(text: &str) -> Vec<MapEntry> {
    let mut out = Vec::new();
    for l in text.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.is_empty() || f[0].starts_with(':') || f[0].starts_with('#') {
            continue;
        }
        // Le numéro de partie est parfois absent.
        let (part, rest) = match f[0].parse::<u32>() {
            Ok(p) => (p, &f[1..]),
            Err(_) => (1, &f[..]),
        };
        if rest.is_empty() {
            continue;
        }
        let ftype = rest[0].chars().next().unwrap_or('?');
        if ftype == 'i' {
            if let Some(p) = rest.get(1) {
                out.push(MapEntry { part, ftype, class: String::new(), path: p.to_string(), target: None, mode: None, owner: None, group: None, mtime: 0 });
            }
            continue;
        }
        let class = rest.get(1).map(|s| s.to_string()).unwrap_or_default();
        let Some(pathspec) = rest.get(2) else { continue };
        let (path, target) = match pathspec.split_once('=') {
            Some((p, t)) => (p.to_string(), Some(t.to_string())),
            None => (pathspec.to_string(), None),
        };
        let num = |i: usize| rest.get(i).and_then(|s| u32::from_str_radix(s, 8).ok());
        let txt = |i: usize| rest.get(i).filter(|s| **s != "?").map(|s| s.to_string());
        let (mode, owner, group, mtime) = match ftype {
            'f' | 'e' | 'v' => (num(3), txt(4), txt(5), rest.get(8).and_then(|s| s.parse().ok()).unwrap_or(0)),
            'd' | 'x' | 'p' => (num(3), txt(4), txt(5), 0),
            'b' | 'c' => (num(5), txt(6), txt(7), 0),
            _ => (None, None, None, 0),
        };
        out.push(MapEntry { part, ftype, class, path, target, mode, owner, group, mtime });
    }
    out
}

/// Remplace $BASEDIR et les autres variables de pkginfo dans un chemin.
fn expand(path: &str, info: &HashMap<String, String>) -> String {
    if !path.contains('$') {
        return path.to_string();
    }
    let mut out = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let mut name = String::new();
        while let Some(&n) = chars.peek() {
            if n.is_ascii_alphanumeric() || n == '_' {
                name.push(n);
                chars.next();
            } else {
                break;
            }
        }
        if let Some(v) = info.get(&name) {
            out.push_str(v);
        }
    }
    out
}

/// Nom de paquet Linux à partir du nom Solaris (« CSWcurl » → « curl »).
pub fn linux_name(pkg: &str) -> String {
    let upper: usize = pkg.chars().take_while(|c| c.is_ascii_uppercase()).count();
    let rest = &pkg[upper..];
    let base = if (2..=5).contains(&upper) && !rest.is_empty() { rest } else { pkg };
    version::deb_name(base)
}

fn solaris_arch(a: &str) -> String {
    let first = a.split([',', ' ']).next().unwrap_or("").trim();
    match first.to_ascii_lowercase().as_str() {
        "all" | "" => "noarch".into(),
        "i386" | "i86pc" => "i686".into(),
        "amd64" | "x86_64" => "x86_64".into(),
        "sparc" | "sparcv9" | "sun4u" | "sun4v" => "sparc".into(),
        other => other.into(),
    }
}

/// Lit un paquet Solaris (le premier si le fichier en contient plusieurs).
pub fn read(path: &Path) -> Result<Package> {
    let f = File::open(path).with_context(|| format!("ouverture de {}", path.display()))?;
    let mut dec = compress::auto_decoder(BufReader::with_capacity(256 * 1024, f))?;
    let mut c = Counting { inner: &mut dec, count: 0 };

    // --- en-tête
    let first = read_line(&mut c)?;
    if !first.starts_with(MAGIC) {
        bail!("ce fichier n'est pas un paquet Solaris");
    }
    let mut pkgs: Vec<(String, u32)> = Vec::new();
    loop {
        let l = read_line(&mut c)?;
        if l.starts_with("# end of header") {
            break;
        }
        if l.is_empty() && c.count > 64 * 1024 {
            bail!("en-tête Solaris sans fin");
        }
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() >= 2 {
            pkgs.push((f[0].to_string(), f[1].parse().unwrap_or(1)));
        }
    }
    let Some((pkginst, nparts)) = pkgs.first().cloned() else { bail!("aucun paquet dans ce fichier Solaris") };
    align512(&mut c)?;

    // --- archive 1 : pkginfo et pkgmap de tous les paquets
    let mut work = Tree::new()?;
    cpio::read_into_counted(&mut c, &mut work)?;
    align512(&mut c)?;
    // --- parties du premier paquet, rangées sous « __part/ »
    let mut part_tree = Tree::new()?;
    // On partage le stockage pour pouvoir déplacer les contenus sans copie.
    part_tree.store = work.store.clone();
    for _ in 0..nparts.max(1) {
        match cpio::read_into_counted(&mut c, &mut part_tree) {
            Ok(_) => align512(&mut c)?,
            Err(e) => {
                if part_tree.entries.is_empty() {
                    return Err(e);
                }
                break;
            }
        }
    }

    let text = |t: &Tree, p: &str| t.read(p).ok().map(|d| String::from_utf8_lossy(&d).to_string());
    let pkginfo_txt = text(&work, &format!("{pkginst}/pkginfo")).or_else(|| text(&part_tree, "pkginfo")).context("pkginfo manquant")?;
    let pkgmap_txt = text(&work, &format!("{pkginst}/pkgmap")).or_else(|| text(&part_tree, "pkgmap")).context("pkgmap manquant")?;
    let info = parse_pkginfo(&pkginfo_txt);
    let map = parse_pkgmap(&pkgmap_txt);
    let basedir = info.get("BASEDIR").cloned().unwrap_or_else(|| "/".into());

    // Archives de classe compressées (archive/none.bz2…) : lues à part.
    let mut class_tree = Tree::new()?;
    class_tree.store = work.store.clone();
    let class_files: Vec<String> = part_tree.entries.keys().filter(|k| k.starts_with("archive/")).cloned().collect();
    for k in &class_files {
        if let Ok(data) = part_tree.read(k) {
            let class = crate::tree::basename(k).split('.').next().unwrap_or("none").to_string();
            let mut dec = compress::auto_decoder(&data[..])?;
            let mut sub = Tree::new()?;
            sub.store = work.store.clone();
            if cpio::read_into(&mut dec, &mut sub).is_ok() {
                for (p, e) in sub.entries {
                    class_tree.entries.insert(format!("{class}/{p}"), e);
                }
            }
        }
    }

    // --- construction de l'arbre final d'après pkgmap
    let mut tree = Tree::new()?;
    tree.store = work.store.clone();
    let lookup = |part: u32, rel: Option<&str>, abs: &str, class: &str| -> Option<Entry> {
        let mut cands: Vec<String> = Vec::new();
        if let Some(r) = rel {
            cands.push(format!("reloc/{r}"));
            cands.push(format!("reloc.{part}/{r}"));
            cands.push(format!("{class}/{r}"));
        }
        cands.push(format!("root/{abs}"));
        cands.push(format!("root.{part}/{abs}"));
        cands.push(format!("reloc/{abs}"));
        cands.push(format!("{class}/{abs}"));
        for c in cands {
            if let Some(e) = part_tree.get(&c).or_else(|| class_tree.get(&c)) {
                return Some(e.clone());
            }
        }
        None
    };
    let mut hardlinks: Vec<(String, String)> = Vec::new();
    let mut scripts = Scripts { style: ScriptStyle::Plain, ..Default::default() };
    let mut missing = 0usize;
    for m in &map {
        if m.ftype == 'i' {
            let body = text(&part_tree, &format!("install/{}", m.path));
            match m.path.as_str() {
                "preinstall" => scripts.pre_install = body,
                "postinstall" => scripts.post_install = body,
                "preremove" => scripts.pre_remove = body,
                "postremove" => scripts.post_remove = body,
                _ => {}
            }
            continue;
        }
        let expanded = expand(&m.path, &info);
        let (rel, abs_path) = if expanded.starts_with('/') {
            (None, expanded.clone())
        } else {
            (Some(expanded.as_str()), format!("{}/{}", basedir.trim_end_matches('/'), expanded))
        };
        let Some(dest) = normalize(&abs_path) else { continue };
        let rel_norm = rel.and_then(normalize);
        let mode = m.mode.unwrap_or(match m.ftype {
            'd' | 'x' => 0o755,
            _ => 0o644,
        }) & 0o7777;
        let mut entry = match m.ftype {
            'd' | 'x' => Entry::dir(mode, m.mtime),
            'f' | 'e' | 'v' => match lookup(m.part, rel_norm.as_deref(), &dest, &m.class) {
                Some(found) if found.is_file() => {
                    let mut e = found;
                    e.mode = mode;
                    if m.mtime > 0 {
                        e.mtime = m.mtime;
                    }
                    e.config = m.ftype == 'e' || m.ftype == 'v';
                    e
                }
                _ => {
                    missing += 1;
                    continue;
                }
            },
            's' => match &m.target {
                Some(t) => Entry::symlink(&expand(t, &info), m.mtime),
                None => continue,
            },
            'l' => {
                if let Some(t) = &m.target {
                    hardlinks.push((dest.clone(), expand(t, &info)));
                }
                continue;
            }
            _ => continue, // périphériques, tubes
        };
        entry.user = m.owner.clone().unwrap_or_else(|| "root".into());
        entry.group = m.group.clone().unwrap_or_else(|| "root".into());
        entry.uid = if entry.user == "root" { 0 } else { entry.uid };
        entry.gid = if entry.group == "root" { 0 } else { entry.gid };
        tree.insert(&dest, entry);
    }
    // Liens physiques : même contenu que la cible.
    for (dest, target) in hardlinks {
        let t = if target.starts_with('/') {
            normalize(&target)
        } else {
            let base = if target.contains('/') && !target.starts_with('.') {
                format!("{}/{}", basedir.trim_end_matches('/'), target)
            } else {
                format!("{}/{}", parent(&dest).unwrap_or(""), target)
            };
            normalize(&base)
        };
        if let Some(e) = t.and_then(|t| tree.get(&t).cloned()) {
            if matches!(e.kind, Kind::File(_)) {
                tree.insert(&dest, e);
            }
        }
    }

    let mut warnings = vec!["warn.solaris".to_string()];
    if pkgs.len() > 1 {
        warnings.push("warn.solaris_multi".into());
    }
    if missing > 0 {
        warnings.push("warn.solaris_missing".into());
    }
    let mut ver = info.get("VERSION").cloned().unwrap_or_else(|| "1".into());
    if let Some((v, _)) = ver.split_once(',') {
        ver = v.trim().to_string();
    }
    let ver = ver.split_whitespace().next().unwrap_or("1").to_string();
    let long_name = info.get("NAME").cloned().unwrap_or_default();
    let summary = match long_name.split_once(" - ") {
        Some((_, s)) => s.trim().to_string(),
        None => long_name.clone(),
    };
    let vendor = info.get("VENDOR").cloned().unwrap_or_default();
    let email = info.get("EMAIL").cloned().unwrap_or_default();
    let maintainer = match (vendor.is_empty(), email.is_empty()) {
        (false, false) => format!("{vendor} <{email}>"),
        (false, true) => vendor,
        (true, false) => email,
        _ => String::new(),
    };
    if scripts.is_empty() {
        scripts.style = ScriptStyle::None;
    }
    let meta = Metadata {
        name: linux_name(&info.get("PKG").cloned().unwrap_or(pkginst)),
        version: ver,
        release: "1".into(),
        arch: solaris_arch(info.get("ARCH").map(|s| s.as_str()).unwrap_or("")),
        summary: if summary.is_empty() { info.get("DESC").cloned().unwrap_or_default() } else { summary },
        description: info.get("DESC").cloned().unwrap_or_default(),
        license: String::new(),
        url: info.get("HOTLINE").filter(|h| h.starts_with("http")).cloned().unwrap_or_default(),
        maintainer,
        section: info.get("CATEGORY").cloned().unwrap_or_default(),
        dep_family: Some(Family::Generic),
        scripts,
        origin_format: Some(Format::Solaris),
        ..Default::default()
    };
    Ok(Package { meta, tree, format: Format::Solaris, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpio::Writer;

    fn cpio_of(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut t = Tree::new().unwrap();
        for (p, d, m) in files {
            if d.is_empty() && *m & 0o040000 != 0 {
                t.insert(p, Entry::dir(m & 0o7777, 0));
            } else {
                t.add_file_bytes(p, d, *m, 0).unwrap();
            }
        }
        let mut out = Vec::new();
        {
            let mut w = Writer::new(&mut out);
            for (p, e) in t.entries.iter() {
                w.add(&t, p, e).unwrap();
            }
            w.finish().unwrap();
        }
        // Les noms sont écrits « ./x » par notre écrivain : sans importance (normalisés).
        while out.len() % 512 != 0 {
            out.push(0);
        }
        out
    }

    #[test]
    fn datastream() {
        let pkginfo = b"PKG=\"CSWhello\"\nNAME=\"hello - GNU hello world\"\nARCH=\"i386\"\nVERSION=\"2.10,REV=2020.01.01\"\nBASEDIR=\"/opt/csw\"\nVENDOR=\"OpenCSW\"\nDESC=\"Friendly greeting\"\n";
        let pkgmap = b": 1 10\n1 d none bin 0755 root bin\n1 f none bin/hello 0755 root bin 6 123 0\n1 s none bin/hi=hello\n1 e none /etc/hello.conf 0644 root sys 4 1 0\n1 i pkginfo 100 1 0\n1 i postinstall 20 1 0\n";
        let mut ds = Vec::new();
        ds.extend_from_slice(b"# PaCkAgE DaTaStReAm\nCSWhello 1 10\n# end of header\n");
        while ds.len() % 512 != 0 {
            ds.push(0);
        }
        ds.extend(cpio_of(&[("CSWhello/pkginfo", pkginfo, 0o644), ("CSWhello/pkgmap", pkgmap, 0o644)]));
        ds.extend(cpio_of(&[
            ("pkginfo", pkginfo, 0o644),
            ("pkgmap", pkgmap, 0o644),
            ("install/postinstall", b"echo done\n", 0o755),
            ("reloc/bin/hello", b"HELLO\n", 0o755),
            ("root/etc/hello.conf", b"x=1\n", 0o644),
        ]));
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hello.pkg");
        std::fs::write(&p, &ds).unwrap();
        let pkg = read(&p).unwrap();
        assert_eq!(pkg.meta.name, "hello");
        assert_eq!(pkg.meta.version, "2.10");
        assert_eq!(pkg.meta.arch, "i686");
        assert_eq!(pkg.meta.summary, "GNU hello world");
        assert_eq!(pkg.tree.read("opt/csw/bin/hello").unwrap(), b"HELLO\n");
        assert!(matches!(&pkg.tree.get("opt/csw/bin/hi").unwrap().kind, Kind::Symlink(t) if t == "hello"));
        assert!(pkg.tree.get("etc/hello.conf").unwrap().config);
        assert_eq!(pkg.tree.get("opt/csw/bin/hello").unwrap().group, "bin");
        assert_eq!(pkg.meta.scripts.post_install.as_deref(), Some("echo done\n"));
    }
}
