//! Paquets Slackware (.txz, .tgz, .tbz, .tlz) et archives tar simples
//! (.tar, .tar.gz, .tar.xz, .tar.bz2, .tar.zst, .tar.lz, .tar.lzma, .tar.Z, .tar.lz4…).
//! Slackware packages and plain tar archives: reader and writers.
//!
//! Un paquet Slackware est une archive tar rangée depuis la racine « / », avec un
//! dossier install/ : slack-desc (description) et doinst.sh (script lancé après
//! l'installation, qui crée aussi les liens symboliques et met en place les
//! fichiers de configuration « .new »).

use crate::compress::{self, Codec};
use crate::fsdirs::is_system_dir;
use crate::model::{Family, Format, Metadata, Package, ScriptStyle, Scripts};
use crate::tarutil::{self, Sink, TarOpts};
use crate::tree::{basename, normalize, parent, Entry, Kind, Tree};
use crate::version;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

/// Extensions des archives tar reconnues (de la plus longue à la plus courte).
pub const TAR_EXTS: &[&str] = &[
    ".tar.gz", ".tar.xz", ".tar.bz2", ".tar.zst", ".tar.zstd", ".tar.lzma", ".tar.lz4", ".tar.lz", ".tar.bz",
    ".tar.z", ".tar", ".tgz", ".txz", ".tbz2", ".tbz", ".tb2", ".tlz", ".tzst", ".taz",
];

/// Extensions propres aux paquets Slackware.
pub const SLACK_EXTS: &[&str] = &[".tgz", ".txz", ".tbz", ".tlz"];

/// Retire l'extension d'archive tar d'un nom de fichier.
pub fn strip_tar_ext(name: &str) -> &str {
    let low = name.to_ascii_lowercase();
    for e in TAR_EXTS {
        if low.ends_with(e) {
            return &name[..name.len() - e.len()];
        }
    }
    name
}

fn looks_like_slack_arch(a: &str) -> bool {
    matches!(a, "noarch" | "fw" | "x86_64" | "aarch64" | "arm" | "armv7" | "armv7hl" | "riscv64" | "s390x" | "ppc" | "ppc64" | "ppc64le")
        || (a.len() == 4 && a.starts_with('i') && a.ends_with("86"))
}

/// Découpe un nom Slackware « nom-version-arch-build » (build peut porter un tag : 1_SBo).
pub fn parse_slack_name(base: &str) -> Option<(String, String, String, String)> {
    let parts: Vec<&str> = base.split('-').collect();
    if parts.len() < 4 {
        return None;
    }
    let n = parts.len();
    let (build, arch, ver) = (parts[n - 1], parts[n - 2], parts[n - 3]);
    if !looks_like_slack_arch(arch) || !build.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some((parts[..n - 3].join("-"), ver.to_string(), arch.to_string(), build.to_string()))
}

/// Nom et version d'une archive quelconque, comme Alien : « nom-1.2.3… ».
pub fn guess_name_version(base: &str) -> (String, String) {
    let b: Vec<char> = base.chars().collect();
    for i in 1..b.len() {
        if b[i - 1] == '-' && b[i].is_ascii_digit() {
            let name: String = b[..i - 1].iter().collect();
            let rest: String = b[i..].iter().collect();
            let ver: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '+' || *c == '~').collect();
            if !name.is_empty() {
                return (name, ver.trim_end_matches('.').to_string());
            }
        }
    }
    (base.to_string(), "1".to_string())
}

/// Architecture Slackware → canonique.
fn slack_arch_to_canonical(a: &str) -> String {
    match a {
        "noarch" | "fw" => "noarch".into(),
        "arm" => "armv7".into(),
        other => version::canonical_arch(other),
    }
}

/// Architecture canonique → nom Slackware.
pub fn slack_arch(a: &str) -> String {
    match version::canonical_arch(a).as_str() {
        "i686" => "i586".into(),
        "armv7" => "arm".into(),
        other => other.into(),
    }
}

