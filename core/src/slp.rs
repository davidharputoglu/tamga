//! Paquets Stampede Linux (.slp) : une archive tar compressée (bzip2 ou gzip)
//! suivie d'un bloc d'informations de 3784 octets (format repris d'Alien).
//! Stampede Linux (.slp) package reader and writer.

use crate::compress::{self, Codec};
use crate::model::{Family, Format, Metadata, Package};
use crate::tarutil::{self, Sink, TarOpts};
use crate::tree::{Kind, Tree};
use crate::version;
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const FOOTER_SIZE: u64 = 3784;
const FOOTER_VERSION: u32 = 5;

/// Champs du bloc final, dans l'ordre : (nom, taille ; 0 = entier sur 4 octets).
const FIELDS: &[(&str, usize)] = &[
    ("conffiles", 756),
    ("priority", 0),
    ("compresstype", 0),
    ("release", 0),
    ("copyright", 0),
    ("conflicts", 128),
    ("setupscript", 128),
    ("summary", 80),
    ("description", 1536),
    ("depends", 512),
    ("provides", 512),
    ("maintainer", 30),
    ("date", 30),
    ("compiler", 0),
    ("version", 20),
    ("name", 20),
    ("arch", 0),
    ("group", 0),
    ("slpkgversion", 0),
];

#[derive(Debug, Default)]
pub struct Footer {
    pub text: std::collections::HashMap<&'static str, String>,
    pub num: std::collections::HashMap<&'static str, u32>,
}

pub fn decode_footer(b: &[u8]) -> Result<Footer> {
    if b.len() != FOOTER_SIZE as usize {
        bail!("bloc d'informations .slp incomplet");
    }
    let mut f = Footer::default();
    let mut pos = 0;
    for (name, size) in FIELDS {
        if *size == 0 {
            let v = u32::from_le_bytes(b[pos..pos + 4].try_into().unwrap());
            f.num.insert(name, v);
            pos += 4;
        } else {
            let raw = &b[pos..pos + size];
            let s = String::from_utf8_lossy(raw).trim_end_matches(['\0', ' ']).to_string();
            f.text.insert(name, s);
            pos += size;
        }
    }
    Ok(f)
}

/// Lit le bloc final d'un fichier .slp.
pub fn read_footer(path: &Path) -> Result<Footer> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    if len <= FOOTER_SIZE {
        bail!("fichier .slp trop court");
    }
    f.seek(SeekFrom::Start(len - FOOTER_SIZE))?;
    let mut b = vec![0u8; FOOTER_SIZE as usize];
    f.read_exact(&mut b)?;
    decode_footer(&b)
}

/// Le fichier ressemble-t-il à un .slp ? (version du format plausible)
pub fn looks_like_slp(path: &Path) -> bool {
    read_footer(path).map(|f| (1..=20).contains(f.num.get("slpkgversion").unwrap_or(&0))).unwrap_or(false)
}

fn arch_from_num(n: u32) -> &'static str {
    match n {
        1 => "i686",
        2 => "sparc",
        3 => "alpha",
        4 => "ppc",
        5 => "m68k",
        _ => "noarch",
    }
}

fn arch_to_num(a: &str) -> Option<u32> {
    match version::canonical_arch(a).as_str() {
        "noarch" => Some(0),
        "i686" => Some(1),
        "sparc" => Some(2),
        "alpha" => Some(3),
        "ppc" | "powerpc" => Some(4),
        "m68k" => Some(5),
        _ => None,
    }
}

pub fn read(path: &Path) -> Result<Package> {
    let footer = read_footer(path)?;
    let ver = *footer.num.get("slpkgversion").unwrap_or(&0);
    if ver < FOOTER_VERSION {
        bail!("err.slp_version");
    }
    let mut f = File::open(path).with_context(|| format!("ouverture de {}", path.display()))?;
    let len = f.metadata()?.len();
    f.seek(SeekFrom::Start(0))?;
    let data = BufReader::with_capacity(256 * 1024, f.take(len - FOOTER_SIZE));
    let mut dec = compress::auto_decoder(data)?;
    let mut tree = Tree::new()?;
    let (_, special) = tarutil::read_tar_into(&mut dec, &mut tree, |_| Sink::Tree)?;
    let t = |k: &str| footer.text.get(k).cloned().unwrap_or_default();
    for c in t("conffiles").split(':') {
        if let Some(e) = tree.entries.get_mut(c.trim().trim_start_matches('/')) {
            e.config = true;
        }
    }
    let license = match footer.num.get("copyright").copied().unwrap_or(254) {
        0 => "GPL",
        1 => "BSD",
        2 => "LGPL",
        _ => "",
    };
    let mut warnings = Vec::new();
    if special > 0 {
        warnings.push("warn.special_files".into());
    }
    // Stampede ne connaissait que i386 : on se fie aux programmes contenus si besoin.
    let mut arch = arch_from_num(*footer.num.get("arch").unwrap_or(&0)).to_string();
    if arch == "noarch" {
        if let Some(m) = crate::elf::scan(&tree).machine {
            arch = m;
        }
    }
    let meta = Metadata {
        name: t("name"),
        version: t("version"),
        release: footer.num.get("release").map(|r| r.to_string()).unwrap_or_default(),
        arch,
        summary: t("summary"),
        description: t("description"),
        license: license.into(),
        maintainer: t("maintainer"),
        dep_family: Some(Family::Generic),
        origin_format: Some(Format::Slp),
        ..Default::default()
    };
    Ok(Package { meta, tree, format: Format::Slp, warnings })
}