/// Lit slack-desc : (résumé, description).
pub fn parse_slack_desc(text: &str) -> (String, String) {
    let mut lines: Vec<String> = Vec::new();
    for l in text.lines() {
        if l.trim_start().starts_with('#') || l.trim_start().starts_with('|') {
            continue;
        }
        if let Some((_, rest)) = l.split_once(':') {
            lines.push(rest.strip_prefix(' ').unwrap_or(rest).trim_end().to_string());
        }
    }
    let first = lines.first().cloned().unwrap_or_default();
    // « hello (GNU hello world) » → « GNU hello world »
    let summary = match (first.find('('), first.rfind(')')) {
        (Some(a), Some(b)) if b > a => first[a + 1..b].trim().to_string(),
        _ => first.trim().to_string(),
    };
    let desc = lines.iter().skip(1).map(|s| s.as_str()).collect::<Vec<_>>().join("\n").trim().to_string();
    (summary, desc)
}

/// Résultat de l'analyse de doinst.sh.
struct DoInst {
    symlinks: Vec<(String, String)>, // (chemin, cible)
    configs: Vec<String>,            // chemins « .new »
    rest: String,                    // le reste du script
}

fn parse_doinst(text: &str) -> DoInst {
    let mut d = DoInst { symlinks: vec![], configs: vec![], rest: String::new() };
    let mut in_func = false;
    for line in text.lines() {
        let t = line.trim();
        if in_func {
            if line.starts_with('}') {
                in_func = false;
            }
            continue;
        }
        if t.starts_with("config()") || t.starts_with("preserve_perms()") || t.starts_with("schema_install()") {
            in_func = !t.ends_with('}');
            continue;
        }
        // ( cd usr/lib ; rm -rf libfoo.so )  /  ( cd usr/lib ; ln -sf libfoo.so.1 libfoo.so )
        if t.starts_with("( cd ") && t.ends_with(')') {
            let inner = t.trim_start_matches('(').trim_end_matches(')').trim();
            if let Some((cd, cmd)) = inner.split_once(';') {
                let dir = cd.trim().trim_start_matches("cd").trim();
                let cmd = cmd.trim();
                if cmd.starts_with("rm -rf ") {
                    continue;
                }
                if let Some(args) = cmd.strip_prefix("ln -sf ") {
                    let a: Vec<&str> = args.split_whitespace().collect();
                    if a.len() == 2 {
                        let path = if dir == "." || dir.is_empty() { a[1].to_string() } else { format!("{dir}/{}", a[1]) };
                        d.symlinks.push((path, a[0].to_string()));
                        continue;
                    }
                }
            }
        }
        if let Some(p) = t.strip_prefix("config ").or_else(|| t.strip_prefix("preserve_perms ")) {
            let p = p.trim().trim_matches('"');
            if p.ends_with(".new") && !p.contains(' ') {
                d.configs.push(p.trim_start_matches('/').to_string());
                continue;
            }
        }
        if t.starts_with("schema_install ") {
            continue;
        }
        d.rest.push_str(line);
        d.rest.push('\n');
    }
    d
}

fn script_is_trivial(s: &str) -> bool {
    s.lines().all(|l| {
        let t = l.trim();
        t.is_empty() || t.starts_with('#')
    })
}

/// Dossiers du premier niveau d'un système Linux.
const FHS_TOP: &[&str] = &["usr", "etc", "opt", "var", "bin", "sbin", "lib", "lib32", "lib64", "libx32", "srv", "boot"];

/// Lit un paquet Slackware ou une archive tar. `slack` : format annoncé par la détection.
pub fn read(path: &Path, as_slack: bool) -> Result<Package> {
    let f = File::open(path).with_context(|| format!("ouverture de {}", path.display()))?;
    let mut dec = compress::auto_decoder(BufReader::with_capacity(256 * 1024, f))?;
    let mut tree = Tree::new()?;
    let (kept, special) = tarutil::read_tar_into(&mut dec, &mut tree, |p| {
        if p.starts_with("install/") && !p[8..].contains('/') { Sink::Keep } else { Sink::Tree }
    })?;
    let get = |n: &str| kept.iter().find(|(p, _)| p == n).map(|(_, d)| String::from_utf8_lossy(d).to_string());
    let has_slack_meta = get("install/slack-desc").is_some() || get("install/doinst.sh").is_some();
    let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let base = strip_tar_ext(&file_name).to_string();
    let slack = has_slack_meta || (as_slack && parse_slack_name(&base).is_some());
    let mut warnings = Vec::new();
    if special > 0 {
        warnings.push("warn.special_files".into());
    }
    // Le dossier install/ des archives simples fait partie du contenu.
    tree.entries.remove("install");
    if !slack {
        for (p, data) in &kept {
            tree.add_file_bytes(p, data, 0o644, tarutil::now())?;
        }
    }

    let mut meta = Metadata { dep_family: Some(Family::Generic), ..Default::default() };
    if slack {
        meta.origin_format = Some(Format::Slack);
        if let Some((n, v, a, b)) = parse_slack_name(&base) {
            meta.name = n;
            meta.version = v;
            meta.arch = slack_arch_to_canonical(&a);
            meta.release = b.chars().take_while(|c| c.is_ascii_digit()).collect();
        } else {
            let (n, v) = guess_name_version(&base);
            meta.name = n;
            meta.version = v;
        }
        if let Some(desc) = get("install/slack-desc") {
            let (s, d) = parse_slack_desc(&desc);
            meta.summary = s;
            meta.description = d;
        }
        // doinst.sh : liens symboliques, fichiers .new, reste du script.
        if let Some(doinst) = get("install/doinst.sh") {
            let d = parse_doinst(&doinst);
            let mtime = tarutil::now();
            for (p, target) in &d.symlinks {
                if let Some(np) = normalize(p) {
                    tree.insert(&np, Entry::symlink(target, mtime));
                }
            }
            for c in &d.configs {
                if let Some(e) = tree.entries.remove(c) {
                    let real = c.trim_end_matches(".new").to_string();
                    let mut e = e;
                    e.config = true;
                    tree.entries.insert(real, e);
                }
            }
            if !script_is_trivial(&d.rest) {
                meta.scripts = Scripts { style: ScriptStyle::Plain, post_install: Some(d.rest), ..Default::default() };
            }
        }
    } else {
        meta.origin_format = Some(Format::Tar);
        let (n, v) = guess_name_version(&base);
        meta.name = n;
        meta.version = v;
        warnings.push("warn.tar_no_meta".into());
        // L'archive est-elle rangée comme un système (usr/, etc/…) ?
        let top: BTreeSet<String> = tree.entries.keys().map(|k| k.split('/').next().unwrap_or("").to_string()).collect();
        let fhs = |set: &BTreeSet<String>| !set.is_empty() && set.iter().all(|t| FHS_TOP.contains(&t.as_str()));
        if !fhs(&top) {
            let single_dir = top.len() == 1 && tree.get(top.iter().next().unwrap()).map(|e| e.is_dir()).unwrap_or(false);
            let inner_top: BTreeSet<String> = if single_dir {
                let t = top.iter().next().unwrap().clone();
                tree.entries
                    .keys()
                    .filter_map(|k| k.strip_prefix(&format!("{t}/")).map(|r| r.split('/').next().unwrap_or("").to_string()))
                    .collect()
            } else {
                BTreeSet::new()
            };
            // Dossier englobant qui contient un vrai système (usr/, etc/…) : on l'enlève.
            // Sinon (bin/, lib/ seuls, ou autre chose) : programme autonome rangé dans /opt/<nom>.
            let rooted_inside = fhs(&inner_top) && inner_top.iter().any(|t| matches!(t.as_str(), "usr" | "etc" | "opt" | "var"));
            let dest = if single_dir && rooted_inside { String::new() } else { format!("opt/{}", version::deb_name(&meta.name)) };
            let strip = if single_dir { format!("{}/", top.iter().next().unwrap()) } else { String::new() };
            relocate(&mut tree, &strip, &dest);
            if !dest.is_empty() {
                warnings.push("warn.tar_relocated".into());
                // Raccourcis /usr/bin vers les programmes de /opt/<nom>/bin.
                let bin = format!("{dest}/bin/");
                let exes: Vec<String> = tree
                    .entries
                    .iter()
                    .filter(|(k, e)| k.starts_with(&bin) && !k[bin.len()..].contains('/') && e.is_file() && e.mode & 0o111 != 0)
                    .map(|(k, _)| k.clone())
                    .collect();
                let mtime = tarutil::now();
                for k in exes {
                    let link = format!("usr/bin/{}", basename(&k));
                    if tree.get(&link).is_none() {
                        tree.insert(&link, Entry::symlink(&format!("/{k}"), mtime));
                    }
                }
            }
        }
    }
    if meta.summary.is_empty() {
        meta.summary = meta.name.clone();
    }
    if meta.arch.is_empty() {
        meta.arch = crate::elf::scan(&tree).machine.unwrap_or_else(|| "noarch".into());
    }
    // Comme Alien : les fichiers de etc/ sont des fichiers de configuration.
    for (p, e) in tree.entries.iter_mut() {
        if p.starts_with("etc/") && e.is_file() {
            e.config = true;
        }
    }
    let format = if slack { Format::Slack } else { Format::Tar };
    Ok(Package { meta, tree, format, warnings })
}