/// Texte terminé par des zéros, coupé à la bonne taille (comme « pack a<n> »).
fn put_text(out: &mut Vec<u8>, s: &str, size: usize) {
    let mut b = s.as_bytes().to_vec();
    b.truncate(size);
    b.resize(size, 0);
    out.extend_from_slice(&b);
}

/// Date lisible (UTC), façon « scalar localtime » de Perl : « Sun Nov  1 17:50:00 2026 ».
fn ctime(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // Algorithme « civil from days » (H. Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let mn = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(mo - 1) as usize];
    format!("{wd} {mn} {d:>2} {h:02}:{m:02}:{s:02} {y}")
}

/// Écrit un .slp. Retourne vrai si l'architecture a dû être notée « toutes ».
pub fn write(meta: &Metadata, tree: &Tree, out: &Path) -> Result<bool> {
    let file = File::create(out).with_context(|| format!("création de {}", out.display()))?;
    let mut w = BufWriter::new(file);
    {
        let enc = compress::encoder(Codec::Bzip2, &mut w, Some(9))?;
        let mut b = tar::Builder::new(enc);
        let opts = TarOpts { dot_prefix: true, root_entry: false, numeric_owner_fallback: true };
        for (p, e) in tree.entries.iter() {
            tarutil::append_entry(&mut b, tree, p, e, &opts)?;
        }
        let enc = b.into_inner()?;
        enc.finish_box()?;
    }
    let conffiles: Vec<String> = tree
        .entries
        .iter()
        .filter(|(_, e)| e.config && matches!(e.kind, Kind::File(_)))
        .map(|(p, _)| format!("/{p}"))
        .collect();
    let (arch, arch_unknown) = match arch_to_num(&meta.arch) {
        Some(n) => (n, false),
        None => (0, true),
    };
    let release: u32 = meta.release.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(1);
    let maint = if meta.maintainer.trim().is_empty() { crate::DEFAULT_MAINTAINER } else { meta.maintainer.trim() };
    let mut footer = Vec::with_capacity(FOOTER_SIZE as usize);
    for (name, size) in FIELDS {
        match (*name, *size) {
            ("conffiles", s) => put_text(&mut footer, &conffiles.join(":"), s),
            ("priority", _) => footer.extend_from_slice(&2u32.to_le_bytes()), // optionnel
            ("compresstype", _) => footer.extend_from_slice(&0u32.to_le_bytes()), // bzip2
            ("release", _) => footer.extend_from_slice(&release.to_le_bytes()),
            ("copyright", _) => footer.extend_from_slice(&254u32.to_le_bytes()), // inconnu
            ("summary", s) => put_text(&mut footer, meta.summary.lines().next().unwrap_or(""), s),
            ("description", s) => put_text(&mut footer, &meta.description, s),
            ("maintainer", s) => put_text(&mut footer, maint, s),
            ("date", s) => put_text(&mut footer, &ctime(tarutil::now()), s),
            ("compiler", _) => footer.extend_from_slice(&252u32.to_le_bytes()), // inconnu
            ("version", s) => put_text(&mut footer, &meta.version, s),
            ("name", s) => put_text(&mut footer, &meta.name, s),
            ("arch", _) => footer.extend_from_slice(&arch.to_le_bytes()),
            ("group", _) => footer.extend_from_slice(&252u32.to_le_bytes()), // inconnu
            ("slpkgversion", _) => footer.extend_from_slice(&FOOTER_VERSION.to_le_bytes()),
            (_, s) if s > 0 => put_text(&mut footer, "", s), // conflicts, setupscript, depends, provides
            _ => footer.extend_from_slice(&0u32.to_le_bytes()),
        }
    }
    debug_assert_eq!(footer.len() as u64, FOOTER_SIZE);
    w.write_all(&footer)?;
    w.flush()?;
    Ok(arch_unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_layout() {
        let total: usize = FIELDS.iter().map(|(_, s)| if *s == 0 { 4 } else { *s }).sum();
        assert_eq!(total as u64, FOOTER_SIZE);
        assert_eq!(ctime(0), "Thu Jan  1 00:00:00 1970");
        assert_eq!(ctime(1_793_491_200), "Sun Nov  1 00:00:00 2026");
    }

    #[test]
    fn roundtrip() {
        let mut t = Tree::new().unwrap();
        t.add_file_bytes("usr/bin/hi", b"#!/bin/sh\necho hi\n", 0o755, 1).unwrap();
        t.add_file_bytes("etc/hi.conf", b"x=1\n", 0o644, 1).unwrap();
        t.entries.get_mut("etc/hi.conf").unwrap().config = true;
        let m = Metadata { name: "hi".into(), version: "1.0".into(), release: "3".into(), arch: "noarch".into(), summary: "Hi".into(), ..Default::default() };
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hi-1.0.slp");
        write(&m, &t, &p).unwrap();
        assert!(looks_like_slp(&p));
        let back = read(&p).unwrap();
        assert_eq!(back.meta.name, "hi");
        assert_eq!(back.meta.release, "3");
        assert_eq!(back.tree.read("usr/bin/hi").unwrap(), b"#!/bin/sh\necho hi\n");
        assert!(back.tree.get("etc/hi.conf").unwrap().config);
    }
}