/// Déplace les entrées : retire `strip` au début des chemins et les range sous `dest`.
fn relocate(tree: &mut Tree, strip: &str, dest: &str) {
    let old = std::mem::take(&mut tree.entries);
    for (k, e) in old {
        let rel = if strip.is_empty() {
            k.clone()
        } else if let Some(r) = k.strip_prefix(strip) {
            r.to_string()
        } else {
            continue; // le dossier englobant lui-même
        };
        let newk = if dest.is_empty() { rel } else { format!("{dest}/{rel}") };
        if let Some(n) = normalize(&newk) {
            tree.insert(&n, e);
        }
    }
}

// ---------------------------------------------------------------------------
// Écriture
// ---------------------------------------------------------------------------

/// Liens symboliques (dossier, cible, nom) et fichiers de configuration (« .new »)
/// tels que makepkg les range dans doinst.sh.
pub fn slack_layout(tree: &Tree) -> (Vec<(String, String, String)>, Vec<String>) {
    let mut links = Vec::new();
    let mut configs = Vec::new();
    for (p, e) in tree.entries.iter() {
        match &e.kind {
            Kind::Symlink(t) => {
                links.push((parent(p).unwrap_or("").to_string(), t.clone(), basename(p).to_string()));
            }
            Kind::File(_) if e.config => configs.push(format!("{p}.new")),
            _ => {}
        }
    }
    (links, configs)
}

/// Texte de slack-desc (exactement 11 lignes « nom: »).
pub fn slack_desc(meta: &Metadata) -> String {
    let name = version::pacman_name(&meta.name);
    let sum = meta.summary.lines().next().unwrap_or("").trim();
    let mut lines = vec![if sum.is_empty() { format!("{name}") } else { format!("{name} ({sum})") }, String::new()];
    let mut cur = String::new();
    for w in meta.description.split_whitespace() {
        if cur.len() + w.len() + 1 > 70 {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines.truncate(11);
    while lines.len() < 11 {
        lines.push(String::new());
    }
    let mut s = String::from(
        "# HOW TO EDIT THIS FILE:\n# The \"handy ruler\" below makes it easier to edit a package description.\n# Line up the first '|' above the ':' following the base package name, and\n# the '|' on the right side marks the last column you can put a character in.\n# You must make exactly 11 lines for the formatting to be correct.  It's also\n# customary to leave one space after the ':' except on otherwise blank lines.\n\n",
    );
    s.push_str(&" ".repeat(name.len()));
    s.push_str("|-----handy-ruler------------------------------------------------------|\n");
    for l in lines {
        if l.is_empty() {
            s.push_str(&format!("{name}:\n"));
        } else {
            s.push_str(&format!("{name}: {l}\n"));
        }
    }
    s
}

/// Écrit un paquet Slackware. `doinst` : contenu de install/doinst.sh.
pub fn write_slack(meta: &Metadata, tree: &Tree, codec: Codec, doinst: Option<&str>, out: &Path) -> Result<()> {
    let now = tarutil::now();
    let file = File::create(out).with_context(|| format!("création de {}", out.display()))?;
    let enc = compress::encoder(codec, BufWriter::new(file), None)?;
    let mut b = tar::Builder::new(enc);
    tarutil::append_root(&mut b, now)?;
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Directory);
    h.set_mode(0o755);
    h.set_mtime(now);
    h.set_uid(0);
    h.set_gid(0);
    let _ = h.set_username("root");
    let _ = h.set_groupname("root");
    h.set_size(0);
    b.append_data(&mut h, "install/", std::io::empty())?;
    tarutil::append_bytes(&mut b, "install/slack-desc", slack_desc(meta).as_bytes(), 0o644, now)?;
    if let Some(d) = doinst {
        tarutil::append_bytes(&mut b, "install/doinst.sh", d.as_bytes(), 0o755, now)?;
    }
    let opts = TarOpts { dot_prefix: false, root_entry: false, numeric_owner_fallback: true };
    for (p, e) in tree.entries.iter() {
        match &e.kind {
            Kind::Symlink(_) => continue, // recréés par doinst.sh
            Kind::File(_) if e.config => {
                tarutil::append_entry(&mut b, tree, &format!("{p}.new"), e, &opts)?;
            }
            _ => tarutil::append_entry(&mut b, tree, p, e, &opts)?,
        }
    }
    let enc = b.into_inner()?;
    enc.finish_box()?;
    Ok(())
}

/// Écrit une archive tar simple, à extraire à la racine (« tar -xpf … -C / »).
/// Les dossiers du système ne sont pas inclus, pour ne pas modifier leurs droits.
pub fn write_tar(tree: &Tree, codec: Codec, out: &Path) -> Result<()> {
    let file = File::create(out).with_context(|| format!("création de {}", out.display()))?;
    let enc = compress::encoder(codec, BufWriter::new(file), None)?;
    let mut b = tar::Builder::new(enc);
    let opts = TarOpts { dot_prefix: true, root_entry: false, numeric_owner_fallback: true };
    for (p, e) in tree.entries.iter() {
        if e.is_dir() && is_system_dir(p) {
            continue;
        }
        tarutil::append_entry(&mut b, tree, p, e, &opts)?;
    }
    let enc = b.into_inner()?;
    enc.finish_box()?;
    Ok(())
}

/// Extension de fichier d'un paquet Slackware selon la compression.
pub fn slack_ext(codec: Codec) -> &'static str {
    match codec {
        Codec::Gzip => ".tgz",
        Codec::Bzip2 => ".tbz",
        _ => ".txz",
    }
}

/// Extension d'une archive tar selon la compression.
pub fn tar_ext(codec: Codec) -> String {
    format!(".tar{}", codec.extension())
}

/// Liste des chemins d'un tar (pour les tests).
pub fn list_tar(path: &Path) -> Result<BTreeMap<String, u8>> {
    let f = File::open(path)?;
    let dec = compress::auto_decoder(BufReader::new(f))?;
    let mut ar = tar::Archive::new(dec);
    let mut out = BTreeMap::new();
    for e in ar.entries()? {
        let e = e?;
        let p = String::from_utf8_lossy(&e.path_bytes()).to_string();
        out.insert(p, e.header().entry_type().as_byte());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(
            parse_slack_name("hello-2.12.1-x86_64-1"),
            Some(("hello".into(), "2.12.1".into(), "x86_64".into(), "1".into()))
        );
        assert_eq!(
            parse_slack_name("kernel-generic-6.1.106-x86_64-1_slack15.0"),
            Some(("kernel-generic".into(), "6.1.106".into(), "x86_64".into(), "1_slack15.0".into()))
        );
        assert_eq!(parse_slack_name("foo-1.0"), None);
        assert_eq!(guess_name_version("foo-bar-1.2.3-linux-x86_64"), ("foo-bar".into(), "1.2.3".into()));
        assert_eq!(guess_name_version("thing"), ("thing".into(), "1".into()));
        assert_eq!(strip_tar_ext("a-1.0.tar.gz"), "a-1.0");
        assert_eq!(strip_tar_ext("a-1.0-x86_64-1.txz"), "a-1.0-x86_64-1");
        assert_eq!(slack_arch("i686"), "i586");
    }

    #[test]
    fn doinst_parsing() {
        let d = parse_doinst(
            "config() {\n  NEW=\"$1\"\n  true\n}\nconfig etc/foo.conf.new\n( cd usr/lib ; rm -rf libfoo.so )\n( cd usr/lib ; ln -sf libfoo.so.1 libfoo.so )\nif [ -x /usr/bin/update-desktop-database ]; then\n  /usr/bin/update-desktop-database -q\nfi\n",
        );
        assert_eq!(d.symlinks, vec![("usr/lib/libfoo.so".to_string(), "libfoo.so.1".to_string())]);
        assert_eq!(d.configs, vec!["etc/foo.conf".to_string() + ".new"]);
        assert!(d.rest.contains("update-desktop-database"));
        assert!(!d.rest.contains("config"));
    }

    #[test]
    fn desc() {
        let m = Metadata { name: "hello".into(), summary: "GNU hello".into(), description: "Prints a greeting.".into(), ..Default::default() };
        let d = slack_desc(&m);
        assert_eq!(d.lines().filter(|l| l.starts_with("hello:")).count(), 11);
        let (s, l) = parse_slack_desc(&d);
        assert_eq!(s, "GNU hello");
        assert_eq!(l, "Prints a greeting.");
    }
}
